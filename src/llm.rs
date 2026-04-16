use anyhow::{Result, anyhow, bail};
use async_openai::{
    Client as OpenAiClient,
    config::AzureConfig,
    error::{ApiError, OpenAIError},
};
use reqwest::{
    Client as HttpClient,
    header::{CONTENT_TYPE, HeaderMap, HeaderValue},
};
use serde_json::{Value, json};
use tracing::{debug, error, warn};

use crate::config::{AppConfig, AzureAnthropicConfig, AzureOpenAiConfig, LlmProvider};

#[derive(Debug, Clone)]
pub struct LlmTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LlmAssistantBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmToolResult {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LlmStopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    PauseTurn,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum LlmMessage {
    System(String),
    UserText(String),
    Assistant { blocks: Vec<LlmAssistantBlock> },
    UserToolResults { results: Vec<LlmToolResult> },
}

#[derive(Debug, Clone)]
pub struct LlmCompletion {
    pub assistant_blocks: Vec<LlmAssistantBlock>,
    pub stop_reason: LlmStopReason,
}

#[derive(Debug, Clone)]
pub struct LlmClient {
    selected_provider: Option<LlmProvider>,
    azure_openai: Option<AzureOpenAiClient>,
    azure_anthropic: Option<AzureAnthropicClient>,
}

#[derive(Debug, Clone)]
struct AzureOpenAiClient {
    client: OpenAiClient<AzureConfig>,
    endpoint: String,
    deployment: String,
    api_version: String,
    temperature: f32,
    top_p: f32,
    max_output_tokens: u32,
}

#[derive(Debug, Clone)]
struct AzureAnthropicClient {
    client: HttpClient,
    endpoint: String,
    deployment: String,
    anthropic_version: String,
    temperature: f32,
    top_p: Option<f32>,
    max_output_tokens: u32,
}

impl LlmClient {
    pub fn new(config: &AppConfig) -> Result<Self> {
        Ok(Self {
            selected_provider: config.effective_llm_provider(),
            azure_openai: config.azure_openai.as_ref().map(AzureOpenAiClient::new),
            azure_anthropic: config
                .azure_anthropic
                .as_ref()
                .map(AzureAnthropicClient::new)
                .transpose()?,
        })
    }

    pub fn provider_label(&self) -> &'static str {
        match self.selected_provider {
            Some(LlmProvider::AzureOpenAi) => "Azure OpenAI",
            Some(LlmProvider::AzureAnthropic) => "Azure Anthropic",
            None => "LLM",
        }
    }

    pub async fn chat_completion(
        &self,
        messages: &[LlmMessage],
        tools: &[LlmTool],
    ) -> Result<LlmCompletion> {
        match self.selected_provider {
            Some(LlmProvider::AzureOpenAi) => {
                let Some(client) = &self.azure_openai else {
                    return Err(anyhow!(
                        "Azure OpenAI is selected but not configured. Add an azure_openai block to enable inference."
                    ));
                };
                client.chat_completion(messages, tools).await
            }
            Some(LlmProvider::AzureAnthropic) => {
                let Some(client) = &self.azure_anthropic else {
                    return Err(anyhow!(
                        "Azure Anthropic is selected but not configured. Add an azure_anthropic block to enable inference."
                    ));
                };
                client.chat_completion(messages, tools).await
            }
            None => Err(anyhow!(
                "No LLM provider is configured. Add an azure_openai or azure_anthropic block to enable inference."
            )),
        }
    }
}

impl AzureOpenAiClient {
    fn new(config: &AzureOpenAiConfig) -> Self {
        let endpoint = config.endpoint.trim_end_matches('/').to_string();
        let client = OpenAiClient::with_config(
            AzureConfig::new()
                .with_api_base(endpoint.clone())
                .with_api_version(config.api_version.clone())
                .with_deployment_id(config.deployment.clone())
                .with_api_key(config.api_key.clone()),
        );

        Self {
            client,
            endpoint,
            deployment: config.deployment.clone(),
            api_version: config.api_version.clone(),
            temperature: config.temperature,
            top_p: config.top_p,
            max_output_tokens: config.max_output_tokens,
        }
    }

