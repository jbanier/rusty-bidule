use anyhow::{Result, anyhow};
use async_openai::{
    Client,
    config::AzureConfig,
    error::{ApiError, OpenAIError},
};
use serde_json::{Value, json};
use tracing::{debug, error};

use crate::config::AzureOpenAiConfig;

#[derive(Debug, Clone)]
pub struct AzureTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone)]
pub struct AzureToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub raw_arguments: String,
}

#[derive(Debug, Clone)]
pub struct AzureCompletion {
    pub assistant_text: Option<String>,
    pub tool_calls: Vec<AzureToolCall>,
}

#[derive(Debug, Clone)]
pub struct AzureClient {
    client: Client<AzureConfig>,
    endpoint: String,
    deployment: String,
    api_version: String,
    temperature: f32,
    top_p: f32,
    max_output_tokens: u32,
}

impl AzureClient {
    pub fn new(config: &AzureOpenAiConfig) -> Self {
        let endpoint = config.endpoint.trim_end_matches('/').to_string();
        let client = Client::with_config(
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

    pub async fn chat_completion(
        &self,
        messages: &[Value],
        tools: &[AzureTool],
    ) -> Result<AzureCompletion> {
        let body = build_chat_request_body(
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

        parse_chat_completion_payload(&payload)
    }

    fn log_and_wrap_error(&self, err: OpenAIError) -> anyhow::Error {
        match err {
            OpenAIError::ApiError(api_error) => {
                log_api_error(
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
                    "failed to deserialize Azure response: {}",
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

fn build_chat_request_body(
    messages: &[Value],
    tools: &[AzureTool],
    temperature: f32,
    top_p: f32,
    max_output_tokens: u32,
) -> Value {
    let mut body = json!({
        "messages": messages,
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

fn parse_chat_completion_payload(payload: &Value) -> Result<AzureCompletion> {
    let message = payload
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .ok_or_else(|| anyhow!("Azure response did not include a choice message"))?;

    let assistant_text = message.get("content").and_then(content_to_string);
    let tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| {
            calls
                .iter()
                .map(|call| {
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
                        .unwrap_or("{}")
                        .to_string();
                    let arguments = serde_json::from_str(&raw_arguments)
                        .unwrap_or_else(|_| Value::String(raw_arguments.clone()));
                    Ok(AzureToolCall {
                        id,
                        name,
                        arguments,
                        raw_arguments,
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();

    Ok(AzureCompletion {
        assistant_text,
        tool_calls,
    })
}

fn log_api_error(endpoint: &str, deployment: &str, api_version: &str, api_error: &ApiError) {
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
        })
        .collect();
    if collected.is_empty() {
        None
    } else {
        Some(collected.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        AzureTool, build_chat_request_body, parse_chat_completion_payload, truncate_for_log,
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
    fn builds_chat_request_with_tools() {
        let body = build_chat_request_body(
            &[json!({"role": "user", "content": "hello"})],
            &[AzureTool {
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
    fn parses_chat_completion_payload() {
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

        let completion = parse_chat_completion_payload(&payload).unwrap();

        assert_eq!(completion.assistant_text.as_deref(), Some("hello"));
        assert_eq!(completion.tool_calls.len(), 1);
        assert_eq!(completion.tool_calls[0].name, "demo");
        assert_eq!(completion.tool_calls[0].arguments["query"], "value");
    }
}
