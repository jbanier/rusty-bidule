use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc::UnboundedSender};

use crate::{
    azure::{AzureClient, AzureTool},
    config::AppConfig,
    conversation_store::ConversationStore,
    mcp_runtime::McpManager,
    tool_evidence::ToolEvidenceWriter,
    types::{ProgressEvent, RunTurnResult, UiEvent},
};

const MAX_AGENT_ITERATIONS: usize = 10;

#[derive(Clone)]
pub struct Orchestrator {
    inner: Arc<OrchestratorInner>,
}

struct OrchestratorInner {
    config: AppConfig,
    store: ConversationStore,
    evidence: ToolEvidenceWriter,
    azure: AzureClient,
    mcp: Mutex<McpManager>,
}

impl Orchestrator {
    pub fn new(config: AppConfig) -> Result<Self> {
        let data_dir = config.data_dir();
        let store = ConversationStore::new(&data_dir);
        store.init()?;
        let evidence = ToolEvidenceWriter::new(store.clone());
        let azure = AzureClient::new(&config.azure_openai);
        let mcp = McpManager::new(
            &data_dir,
            config.mcp_runtime.clone(),
            config.mcp_servers.clone(),
        )?;
        Ok(Self {
            inner: Arc::new(OrchestratorInner {
                config,
                store,
                evidence,
                azure,
                mcp: Mutex::new(mcp),
            }),
        })
    }

    pub fn store(&self) -> ConversationStore {
        self.inner.store.clone()
    }

    pub async fn login_mcp_server(&self, server_name: &str) -> Result<()> {
        let mut manager = self.inner.mcp.lock().await;
        manager.login_server(server_name).await
    }

    pub async fn ensure_default_conversation(&self) -> Result<String> {
        let conversations = self.inner.store.list_conversations()?;
        if let Some(conversation) = conversations.first() {
            Ok(conversation.conversation_id.clone())
        } else {
            Ok(self.inner.store.create_conversation()?.conversation_id)
        }
    }

    pub async fn run_turn(
        &self,
        conversation_id: &str,
        user_message: String,
        ui_tx: UnboundedSender<UiEvent>,
    ) -> Result<RunTurnResult> {
        self.emit(&ui_tx, "turn_started", "Recording user message", None, None);
        self.inner
            .store
            .append_message(conversation_id, "user", &user_message)?;
        self.inner.store.append_log(
            conversation_id,
            &format!("user message received: {}", user_message.replace('\n', " ")),
        )?;

        let history = self.inner.store.load(conversation_id)?.messages;
        let (mcp_tools, mcp_error) = {
            let mut manager = self.inner.mcp.lock().await;
            match manager.list_tools().await {
                Ok(tools) => (tools, None),
                Err(err) => (Vec::new(), Some(err.to_string())),
            }
        };

        if let Some(error) = mcp_error.as_deref() {
            self.inner
                .store
                .append_log(conversation_id, &format!("MCP discovery degraded: {error}"))?;
        }

        let azure_tools: Vec<AzureTool> = mcp_tools
            .iter()
            .map(|tool| AzureTool {
                name: tool.external_name.clone(),
                description: format!("{} (server: {})", tool.description, tool.server_name),
                parameters: tool.input_schema.clone(),
            })
            .collect();

        let mut messages = build_messages(
            self.inner.config.prompt.as_deref(),
            &history,
            mcp_error.as_deref(),
        );
        let mut tool_call_count = 0usize;

        for iteration in 0..MAX_AGENT_ITERATIONS {
            let context_chars = estimate_context_chars(&messages);
            self.emit(
                &ui_tx,
                "context_size",
                &format!(
                    "Context size: ~{} chars across {} messages",
                    context_chars,
                    messages.len()
                ),
                None,
                Some(tool_call_count),
            );
            self.emit(
                &ui_tx,
                "llm_start",
                if iteration == 0 {
                    "Consulting Azure OpenAI"
                } else {
                    "Continuing tool-grounded reasoning"
                },
                None,
                Some(tool_call_count),
            );

            let completion = self
                .inner
                .azure
                .chat_completion(&messages, &azure_tools)
                .await
                .context("Azure completion failed")?;

            if !completion.tool_calls.is_empty() {
                messages.push(json!({
                    "role": "assistant",
                    "content": Value::Null,
                    "tool_calls": completion.tool_calls.iter().map(|call| {
                        json!({
                            "id": call.id,
                            "type": "function",
                            "function": {
                                "name": call.name,
                                "arguments": call.raw_arguments,
                            }
                        })
                    }).collect::<Vec<_>>()
                }));

                for tool_call in completion.tool_calls {
                    tool_call_count += 1;
                    self.emit(
                        &ui_tx,
                        "tool_start",
                        &format!("Calling {}", tool_call.name),
                        Some(tool_call.name.clone()),
                        Some(tool_call_count),
                    );

                    let tool_result = self
                        .execute_tool_call(
                            conversation_id,
                            &tool_call.name,
                            tool_call.arguments.clone(),
                        )
                        .await;

                    let tool_output = match tool_result {
                        Ok(output) => output,
                        Err(err) => format!("Tool call failed: {err}"),
                    };

                    self.emit(
                        &ui_tx,
                        "tool_end",
                        &format!("Finished {}", tool_call.name),
                        Some(tool_call.name.clone()),
                        Some(tool_call_count),
                    );

                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": tool_call.id,
                        "content": tool_output,
                    }));
                }
                continue;
            }

            let reply = completion
                .assistant_text
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "The model returned an empty response.".to_string());
            self.inner
                .store
                .append_message(conversation_id, "assistant", &reply)?;
            self.inner.store.append_log(
                conversation_id,
                &format!("assistant reply stored ({} chars)", reply.len()),
            )?;
            self.emit(
                &ui_tx,
                "turn_finished",
                "Reply ready",
                None,
                Some(tool_call_count),
            );
            return Ok(RunTurnResult {
                reply,
                tool_calls: tool_call_count,
            });
        }

        bail!(
            "agent exceeded the {}-iteration safety limit",
            MAX_AGENT_ITERATIONS
        )
    }

    async fn execute_tool_call(
        &self,
        conversation_id: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<String> {
        self.inner.store.append_log(
            conversation_id,
            &format!(
                "tool call started: {} args={} ",
                tool_name,
                serde_json::to_string(&arguments)?
            ),
        )?;

        let result = {
            let mut manager = self.inner.mcp.lock().await;
            manager.call_tool(tool_name, arguments.clone()).await
        };

        match result {
            Ok(output) => {
                self.inner.evidence.write_artifact(
                    conversation_id,
                    tool_name,
                    &arguments,
                    "success",
                    &output,
                )?;
                self.inner.store.append_log(
                    conversation_id,
                    &format!("tool call finished: {}", tool_name),
                )?;
                Ok(output)
            }
            Err(err) => {
                let failure = format!("{err:#}");
                self.inner.evidence.write_artifact(
                    conversation_id,
                    tool_name,
                    &arguments,
                    "failure",
                    &failure,
                )?;
                self.inner.store.append_log(
                    conversation_id,
                    &format!("tool call failed: {} error={}", tool_name, failure),
                )?;
                Err(anyhow!(failure))
            }
        }
    }

    fn emit(
        &self,
        ui_tx: &UnboundedSender<UiEvent>,
        kind: &str,
        message: &str,
        tool_name: Option<String>,
        tool_call_count: Option<usize>,
    ) {
        let _ = ui_tx.send(UiEvent::Progress(ProgressEvent {
            kind: kind.to_string(),
            message: message.to_string(),
            tool_name,
            tool_call_count,
        }));
    }
}