    async fn chat_completion(
        &self,
        messages: &[LlmMessage],
        tools: &[LlmTool],
    ) -> Result<LlmCompletion> {
        let body = build_openai_chat_request_body(
            messages,
            tools,
            self.temperature,
            self.top_p,
            self.max_output_tokens,
        );

        debug!(
            endpoint = %self.endpoint,
            deployment = %self.deployment,
            api_version = %self.api_version,
            message_count = messages.len(),
            tool_count = tools.len(),
            "sending Azure OpenAI chat completion request"
        );

        let payload: Value = match self.client.chat().create_byot(&body).await {
            Ok(payload) => payload,
            Err(err) => return Err(self.log_and_wrap_error(err)),
        };

        parse_openai_chat_completion_payload(&payload)
    }

    fn log_and_wrap_error(&self, err: OpenAIError) -> anyhow::Error {
        match err {
            OpenAIError::ApiError(api_error) => {
                log_openai_api_error(
                    &self.endpoint,
                    &self.deployment,
                    &self.api_version,
                    &api_error,
                );
                anyhow!("Azure OpenAI request failed: {api_error}")
            }
            OpenAIError::JSONDeserialize(deserialize_error, body) => {
                error!(
                    endpoint = %self.endpoint,
                    deployment = %self.deployment,
                    api_version = %self.api_version,
                    deserialize_error = %deserialize_error,
                    response_body = %truncate_for_log(&body),
                    "Azure OpenAI response parse failed"
                );
                anyhow!(
                    "failed to deserialize Azure OpenAI response: {}",
                    truncate_for_log(&body)
                )
            }
            OpenAIError::Reqwest(reqwest_error) => {
                error!(
                    endpoint = %self.endpoint,
                    deployment = %self.deployment,
                    api_version = %self.api_version,
                    error = %reqwest_error,
                    "failed to reach Azure OpenAI endpoint"
                );
                anyhow!("failed to reach Azure OpenAI endpoint: {reqwest_error}")
            }
            other => {
                error!(
                    endpoint = %self.endpoint,
                    deployment = %self.deployment,
                    api_version = %self.api_version,
                    error = %other,
                    "Azure OpenAI request failed"
                );
                anyhow!("Azure OpenAI request failed: {other}")
            }
        }
    }
}

