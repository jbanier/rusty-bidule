use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use serde_json::{Value, json};

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
    client: Client,
    endpoint: String,
    deployment: String,
    api_version: String,
    api_key: String,
    temperature: f32,
    top_p: f32,
    max_output_tokens: u32,
}

impl AzureClient {
    pub fn new(config: &AzureOpenAiConfig) -> Self {
        Self {
            client: Client::new(),
            endpoint: config.endpoint.trim_end_matches('/').to_string(),
            deployment: config.deployment.clone(),
            api_version: config.api_version.clone(),
            api_key: config.api_key.clone(),
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
        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version={}",
            self.endpoint, self.deployment, self.api_version
        );

        let mut body = json!({
            "messages": messages,
            "temperature": self.temperature,
            "top_p": self.top_p,
            "max_tokens": self.max_output_tokens,
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

        let response = self
            .client
            .post(url)
            .header("api-key", &self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("failed to reach Azure OpenAI endpoint")?;

        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            return Err(anyhow!("Azure OpenAI returned HTTP {}: {}", status, text));
        }

        let payload: Value = serde_json::from_str(&text)
            .with_context(|| format!("failed to parse Azure response body: {text}"))?;
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