fn build_messages(
    prompt: Option<&str>,
    history: &[crate::types::Message],
    mcp_error: Option<&str>,
) -> Vec<Value> {
    let mut system_prompt = String::from(
        "You are a CSIRT investigation assistant operating inside an evidence-preserving MCP client. Use tools when needed, stay grounded in tool output, and be explicit about uncertainty or failures.",
    );
    if let Some(prompt) = prompt {
        system_prompt.push_str("\n\nOperator prompt:\n");
        system_prompt.push_str(prompt);
    }
    if let Some(mcp_error) = mcp_error {
        system_prompt.push_str("\n\nCurrent MCP warning:\n");
        system_prompt.push_str(mcp_error);
        system_prompt.push_str("\nProceed carefully and explain when tool access is degraded.");
    }

    let mut messages = vec![json!({"role": "system", "content": system_prompt})];
    messages.extend(history.iter().map(|message| {
        json!({
            "role": message.role,
            "content": message.content,
        })
    }));
    messages
}

fn estimate_context_chars(messages: &[Value]) -> usize {
    messages.iter().map(json_value_len).sum()
}

fn json_value_len(value: &Value) -> usize {
    match value {
        Value::Null => 0,
        Value::Bool(boolean) => usize::from(*boolean),
        Value::Number(number) => number.to_string().len(),
        Value::String(text) => text.chars().count(),
        Value::Array(items) => items.iter().map(json_value_len).sum(),
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| key.chars().count() + json_value_len(value))
            .sum(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::estimate_context_chars;

    #[test]
    fn estimates_context_chars_from_nested_messages() {
        let messages = vec![
            json!({"role": "system", "content": "hello"}),
            json!({"role": "assistant", "tool_calls": [{"id": "1", "function": {"name": "demo", "arguments": "{\"x\":1}"}}]}),
        ];

        let chars = estimate_context_chars(&messages);

        assert!(
            chars
                >= "systemhelloroleassistanttool_callsid1functionnamedemoarguments{\"x\":1}"
                    .chars()
                    .count()
        );
    }
}