impl AzureAnthropicClient {
    fn new(config: &AzureAnthropicConfig) -> Result<Self> {
        let endpoint = config.endpoint.trim_end_matches('/').to_string();
        let anthropic_version = config.effective_anthropic_version();
        let top_p = config.effective_top_p();
        if let Some(ignored_api_version) = config.ignored_api_version() {
            warn!(
                endpoint = %endpoint,
                deployment = %config.deployment,
                ignored_api_version = %ignored_api_version,
                anthropic_version = %anthropic_version,
                "azure_anthropic.api_version is ignored for Foundry Claude requests; set azure_anthropic.anthropic_version instead"
            );
        }
        if config.top_p.is_some() && top_p.is_none() {
            warn!(
                endpoint = %endpoint,
                deployment = %config.deployment,
                configured_top_p = ?config.top_p,
                temperature = config.temperature,
                "azure_anthropic.top_p at or near 1.0 is ignored; sending temperature only"
            );
        } else if top_p.is_some() {
            warn!(
                endpoint = %endpoint,
                deployment = %config.deployment,
                configured_top_p = ?top_p,
                temperature = config.temperature,
                "azure_anthropic.top_p overrides temperature because Anthropic requests cannot send both"
            );
        }
        let mut headers = HeaderMap::new();
        headers.insert("api-key", HeaderValue::from_str(&config.api_key)?);
        headers.insert("x-api-key", HeaderValue::from_str(&config.api_key)?);
        headers.insert(
            "anthropic-version",
            HeaderValue::from_str(&anthropic_version)?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let client = HttpClient::builder().default_headers(headers).build()?;

        Ok(Self {
            client,
            endpoint,
            deployment: config.deployment.clone(),
            anthropic_version,
            temperature: config.temperature,
            top_p,
            max_output_tokens: config.max_output_tokens,
        })
    }

    async fn chat_completion(
        &self,
        messages: &[LlmMessage],
        tools: &[LlmTool],
    ) -> Result<LlmCompletion> {
        let body = build_anthropic_chat_request_body(
            messages,
            tools,
            &self.deployment,
            self.temperature,
            self.top_p,
            self.max_output_tokens,
        )?;
        let url = format!("{}/v1/messages", self.endpoint);

        debug!(
            endpoint = %self.endpoint,
            deployment = %self.deployment,
            anthropic_version = %self.anthropic_version,
            message_count = messages.len(),
            tool_count = tools.len(),
            "sending Azure Anthropic chat completion request"
        );

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|err| {
                error!(
                    endpoint = %self.endpoint,
                    deployment = %self.deployment,
                    anthropic_version = %self.anthropic_version,
                    error = %err,
                    "failed to reach Azure Anthropic endpoint"
                );
                anyhow!("failed to reach Azure Anthropic endpoint: {err}")
            })?;

        let status = response.status();
        let payload = response.text().await.map_err(|err| {
            error!(
                endpoint = %self.endpoint,
                deployment = %self.deployment,
                anthropic_version = %self.anthropic_version,
                error = %err,
                "failed to read Azure Anthropic response body"
            );
            anyhow!("failed to read Azure Anthropic response body: {err}")
        })?;

        if !status.is_success() {
            error!(
                endpoint = %self.endpoint,
                deployment = %self.deployment,
                anthropic_version = %self.anthropic_version,
                status = %status,
                response_body = %truncate_for_log(&payload),
                "Azure Anthropic request failed"
            );
            return Err(anyhow!(
                "Azure Anthropic request failed with status {}: {}",
                status,
                truncate_for_log(&payload)
            ));
        }

        let payload: Value = serde_json::from_str(&payload).map_err(|err| {
            error!(
                endpoint = %self.endpoint,
                deployment = %self.deployment,
                anthropic_version = %self.anthropic_version,
                error = %err,
                "failed to deserialize Azure Anthropic response"
            );
            anyhow!("failed to deserialize Azure Anthropic response: {err}")
        })?;

        parse_anthropic_chat_completion_payload(&payload)
    }
}

fn build_openai_chat_request_body(
    messages: &[LlmMessage],
    tools: &[LlmTool],
    temperature: f32,
    top_p: f32,
    max_output_tokens: u32,
) -> Value {
    let mut body = json!({
        "messages": openai_messages(messages),
        "temperature": temperature,
        "top_p": top_p,
        "max_tokens": max_output_tokens,
        "stream": false,
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters,
                        }
                    })
                })
                .collect(),
        );
        body["tool_choice"] = Value::String("auto".to_string());
    }
    body
}

fn openai_messages(messages: &[LlmMessage]) -> Vec<Value> {
    let mut out = Vec::new();
    for message in messages {
        match message {
            LlmMessage::System(text) => out.push(json!({
                "role": "system",
                "content": text,
            })),
            LlmMessage::UserText(text) => out.push(json!({
                "role": "user",
                "content": text,
            })),
            LlmMessage::Assistant { blocks } => {
                let text = assistant_blocks_to_text(blocks);
                let tool_calls = assistant_blocks_to_openai_tool_calls(blocks);
                let mut message = json!({
                    "role": "assistant",
                    "content": if text.is_empty() { Value::Null } else { Value::String(text) },
                });
                if !tool_calls.is_empty() {
                    message["tool_calls"] = Value::Array(tool_calls);
                }
                out.push(message);
            }
            LlmMessage::UserToolResults { results } => {
                for result in results {
                    out.push(json!({
                        "role": "tool",
                        "tool_call_id": result.tool_use_id,
                        "content": result.content,
                    }));
                }
            }
        }
    }
    out
}

fn assistant_blocks_to_text(blocks: &[LlmAssistantBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            LlmAssistantBlock::Text { text } if !text.trim().is_empty() => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn assistant_blocks_to_openai_tool_calls(blocks: &[LlmAssistantBlock]) -> Vec<Value> {
    blocks
        .iter()
        .filter_map(|block| match block {
            LlmAssistantBlock::ToolUse { id, name, input } => Some(json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string()),
                }
            })),
            _ => None,
        })
        .collect()
}

fn parse_openai_chat_completion_payload(payload: &Value) -> Result<LlmCompletion> {
    let message = payload
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .ok_or_else(|| anyhow!("Azure OpenAI response did not include a choice message"))?;

    let mut assistant_blocks = Vec::new();
    if let Some(text) = message.get("content").and_then(content_to_string)
        && !text.trim().is_empty()
    {
        assistant_blocks.push(LlmAssistantBlock::Text { text });
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in tool_calls {
            let id = call
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("tool call missing id"))?
                .to_string();
            let function = call
                .get("function")
                .ok_or_else(|| anyhow!("tool call missing function"))?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("tool call missing function.name"))?
                .to_string();
            let raw_arguments = function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            let input = serde_json::from_str(raw_arguments)
                .unwrap_or_else(|_| Value::String(raw_arguments.to_string()));
            assistant_blocks.push(LlmAssistantBlock::ToolUse { id, name, input });
        }
    }

    Ok(LlmCompletion {
        stop_reason: if assistant_blocks
            .iter()
            .any(|block| matches!(block, LlmAssistantBlock::ToolUse { .. }))
        {
            LlmStopReason::ToolUse
        } else {
            LlmStopReason::EndTurn
        },
        assistant_blocks,
    })
}

fn build_anthropic_chat_request_body(
    messages: &[LlmMessage],
    tools: &[LlmTool],
    deployment: &str,
    temperature: f32,
    top_p: Option<f32>,
    max_output_tokens: u32,
) -> Result<Value> {
    let (system, anthropic_messages) = anthropic_messages(messages)?;
    let mut body = json!({
        "model": deployment,
        "messages": anthropic_messages,
        "max_tokens": max_output_tokens,
    });
    if let Some(top_p) = top_p {
        body["top_p"] = json!(top_p);
    } else {
        body["temperature"] = json!(temperature);
    }
    if let Some(system) = system {
        body["system"] = Value::String(system);
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(|tool| {
                    json!({
                        "name": tool.name,
                        "description": tool.description,
                        "input_schema": tool.parameters,
                    })
                })
                .collect(),
        );
    }
    Ok(body)
}

fn anthropic_messages(messages: &[LlmMessage]) -> Result<(Option<String>, Vec<Value>)> {
    let mut system_parts = Vec::new();
    let mut out = Vec::new();

    for message in messages {
        match message {
            LlmMessage::System(text) => {
                if !text.trim().is_empty() {
                    system_parts.push(text.clone());
                }
            }
            LlmMessage::UserText(text) => out.push(json!({
                "role": "user",
                "content": [{"type": "text", "text": text}],
            })),
            LlmMessage::Assistant { blocks } => {
                out.push(json!({
                    "role": "assistant",
                    "content": anthropic_assistant_blocks(blocks),
                }));
            }
            LlmMessage::UserToolResults { results } => {
                if results.is_empty() {
                    continue;
                }
                out.push(json!({
                    "role": "user",
                    "content": results.iter().map(|result| {
                        let mut block = json!({
                            "type": "tool_result",
                            "tool_use_id": result.tool_use_id,
                            "content": result.content,
                        });
                        if result.is_error {
                            block["is_error"] = Value::Bool(true);
                        }
                        block
                    }).collect::<Vec<_>>(),
                }));
            }
        }
    }

    for window in out.windows(2) {
        if window[0]["role"] == "assistant"
            && has_anthropic_tool_use(&window[0])
            && window[1]["role"] != "user"
        {
            bail!(
                "Anthropic tool_result blocks must immediately follow the assistant tool_use turn"
            );
        }
    }

    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };
    Ok((system, out))
}

fn anthropic_assistant_blocks(blocks: &[LlmAssistantBlock]) -> Vec<Value> {
    blocks
        .iter()
        .map(|block| match block {
            LlmAssistantBlock::Text { text } => json!({
                "type": "text",
                "text": text,
            }),
            LlmAssistantBlock::ToolUse { id, name, input } => json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input,
            }),
        })
        .collect()
}

fn has_anthropic_tool_use(message: &Value) -> bool {
    message["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
        })
        .unwrap_or(false)
}

fn parse_anthropic_chat_completion_payload(payload: &Value) -> Result<LlmCompletion> {
    let content = payload
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Azure Anthropic response did not include content"))?;

    let assistant_blocks = content
        .iter()
        .filter_map(|block| match block.get("type").and_then(Value::as_str) {
            Some("text") => Some(
                block
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|text| LlmAssistantBlock::Text {
                        text: text.to_string(),
                    })
                    .ok_or_else(|| anyhow!("text block missing text")),
            ),
            Some("tool_use") => Some(parse_anthropic_tool_use(block)),
            _ => None,
        })
        .collect::<Result<Vec<_>>>()?;

    let stop_reason = parse_anthropic_stop_reason(payload.get("stop_reason"));

    Ok(LlmCompletion {
        assistant_blocks,
        stop_reason,
    })
}

fn parse_anthropic_tool_use(block: &Value) -> Result<LlmAssistantBlock> {
    let id = block
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("tool_use block missing id"))?
        .to_string();
    let name = block
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("tool_use block missing name"))?
        .to_string();
    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
    Ok(LlmAssistantBlock::ToolUse { id, name, input })
}

fn parse_anthropic_stop_reason(stop_reason: Option<&Value>) -> LlmStopReason {
    match stop_reason.and_then(Value::as_str) {
        Some("end_turn") | Some("stop_sequence") | None => LlmStopReason::EndTurn,
        Some("tool_use") => LlmStopReason::ToolUse,
        Some("max_tokens") => LlmStopReason::MaxTokens,
        Some("pause_turn") => LlmStopReason::PauseTurn,
        Some(other) => LlmStopReason::Unknown(other.to_string()),
    }
}

fn log_openai_api_error(endpoint: &str, deployment: &str, api_version: &str, api_error: &ApiError) {
    error!(
        endpoint,
        deployment,
        api_version,
        error_type = api_error.r#type.as_deref().unwrap_or("unknown"),
        error_param = api_error.param.as_deref().unwrap_or(""),
        error_code = api_error.code.as_deref().unwrap_or(""),
        error_message = %api_error.message,
        "Azure OpenAI request failed"
    );
}

fn truncate_for_log(text: &str) -> String {
    const LIMIT: usize = 1_000;
    let mut truncated = text.chars().take(LIMIT).collect::<String>();
    if text.chars().count() > LIMIT {
        truncated.push_str("...(truncated)");
    }
    truncated
}

fn content_to_string(content: &Value) -> Option<String> {
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    let items = content.as_array()?;
    let collected: Vec<String> = items
        .iter()
        .filter_map(|item| {
            item.get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| item.as_str().map(str::to_string))
                .or_else(|| {
                    item.get("content")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
        })
        .collect();
    if collected.is_empty() {
        None
    } else {
        Some(collected.join("\n"))
    }
}

pub fn llm_message_text_len(message: &LlmMessage) -> usize {
    match message {
        LlmMessage::System(text) | LlmMessage::UserText(text) => text.chars().count(),
        LlmMessage::Assistant { blocks } => blocks
            .iter()
            .map(|block| match block {
                LlmAssistantBlock::Text { text } => text.chars().count(),
                LlmAssistantBlock::ToolUse { id, name, input } => {
                    id.chars().count() + name.chars().count() + json_value_len(input)
                }
            })
            .sum(),
        LlmMessage::UserToolResults { results } => results
            .iter()
            .map(|result| {
                result.tool_use_id.chars().count()
                    + result.content.chars().count()
                    + usize::from(result.is_error)
            })
            .sum(),
    }
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

    use super::{
        LlmAssistantBlock, LlmMessage, LlmStopReason, LlmTool, LlmToolResult,
        build_anthropic_chat_request_body, build_openai_chat_request_body,
        parse_anthropic_chat_completion_payload, parse_openai_chat_completion_payload,
        truncate_for_log,
    };

    #[test]
    fn truncates_long_bodies_for_logs() {
        let body = "x".repeat(1_100);
        let logged = truncate_for_log(&body);

        assert_eq!(
            logged.chars().take(1_000).collect::<String>(),
            "x".repeat(1_000)
        );
        assert!(logged.ends_with("...(truncated)"));
    }

    #[test]
    fn builds_openai_chat_request_with_tools() {
        let body = build_openai_chat_request_body(
            &[LlmMessage::UserText("hello".to_string())],
            &[LlmTool {
                name: "demo".to_string(),
                description: "Demo tool".to_string(),
                parameters: json!({"type": "object"}),
            }],
            0.2,
            1.0,
            1200,
        );

        assert_eq!(body["stream"], json!(false));
        assert_eq!(body["tool_choice"], json!("auto"));
        assert_eq!(body["tools"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn parses_openai_chat_completion_payload() {
        let payload = json!({
            "choices": [{
                "message": {
                    "content": [{"text": "hello"}],
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {
                            "name": "demo",
                            "arguments": "{\"query\":\"value\"}"
                        }
                    }]
                }
            }]
        });

        let completion = parse_openai_chat_completion_payload(&payload).unwrap();

        assert_eq!(completion.stop_reason, LlmStopReason::ToolUse);
        assert_eq!(completion.assistant_blocks.len(), 2);
    }

    #[test]
    fn builds_anthropic_chat_request_with_tools_and_error_results() {
        let body = build_anthropic_chat_request_body(
            &[
                LlmMessage::System("sys".to_string()),
                LlmMessage::Assistant {
                    blocks: vec![
                        LlmAssistantBlock::Text {
                            text: "thinking".to_string(),
                        },
                        LlmAssistantBlock::ToolUse {
                            id: "toolu_1".to_string(),
                            name: "demo".to_string(),
                            input: json!({"query":"hello"}),
                        },
                    ],
                },
                LlmMessage::UserToolResults {
                    results: vec![LlmToolResult {
                        tool_use_id: "toolu_1".to_string(),
                        content: "boom".to_string(),
                        is_error: true,
                    }],
                },
            ],
            &[LlmTool {
                name: "demo".to_string(),
                description: "Demo tool".to_string(),
                parameters: json!({"type": "object", "properties": {}}),
            }],
            "claude-opus-4-6",
            0.2,
            None,
            1200,
        )
        .unwrap();

        assert_eq!(body["model"], "claude-opus-4-6");
        assert_eq!(body["system"], "sys");
        assert!(
            body["temperature"]
                .as_f64()
                .is_some_and(|value| (value - 0.2).abs() < 1e-6)
        );
        assert!(body.get("top_p").is_none());
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
        assert_eq!(body["messages"][0]["content"][1]["type"], "tool_use");
        assert_eq!(body["messages"][1]["content"][0]["type"], "tool_result");
        assert_eq!(body["messages"][1]["content"][0]["is_error"], true);
    }

    #[test]
    fn builds_anthropic_chat_request_with_top_p_without_temperature() {
        let body = build_anthropic_chat_request_body(
            &[LlmMessage::UserText("hello".to_string())],
            &[],
            "claude-opus-4-6",
            0.2,
            Some(0.9),
            1200,
        )
        .unwrap();

        assert!(
            body["top_p"]
                .as_f64()
                .is_some_and(|value| (value - 0.9).abs() < 1e-6)
        );
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn parses_anthropic_chat_completion_payload_with_mixed_blocks() {
        let payload = json!({
            "stop_reason": "tool_use",
            "content": [
                {"type": "text", "text": "hello"},
                {"type": "tool_use", "id": "toolu_1", "name": "demo", "input": {"query": "value"}}
            ]
        });

        let completion = parse_anthropic_chat_completion_payload(&payload).unwrap();

        assert_eq!(completion.stop_reason, LlmStopReason::ToolUse);
        assert_eq!(
            completion.assistant_blocks[0],
            LlmAssistantBlock::Text {
                text: "hello".to_string()
            }
        );
        assert!(matches!(
            completion.assistant_blocks[1],
            LlmAssistantBlock::ToolUse { .. }
        ));
    }

    #[test]
    fn parses_anthropic_stop_reasons() {
        let payload = json!({
            "stop_reason": "pause_turn",
            "content": [{"type":"text","text":"hold"}]
        });

        let completion = parse_anthropic_chat_completion_payload(&payload).unwrap();
        assert_eq!(completion.stop_reason, LlmStopReason::PauseTurn);
    }
}
