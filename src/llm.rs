use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use adk_rust::futures::StreamExt as _;
use adk_rust::{
    Content as AdkContent, FinishReason as AdkFinishReason, FunctionResponseData,
    GenerateContentConfig, Llm as AdkLlm, LlmRequest as AdkLlmRequest,
    LlmResponse as AdkLlmResponse, Part as AdkPart, UsageMetadata as AdkUsageMetadata,
};
use anyhow::{Context, Result, anyhow, bail};
use async_openai::{
    Client as AsyncOpenAiClient,
    config::AzureConfig,
    error::{ApiError, OpenAIError},
};
use reqwest::{
    Client as HttpClient,
    header::{CONTENT_TYPE, HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{debug, error, warn};

use crate::config::{
    AdkConfig, AdkProvider, AppConfig, AzureAnthropicConfig, AzureOpenAiConfig, LlmProvider,
    OpenAiCompatibleConfig, OpenAiConfig,
};
use crate::types::LlmUsage;

#[derive(Debug, Clone)]
pub struct LlmTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub usage: Option<LlmUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub tool_calling: bool,
    pub streaming: bool,
    pub usage_metadata: bool,
    pub structured_output: bool,
    pub reasoning_controls: bool,
}

impl ModelCapabilities {
    const fn chat_with_tools_and_usage() -> Self {
        Self {
            tool_calling: true,
            streaming: false,
            usage_metadata: true,
            structured_output: false,
            reasoning_controls: false,
        }
    }
}

type ModelBackendFuture<'a> = Pin<Box<dyn Future<Output = Result<LlmCompletion>> + Send + 'a>>;

trait ModelBackend: Send + Sync {
    fn label(&self) -> &'static str;

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities::chat_with_tools_and_usage()
    }

    fn chat_completion<'a>(
        &'a self,
        messages: &'a [LlmMessage],
        tools: &'a [LlmTool],
    ) -> ModelBackendFuture<'a>;
}

#[derive(Debug, Clone)]
pub struct LlmClient {
    selected_provider: Option<LlmProvider>,
    azure_openai: Option<AzureOpenAiClient>,
    openai: Option<OpenAiClient>,
    azure_anthropic: Option<AzureAnthropicClient>,
    openai_compatible: Option<OpenAiCompatibleClient>,
    adk: Option<AdkClient>,
}

#[derive(Debug, Clone)]
struct AzureOpenAiClient {
    client: AsyncOpenAiClient<AzureConfig>,
    endpoint: String,
    deployment: String,
    api_version: String,
    temperature: f32,
    top_p: f32,
    max_output_tokens: u32,
}

#[derive(Debug, Clone)]
struct OpenAiClient {
    client: HttpClient,
    endpoint: String,
    chat_url: String,
    model: String,
    api_key: String,
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

#[derive(Debug, Clone)]
struct OpenAiCompatibleClient {
    client: HttpClient,
    base_url: String,
    chat_url: String,
    model: String,
    api_key: Option<String>,
    temperature: f32,
    top_p: f32,
    max_output_tokens: u32,
}

#[derive(Clone)]
struct AdkClient {
    provider: AdkProvider,
    model: String,
    temperature: f32,
    top_p: f32,
    max_output_tokens: u32,
    backend: Arc<dyn AdkLlm>,
}

impl std::fmt::Debug for AdkClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AdkClient")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .field("top_p", &self.top_p)
            .field("max_output_tokens", &self.max_output_tokens)
            .finish_non_exhaustive()
    }
}

impl LlmClient {
    pub fn new(config: &AppConfig) -> Result<Self> {
        Ok(Self {
            selected_provider: config.effective_llm_provider(),
            azure_openai: config.azure_openai.as_ref().map(AzureOpenAiClient::new),
            openai: config.openai.as_ref().map(OpenAiClient::new).transpose()?,
            azure_anthropic: config
                .azure_anthropic
                .as_ref()
                .map(AzureAnthropicClient::new)
                .transpose()?,
            openai_compatible: config
                .openai_compatible
                .as_ref()
                .map(OpenAiCompatibleClient::new)
                .transpose()?,
            adk: config.adk.as_ref().map(AdkClient::new).transpose()?,
        })
    }

    pub fn provider_label(&self) -> &'static str {
        self.active_backend()
            .map(|backend| backend.label())
            .unwrap_or("LLM")
    }

    pub fn model_capabilities(&self) -> Option<ModelCapabilities> {
        self.active_backend()
            .map(|backend| backend.capabilities())
            .ok()
    }

    pub async fn chat_completion(
        &self,
        messages: &[LlmMessage],
        tools: &[LlmTool],
    ) -> Result<LlmCompletion> {
        self.active_backend()?
            .chat_completion(messages, tools)
            .await
    }

    fn active_backend(&self) -> Result<&dyn ModelBackend> {
        match self.selected_provider {
            Some(LlmProvider::AzureOpenAi) => {
                let Some(client) = &self.azure_openai else {
                    return Err(anyhow!(
                        "Azure OpenAI is selected but not configured. Add an azure_openai block to enable inference."
                    ));
                };
                Ok(client)
            }
            Some(LlmProvider::OpenAi) => {
                let Some(client) = &self.openai else {
                    return Err(anyhow!(
                        "OpenAI is selected but not configured. Add an openai block to enable inference."
                    ));
                };
                Ok(client)
            }
            Some(LlmProvider::AzureAnthropic) => {
                let Some(client) = &self.azure_anthropic else {
                    return Err(anyhow!(
                        "Azure Anthropic is selected but not configured. Add an azure_anthropic block to enable inference."
                    ));
                };
                Ok(client)
            }
            Some(LlmProvider::OpenAiCompatible) => {
                let Some(client) = &self.openai_compatible else {
                    return Err(anyhow!(
                        "OpenAI-compatible is selected but not configured. Add an openai_compatible block to enable inference."
                    ));
                };
                Ok(client)
            }
            Some(LlmProvider::Adk) => {
                let Some(client) = &self.adk else {
                    return Err(anyhow!(
                        "ADK is selected but not configured. Add an adk block to enable inference."
                    ));
                };
                Ok(client)
            }
            None => Err(anyhow!(
                "No LLM provider is configured. Add an azure_openai, openai, azure_anthropic, openai_compatible, or adk block to enable inference."
            )),
        }
    }
}

impl AzureOpenAiClient {
    fn new(config: &AzureOpenAiConfig) -> Self {
        let endpoint = config.endpoint.trim_end_matches('/').to_string();
        let client = AsyncOpenAiClient::with_config(
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

impl ModelBackend for AzureOpenAiClient {
    fn label(&self) -> &'static str {
        "Azure OpenAI"
    }

    fn chat_completion<'a>(
        &'a self,
        messages: &'a [LlmMessage],
        tools: &'a [LlmTool],
    ) -> ModelBackendFuture<'a> {
        Box::pin(async move { AzureOpenAiClient::chat_completion(self, messages, tools).await })
    }
}

impl OpenAiClient {
    fn new(config: &OpenAiConfig) -> Result<Self> {
        let endpoint = config.endpoint.trim_end_matches('/').to_string();
        let chat_url = openai_compatible_chat_url(&endpoint);
        let client = HttpClient::builder().build()?;

        Ok(Self {
            client,
            endpoint,
            chat_url,
            model: config.model.clone(),
            api_key: config.api_key.clone(),
            temperature: config.temperature,
            top_p: config.top_p,
            max_output_tokens: config.max_output_tokens,
        })
    }

    async fn chat_completion(
        &self,
        messages: &[LlmMessage],
        tools: &[LlmTool],
    ) -> Result<LlmCompletion> {
        let body = build_openai_compatible_chat_request_body(
            messages,
            tools,
            &self.model,
            self.temperature,
            self.top_p,
            self.max_output_tokens,
        );

        debug!(
            endpoint = %self.endpoint,
            chat_url = %self.chat_url,
            model = %self.model,
            message_count = messages.len(),
            tool_count = tools.len(),
            "sending OpenAI chat completion request"
        );

        let response = self
            .client
            .post(&self.chat_url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|err| {
                error!(
                    endpoint = %self.endpoint,
                    chat_url = %self.chat_url,
                    model = %self.model,
                    error = %err,
                    "failed to reach OpenAI endpoint"
                );
                anyhow!("failed to reach OpenAI endpoint: {err}")
            })?;

        let status = response.status();
        let payload = response.text().await.map_err(|err| {
            error!(
                endpoint = %self.endpoint,
                chat_url = %self.chat_url,
                model = %self.model,
                error = %err,
                "failed to read OpenAI response body"
            );
            anyhow!("failed to read OpenAI response body: {err}")
        })?;

        if !status.is_success() {
            error!(
                endpoint = %self.endpoint,
                chat_url = %self.chat_url,
                model = %self.model,
                status = %status,
                response_body = %truncate_for_log(&payload),
                "OpenAI request failed"
            );
            return Err(anyhow!(
                "OpenAI request failed with status {}: {}",
                status,
                truncate_for_log(&payload)
            ));
        }

        let payload: Value = serde_json::from_str(&payload).map_err(|err| {
            error!(
                endpoint = %self.endpoint,
                chat_url = %self.chat_url,
                model = %self.model,
                error = %err,
                "failed to deserialize OpenAI response"
            );
            anyhow!("failed to deserialize OpenAI response: {err}")
        })?;

        parse_openai_chat_completion_payload(&payload)
    }
}

impl ModelBackend for OpenAiClient {
    fn label(&self) -> &'static str {
        "OpenAI"
    }

    fn chat_completion<'a>(
        &'a self,
        messages: &'a [LlmMessage],
        tools: &'a [LlmTool],
    ) -> ModelBackendFuture<'a> {
        Box::pin(async move { OpenAiClient::chat_completion(self, messages, tools).await })
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

impl ModelBackend for AzureAnthropicClient {
    fn label(&self) -> &'static str {
        "Azure Anthropic"
    }

    fn chat_completion<'a>(
        &'a self,
        messages: &'a [LlmMessage],
        tools: &'a [LlmTool],
    ) -> ModelBackendFuture<'a> {
        Box::pin(async move { AzureAnthropicClient::chat_completion(self, messages, tools).await })
    }
}

impl OpenAiCompatibleClient {
    fn new(config: &OpenAiCompatibleConfig) -> Result<Self> {
        let base_url = config.base_url.trim_end_matches('/').to_string();
        let chat_url = openai_compatible_chat_url(&base_url);
        let api_key = config
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|api_key| !api_key.is_empty())
            .map(str::to_string);
        let client = HttpClient::builder().build()?;

        Ok(Self {
            client,
            base_url,
            chat_url,
            model: config.model.clone(),
            api_key,
            temperature: config.temperature,
            top_p: config.top_p,
            max_output_tokens: config.max_output_tokens,
        })
    }

    async fn chat_completion(
        &self,
        messages: &[LlmMessage],
        tools: &[LlmTool],
    ) -> Result<LlmCompletion> {
        let body = build_openai_compatible_chat_request_body(
            messages,
            tools,
            &self.model,
            self.temperature,
            self.top_p,
            self.max_output_tokens,
        );

        debug!(
            base_url = %self.base_url,
            chat_url = %self.chat_url,
            model = %self.model,
            message_count = messages.len(),
            tool_count = tools.len(),
            "sending OpenAI-compatible chat completion request"
        );

        let mut request = self.client.post(&self.chat_url).json(&body);
        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }

        let response = request.send().await.map_err(|err| {
            error!(
                base_url = %self.base_url,
                chat_url = %self.chat_url,
                model = %self.model,
                error = %err,
                "failed to reach OpenAI-compatible endpoint"
            );
            anyhow!("failed to reach OpenAI-compatible endpoint: {err}")
        })?;

        let status = response.status();
        let payload = response.text().await.map_err(|err| {
            error!(
                base_url = %self.base_url,
                chat_url = %self.chat_url,
                model = %self.model,
                error = %err,
                "failed to read OpenAI-compatible response body"
            );
            anyhow!("failed to read OpenAI-compatible response body: {err}")
        })?;

        if !status.is_success() {
            error!(
                base_url = %self.base_url,
                chat_url = %self.chat_url,
                model = %self.model,
                status = %status,
                response_body = %truncate_for_log(&payload),
                "OpenAI-compatible request failed"
            );
            return Err(anyhow!(
                "OpenAI-compatible request failed with status {}: {}",
                status,
                truncate_for_log(&payload)
            ));
        }

        let payload: Value = serde_json::from_str(&payload).map_err(|err| {
            error!(
                base_url = %self.base_url,
                chat_url = %self.chat_url,
                model = %self.model,
                error = %err,
                "failed to deserialize OpenAI-compatible response"
            );
            anyhow!("failed to deserialize OpenAI-compatible response: {err}")
        })?;

        parse_openai_chat_completion_payload(&payload)
    }
}

impl ModelBackend for OpenAiCompatibleClient {
    fn label(&self) -> &'static str {
        "OpenAI-compatible"
    }

    fn chat_completion<'a>(
        &'a self,
        messages: &'a [LlmMessage],
        tools: &'a [LlmTool],
    ) -> ModelBackendFuture<'a> {
        Box::pin(
            async move { OpenAiCompatibleClient::chat_completion(self, messages, tools).await },
        )
    }
}

impl AdkClient {
    fn new(config: &AdkConfig) -> Result<Self> {
        let api_key = config.api_key.clone();
        let model = config.model.clone();
        let endpoint = normalized_optional_endpoint(config.endpoint.as_deref());
        let backend: Arc<dyn AdkLlm> = match config.provider {
            AdkProvider::Gemini => {
                Arc::new(adk_rust::model::GeminiModel::new(api_key, model.clone())?)
            }
            AdkProvider::OpenAi => {
                let mut adk_config =
                    adk_rust::model::openai::OpenAIConfig::new(api_key, model.clone());
                adk_config.base_url = endpoint.clone();
                Arc::new(adk_rust::model::openai::OpenAIClient::new(adk_config)?)
            }
            AdkProvider::OpenAiCompatible => {
                let endpoint = endpoint.ok_or_else(|| {
                    anyhow!("adk.endpoint is required when adk.provider is openai_compatible")
                })?;
                let adk_config = adk_rust::model::openai_compatible::OpenAICompatibleConfig::new(
                    api_key,
                    model.clone(),
                )
                .with_base_url(endpoint)
                .with_provider_name("openai-compatible");
                Arc::new(adk_rust::model::openai_compatible::OpenAICompatible::new(
                    adk_config,
                )?)
            }
            AdkProvider::Anthropic => {
                let mut adk_config =
                    adk_rust::model::anthropic::AnthropicConfig::new(api_key, model.clone())
                        .with_max_tokens(config.max_output_tokens);
                if let Some(endpoint) = endpoint {
                    adk_config = adk_config.with_base_url(endpoint);
                }
                Arc::new(adk_rust::model::anthropic::AnthropicClient::new(
                    adk_config,
                )?)
            }
            AdkProvider::AzureAi => {
                let endpoint = endpoint.ok_or_else(|| {
                    anyhow!("adk.endpoint is required when adk.provider is azure_ai")
                })?;
                let adk_config =
                    adk_rust::model::azure_ai::AzureAIConfig::new(endpoint, api_key, model.clone());
                Arc::new(adk_rust::model::azure_ai::AzureAIClient::new(adk_config)?)
            }
        };

        Ok(Self {
            provider: config.provider,
            model,
            temperature: config.temperature,
            top_p: config.top_p,
            max_output_tokens: config.max_output_tokens,
            backend,
        })
    }

    async fn chat_completion(
        &self,
        messages: &[LlmMessage],
        tools: &[LlmTool],
    ) -> Result<LlmCompletion> {
        let mut request =
            AdkLlmRequest::new(self.model.clone(), adk_contents(self.provider, messages))
                .with_config(GenerateContentConfig {
                    temperature: Some(self.temperature),
                    top_p: Some(self.top_p),
                    max_output_tokens: Some(self.max_output_tokens as i32),
                    ..Default::default()
                });
        request.tools = adk_tools(tools);

        debug!(
            provider = ?self.provider,
            model = %self.model,
            message_count = messages.len(),
            tool_count = tools.len(),
            "sending ADK model request"
        );

        let mut stream = self
            .backend
            .generate_content(request, false)
            .await
            .map_err(|err| {
                error!(
                    provider = ?self.provider,
                    model = %self.model,
                    error = %err,
                    "ADK model request failed"
                );
                anyhow!("ADK model request failed: {err}")
            })?;

        let mut responses = Vec::new();
        while let Some(item) = stream.next().await {
            responses.push(item.map_err(|err| {
                error!(
                    provider = ?self.provider,
                    model = %self.model,
                    error = %err,
                    "ADK model response failed"
                );
                anyhow!("ADK model response failed: {err}")
            })?);
        }

        adk_completion_from_responses(&responses)
    }

    const fn provider_label(&self) -> &'static str {
        match self.provider {
            AdkProvider::Gemini => "ADK Gemini",
            AdkProvider::OpenAi => "ADK OpenAI",
            AdkProvider::OpenAiCompatible => "ADK OpenAI-compatible",
            AdkProvider::Anthropic => "ADK Anthropic",
            AdkProvider::AzureAi => "ADK Azure AI",
        }
    }
}

impl ModelBackend for AdkClient {
    fn label(&self) -> &'static str {
        self.provider_label()
    }

    fn chat_completion<'a>(
        &'a self,
        messages: &'a [LlmMessage],
        tools: &'a [LlmTool],
    ) -> ModelBackendFuture<'a> {
        Box::pin(async move { AdkClient::chat_completion(self, messages, tools).await })
    }
}

fn normalized_optional_endpoint(endpoint: Option<&str>) -> Option<String> {
    endpoint
        .map(str::trim)
        .filter(|endpoint| !endpoint.is_empty())
        .map(|endpoint| endpoint.trim_end_matches('/').to_string())
}

fn openai_compatible_chat_url(base_url: &str) -> String {
    let base_url = base_url.trim_end_matches('/');
    if base_url.ends_with("/chat/completions") {
        base_url.to_string()
    } else {
        format!("{base_url}/chat/completions")
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

fn build_openai_compatible_chat_request_body(
    messages: &[LlmMessage],
    tools: &[LlmTool],
    model: &str,
    temperature: f32,
    top_p: f32,
    max_output_tokens: u32,
) -> Value {
    let mut body =
        build_openai_chat_request_body(messages, tools, temperature, top_p, max_output_tokens);
    body["model"] = Value::String(model.to_string());
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

fn adk_contents(provider: AdkProvider, messages: &[LlmMessage]) -> Vec<AdkContent> {
    let mut out = Vec::new();
    let mut tool_names_by_id = HashMap::new();

    for message in messages {
        match message {
            LlmMessage::System(text) if provider == AdkProvider::Gemini => {
                if !text.trim().is_empty() {
                    out.push(
                        AdkContent::new("user").with_text(format!("System instruction:\n{text}")),
                    );
                }
            }
            LlmMessage::System(text) => {
                if !text.trim().is_empty() {
                    out.push(AdkContent::new("system").with_text(text.clone()));
                }
            }
            LlmMessage::UserText(text) => out.push(AdkContent::new("user").with_text(text.clone())),
            LlmMessage::Assistant { blocks } => {
                let mut parts = Vec::new();
                for block in blocks {
                    match block {
                        LlmAssistantBlock::Text { text } if !text.trim().is_empty() => {
                            parts.push(AdkPart::Text { text: text.clone() });
                        }
                        LlmAssistantBlock::ToolUse { id, name, input } => {
                            tool_names_by_id.insert(id.clone(), name.clone());
                            parts.push(AdkPart::FunctionCall {
                                name: name.clone(),
                                args: input.clone(),
                                id: Some(id.clone()),
                                thought_signature: None,
                            });
                        }
                        _ => {}
                    }
                }
                if !parts.is_empty() {
                    out.push(AdkContent {
                        role: "model".to_string(),
                        parts,
                    });
                }
            }
            LlmMessage::UserToolResults { results } => {
                for result in results {
                    let name = tool_names_by_id
                        .get(&result.tool_use_id)
                        .cloned()
                        .unwrap_or_else(|| "tool".to_string());
                    let response = if result.is_error {
                        json!({
                            "is_error": true,
                            "content": result.content,
                        })
                    } else {
                        Value::String(result.content.clone())
                    };
                    out.push(AdkContent {
                        role: "function".to_string(),
                        parts: vec![AdkPart::FunctionResponse {
                            function_response: FunctionResponseData::new(name, response),
                            id: Some(result.tool_use_id.clone()),
                        }],
                    });
                }
            }
        }
    }

    out
}

fn adk_tools(tools: &[LlmTool]) -> HashMap<String, Value> {
    tools
        .iter()
        .map(|tool| {
            (
                tool.name.clone(),
                json!({
                    "description": tool.description,
                    "parameters": tool.parameters,
                }),
            )
        })
        .collect()
}

fn adk_completion_from_responses(responses: &[AdkLlmResponse]) -> Result<LlmCompletion> {
    let mut assistant_blocks = Vec::new();
    let mut finish_reason = None;
    let mut usage = LlmUsage::default();

    for response in responses {
        if let Some(error_message) = response
            .error_message
            .as_deref()
            .filter(|message| !message.trim().is_empty())
        {
            let error_code = response.error_code.as_deref().unwrap_or("unknown");
            bail!("ADK model response returned {error_code}: {error_message}");
        }

        if let Some(content) = &response.content {
            for part in &content.parts {
                match part {
                    AdkPart::Text { text } if !text.trim().is_empty() => {
                        assistant_blocks.push(LlmAssistantBlock::Text { text: text.clone() });
                    }
                    AdkPart::FunctionCall { name, args, id, .. } => {
                        assistant_blocks.push(LlmAssistantBlock::ToolUse {
                            id: id
                                .clone()
                                .unwrap_or_else(|| format!("adk_tool_{}", assistant_blocks.len())),
                            name: name.clone(),
                            input: args.clone(),
                        });
                    }
                    _ => {}
                }
            }
        }

        if let Some(response_finish_reason) = response.finish_reason {
            finish_reason = Some(response_finish_reason);
        }

        if let Some(response_usage) = &response.usage_metadata
            && let Some(next_usage) = adk_usage(response_usage)
        {
            usage.add_assign(&next_usage);
        }
    }

    let stop_reason = if assistant_blocks
        .iter()
        .any(|block| matches!(block, LlmAssistantBlock::ToolUse { .. }))
    {
        LlmStopReason::ToolUse
    } else {
        match finish_reason {
            Some(AdkFinishReason::Stop) | None => LlmStopReason::EndTurn,
            Some(AdkFinishReason::MaxTokens) => LlmStopReason::MaxTokens,
            Some(AdkFinishReason::Safety) => LlmStopReason::Unknown("safety".to_string()),
            Some(AdkFinishReason::Recitation) => LlmStopReason::Unknown("recitation".to_string()),
            Some(AdkFinishReason::Other) => LlmStopReason::Unknown("other".to_string()),
        }
    };

    Ok(LlmCompletion {
        assistant_blocks,
        stop_reason,
        usage: (!usage.is_empty()).then_some(usage),
    })
}

fn adk_usage(metadata: &AdkUsageMetadata) -> Option<LlmUsage> {
    let estimated_cost_micros = metadata.cost.and_then(|cost| {
        if cost.is_finite() && cost >= 0.0 {
            Some((cost * 1_000_000.0).round() as u64)
        } else {
            None
        }
    });
    let usage = LlmUsage {
        input_tokens: positive_i32_to_u64(metadata.prompt_token_count),
        output_tokens: positive_i32_to_u64(metadata.candidates_token_count),
        total_tokens: positive_i32_to_u64(metadata.total_token_count),
        estimated_cost_micros,
        estimated_cost_currency: estimated_cost_micros.map(|_| "USD".to_string()),
    };
    (!usage.is_empty()).then_some(usage)
}

fn positive_i32_to_u64(value: i32) -> Option<u64> {
    (value > 0).then_some(value as u64)
}

fn parse_openai_chat_completion_payload(payload: &Value) -> Result<LlmCompletion> {
    let choice = payload
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| anyhow!("OpenAI-compatible response did not include a choice"))?;
    let message = payload
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .ok_or_else(|| anyhow!("OpenAI-compatible response did not include a choice message"))?;

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
            let input = parse_tool_input(&name, function.get("arguments").cloned());
            assistant_blocks.push(LlmAssistantBlock::ToolUse { id, name, input });
        }
    }

    let stop_reason = if assistant_blocks
        .iter()
        .any(|block| matches!(block, LlmAssistantBlock::ToolUse { .. }))
    {
        LlmStopReason::ToolUse
    } else {
        parse_openai_finish_reason(choice.get("finish_reason"))
    };

    Ok(LlmCompletion {
        stop_reason,
        assistant_blocks,
        usage: parse_openai_usage(payload),
    })
}

fn parse_openai_usage(payload: &Value) -> Option<LlmUsage> {
    let usage = payload.get("usage")?;
    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(Value::as_u64);
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(Value::as_u64);
    let total_tokens = usage.get("total_tokens").and_then(Value::as_u64);
    let usage = LlmUsage {
        input_tokens,
        output_tokens,
        total_tokens,
        ..Default::default()
    };
    (!usage.is_empty()).then_some(usage)
}

fn parse_openai_finish_reason(finish_reason: Option<&Value>) -> LlmStopReason {
    match finish_reason.and_then(Value::as_str) {
        Some("stop") | None => LlmStopReason::EndTurn,
        Some("tool_calls") | Some("function_call") => LlmStopReason::ToolUse,
        Some("length") => LlmStopReason::MaxTokens,
        Some(other) => LlmStopReason::Unknown(other.to_string()),
    }
}

fn parse_tool_input(tool_name: &str, input: Option<Value>) -> Value {
    let Some(input) = input else {
        return json!({});
    };
    normalize_tool_input(tool_name, input)
}

fn normalize_tool_input(tool_name: &str, input: Value) -> Value {
    match input {
        Value::String(raw) => {
            parse_tool_input_string(tool_name, &raw).unwrap_or_else(|| Value::String(raw))
        }
        Value::Object(mut map) => {
            if map.len() == 1
                && let Some(wrapped) = map
                    .remove("arguments")
                    .or_else(|| map.remove("input"))
                    .or_else(|| map.remove("parameters"))
            {
                return normalize_tool_input(tool_name, wrapped);
            }
            Value::Object(map)
        }
        Value::Null => json!({}),
        other => other,
    }
}

fn parse_tool_input_string(tool_name: &str, raw: &str) -> Option<Value> {
    serde_json::from_str(raw)
        .ok()
        .or_else(|| {
            (tool_name == "local__write_file")
                .then(|| parse_relaxed_json_object(raw).ok())
                .flatten()
        })
        .map(|value| normalize_tool_input(tool_name, value))
}

fn parse_relaxed_json_object(raw: &str) -> Result<Value> {
    let mut parser = RelaxedJsonObjectParser::new(raw);
    parser.parse_object()
}

struct RelaxedJsonObjectParser {
    chars: Vec<char>,
    pos: usize,
}

impl RelaxedJsonObjectParser {
    fn new(raw: &str) -> Self {
        Self {
            chars: raw.chars().collect(),
            pos: 0,
        }
    }

    fn parse_object(&mut self) -> Result<Value> {
        self.skip_ws();
        self.expect('{')?;
        let mut map = serde_json::Map::new();
        loop {
            self.skip_ws();
            if self.consume('}') {
                break;
            }
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(':')?;
            self.skip_ws();
            let value = self.parse_value()?;
            map.insert(key, value);
            self.skip_ws();
            if self.consume('}') {
                break;
            }
            self.expect(',')?;
        }
        self.skip_ws();
        if self.pos != self.chars.len() {
            bail!("unexpected trailing content in tool arguments");
        }
        Ok(Value::Object(map))
    }

    fn parse_value(&mut self) -> Result<Value> {
        self.skip_ws();
        match self.peek() {
            Some('"') => Ok(Value::String(self.parse_string()?)),
            Some('{') => self.parse_object(),
            Some('t') => {
                self.expect_literal("true")?;
                Ok(Value::Bool(true))
            }
            Some('f') => {
                self.expect_literal("false")?;
                Ok(Value::Bool(false))
            }
            Some('n') => {
                self.expect_literal("null")?;
                Ok(Value::Null)
            }
            Some('-' | '0'..='9') => self.parse_number(),
            Some(other) => bail!("unexpected value start '{other}' in tool arguments"),
            None => bail!("unexpected end of tool arguments"),
        }
    }

    fn parse_string(&mut self) -> Result<String> {
        self.expect('"')?;
        let mut encoded = String::new();
        while let Some(ch) = self.next() {
            match ch {
                '"' => {
                    let quoted = format!("\"{encoded}\"");
                    return serde_json::from_str(&quoted)
                        .context("failed to decode relaxed JSON string");
                }
                '\\' => {
                    encoded.push('\\');
                    let escaped = self
                        .next()
                        .ok_or_else(|| anyhow!("unterminated escape in tool arguments"))?;
                    encoded.push(escaped);
                    if escaped == 'u' {
                        for _ in 0..4 {
                            let hex = self.next().ok_or_else(|| {
                                anyhow!("unterminated unicode escape in tool arguments")
                            })?;
                            encoded.push(hex);
                        }
                    }
                }
                '\n' => encoded.push_str("\\n"),
                '\r' => encoded.push_str("\\r"),
                '\t' => encoded.push_str("\\t"),
                ch if ch.is_control() => {
                    bail!("unsupported control character in tool arguments")
                }
                other => encoded.push(other),
            }
        }
        bail!("unterminated string in tool arguments")
    }

    fn parse_number(&mut self) -> Result<Value> {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch == ',' || ch == '}' || ch.is_whitespace() {
                break;
            }
            self.pos += 1;
        }
        let raw = self.chars[start..self.pos].iter().collect::<String>();
        serde_json::from_str(&raw).with_context(|| format!("invalid number '{raw}'"))
    }

    fn expect_literal(&mut self, literal: &str) -> Result<()> {
        for expected in literal.chars() {
            self.expect(expected)?;
        }
        Ok(())
    }

    fn expect(&mut self, expected: char) -> Result<()> {
        match self.next() {
            Some(actual) if actual == expected => Ok(()),
            Some(actual) => bail!(
                "expected '{expected}' at character {}, got '{actual}' in tool arguments",
                self.pos.saturating_sub(1)
            ),
            None => bail!("expected '{expected}', got end of tool arguments"),
        }
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += 1;
        Some(ch)
    }
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
        usage: parse_anthropic_usage(payload),
    })
}

fn parse_anthropic_usage(payload: &Value) -> Option<LlmUsage> {
    let usage = payload.get("usage")?;
    let input_tokens = usage.get("input_tokens").and_then(Value::as_u64);
    let output_tokens = usage.get("output_tokens").and_then(Value::as_u64);
    let total_tokens = match (input_tokens, output_tokens) {
        (Some(input), Some(output)) => Some(input.saturating_add(output)),
        _ => None,
    };
    let usage = LlmUsage {
        input_tokens,
        output_tokens,
        total_tokens,
        ..Default::default()
    };
    (!usage.is_empty()).then_some(usage)
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
    let input = parse_tool_input(&name, block.get("input").cloned());
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
    use std::sync::Arc;

    use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::Mutex};

    use crate::{
        config::{
            AdkConfig, AdkProvider, AppConfig, LlmProvider, LocalToolsConfig, McpRuntimeConfig,
            OpenAiCompatibleConfig, OpenAiConfig, SkillsConfig,
        },
        types::AgentPermissions,
    };

    use super::{
        AdkPart, LlmAssistantBlock, LlmMessage, LlmStopReason, LlmTool, LlmToolResult,
        OpenAiClient, OpenAiCompatibleClient, adk_contents, adk_tools,
        build_anthropic_chat_request_body, build_openai_chat_request_body,
        build_openai_compatible_chat_request_body, openai_compatible_chat_url,
        parse_anthropic_chat_completion_payload, parse_openai_chat_completion_payload,
        truncate_for_log,
    };

    #[derive(Debug, Clone)]
    struct CapturedOpenAiRequest {
        authorization: Option<String>,
        body: Value,
    }

    type CapturedOpenAiState = Arc<Mutex<Option<CapturedOpenAiRequest>>>;

    async fn capture_openai_request(
        State(state): State<CapturedOpenAiState>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        let authorization = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        *state.lock().await = Some(CapturedOpenAiRequest {
            authorization,
            body,
        });

        Json(json!({
            "choices": [{
                "finish_reason": "stop",
                "message": {
                    "content": "done"
                }
            }]
        }))
    }

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
    fn llm_client_exposes_active_model_backend_capabilities() {
        let config = AppConfig {
            prompt: None,
            data_dir: None,
            llm_provider: Some(LlmProvider::OpenAi),
            azure_openai: None,
            openai: Some(OpenAiConfig {
                api_key: "test-key".to_string(),
                endpoint: "https://api.openai.com/v1".to_string(),
                model: "gpt-test".to_string(),
                temperature: 0.2,
                top_p: 1.0,
                max_output_tokens: 1_024,
                max_advertised_tools: 128,
            }),
            azure_anthropic: None,
            openai_compatible: None,
            adk: None,
            agent_permissions: AgentPermissions::default(),
            local_tools: LocalToolsConfig::default(),
            tool_environment: Default::default(),
            agent: Default::default(),
            skills: SkillsConfig::default(),
            mcp_runtime: McpRuntimeConfig::default(),
            mcp_servers: Vec::new(),
            tracing: None,
        };

        let client = super::LlmClient::new(&config).expect("client should build");
        let capabilities = client
            .model_capabilities()
            .expect("active backend should expose capabilities");

        assert_eq!(client.provider_label(), "OpenAI");
        assert!(capabilities.tool_calling);
        assert!(capabilities.usage_metadata);
        assert!(!capabilities.streaming);
    }

    #[test]
    fn llm_client_exposes_adk_backend_capabilities() {
        let config = AppConfig {
            prompt: None,
            data_dir: None,
            llm_provider: Some(LlmProvider::Adk),
            azure_openai: None,
            openai: None,
            azure_anthropic: None,
            openai_compatible: None,
            adk: Some(AdkConfig {
                provider: AdkProvider::AzureAi,
                api_key: "test-key".to_string(),
                endpoint: Some("https://example.invalid".to_string()),
                model: "test-model".to_string(),
                temperature: 0.2,
                top_p: 1.0,
                max_output_tokens: 1_024,
                max_advertised_tools: 128,
            }),
            agent_permissions: AgentPermissions::default(),
            local_tools: LocalToolsConfig::default(),
            tool_environment: Default::default(),
            agent: Default::default(),
            skills: SkillsConfig::default(),
            mcp_runtime: McpRuntimeConfig::default(),
            mcp_servers: Vec::new(),
            tracing: None,
        };

        let client = super::LlmClient::new(&config).expect("client should build");
        let capabilities = client
            .model_capabilities()
            .expect("active backend should expose capabilities");

        assert_eq!(client.provider_label(), "ADK Azure AI");
        assert!(capabilities.tool_calling);
        assert!(capabilities.usage_metadata);
    }

    #[test]
    fn builds_adk_contents_with_tool_results() {
        let contents = adk_contents(
            AdkProvider::AzureAi,
            &[
                LlmMessage::System("system prompt".to_string()),
                LlmMessage::Assistant {
                    blocks: vec![LlmAssistantBlock::ToolUse {
                        id: "call-1".to_string(),
                        name: "lookup".to_string(),
                        input: json!({"query": "example"}),
                    }],
                },
                LlmMessage::UserToolResults {
                    results: vec![LlmToolResult {
                        tool_use_id: "call-1".to_string(),
                        content: "done".to_string(),
                        is_error: false,
                    }],
                },
            ],
        );

        assert_eq!(contents[0].role, "system");
        assert_eq!(contents[1].role, "model");
        assert_eq!(contents[2].role, "function");
        match &contents[2].parts[0] {
            AdkPart::FunctionResponse {
                function_response, ..
            } => {
                assert_eq!(function_response.name, "lookup");
                assert_eq!(function_response.response, json!("done"));
            }
            part => panic!("expected function response, got {part:?}"),
        }
    }

    #[test]
    fn builds_adk_tools() {
        let tools = adk_tools(&[LlmTool {
            name: "demo".to_string(),
            description: "Demo tool".to_string(),
            parameters: json!({"type": "object"}),
        }]);

        assert_eq!(tools["demo"]["description"], json!("Demo tool"));
        assert_eq!(tools["demo"]["parameters"]["type"], json!("object"));
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
    fn builds_openai_compatible_request_with_model() {
        let body = build_openai_compatible_chat_request_body(
            &[LlmMessage::UserText("hello".to_string())],
            &[],
            "openrouter/anthropic/claude-sonnet",
            0.2,
            1.0,
            1200,
        );

        assert_eq!(body["model"], json!("openrouter/anthropic/claude-sonnet"));
        assert_eq!(body["messages"][0]["role"], json!("user"));
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn builds_openai_compatible_chat_url() {
        assert_eq!(
            openai_compatible_chat_url("http://127.0.0.1:4000/v1/"),
            "http://127.0.0.1:4000/v1/chat/completions"
        );
        assert_eq!(
            openai_compatible_chat_url("http://127.0.0.1:4000/v1/chat/completions"),
            "http://127.0.0.1:4000/v1/chat/completions"
        );
    }

    #[tokio::test]
    async fn openai_compatible_client_posts_chat_completion_request() {
        let captured = Arc::new(Mutex::new(None));
        let app = Router::new()
            .route("/v1/chat/completions", post(capture_openai_request))
            .with_state(captured.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = OpenAiCompatibleClient::new(&OpenAiCompatibleConfig {
            api_key: Some("test-key".to_string()),
            base_url: format!("http://{addr}/v1"),
            model: "test-model".to_string(),
            temperature: 0.2,
            top_p: 1.0,
            max_output_tokens: 1200,
            max_advertised_tools: 128,
        })
        .unwrap();

        let completion = client
            .chat_completion(
                &[LlmMessage::UserText("hello".to_string())],
                &[LlmTool {
                    name: "demo".to_string(),
                    description: "Demo tool".to_string(),
                    parameters: json!({"type": "object"}),
                }],
            )
            .await
            .unwrap();

        let request = captured.lock().await.clone().unwrap();
        assert_eq!(request.authorization.as_deref(), Some("Bearer test-key"));
        assert_eq!(request.body["model"], json!("test-model"));
        assert_eq!(request.body["tool_choice"], json!("auto"));
        assert_eq!(request.body["tools"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            completion.assistant_blocks,
            vec![LlmAssistantBlock::Text {
                text: "done".to_string()
            }]
        );
    }

    #[tokio::test]
    async fn openai_client_posts_chat_completion_request() {
        let captured = Arc::new(Mutex::new(None));
        let app = Router::new()
            .route("/v1/chat/completions", post(capture_openai_request))
            .with_state(captured.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = OpenAiClient::new(&OpenAiConfig {
            api_key: "openai-key".to_string(),
            endpoint: format!("http://{addr}/v1"),
            model: "gpt-5".to_string(),
            temperature: 0.2,
            top_p: 1.0,
            max_output_tokens: 1200,
            max_advertised_tools: 128,
        })
        .unwrap();

        let completion = client
            .chat_completion(&[LlmMessage::UserText("hello".to_string())], &[])
            .await
            .unwrap();

        let request = captured.lock().await.clone().unwrap();
        assert_eq!(request.authorization.as_deref(), Some("Bearer openai-key"));
        assert_eq!(request.body["model"], json!("gpt-5"));
        assert!(request.body.get("tools").is_none());
        assert_eq!(
            completion.assistant_blocks,
            vec![LlmAssistantBlock::Text {
                text: "done".to_string()
            }]
        );
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
    fn parses_openai_tool_arguments_when_returned_as_object() {
        let large_text = "x".repeat(15_000);
        let payload = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {
                            "name": "local__write_file",
                            "arguments": {
                                "path": "large.txt",
                                "text": large_text
                            }
                        }
                    }]
                }
            }]
        });

        let completion = parse_openai_chat_completion_payload(&payload).unwrap();
        let LlmAssistantBlock::ToolUse { input, .. } = &completion.assistant_blocks[0] else {
            panic!("expected tool use");
        };

        assert_eq!(input["path"], "large.txt");
        assert_eq!(input["text"].as_str().unwrap().len(), 15_000);
    }

    #[test]
    fn parses_openai_write_file_large_markdown_arguments_string() {
        let large_markdown = (0..530)
            .map(|line| {
                format!("## Section {line}\n\n| columnA | columnB |\n| --- | --- |\n| valueforA | Value for B |\n\n")
            })
            .collect::<String>();
        assert!(large_markdown.len() > 15_000);
        let raw_arguments = serde_json::to_string(&json!({
            "path": "large.md",
            "mode": "overwrite",
            "text": large_markdown,
        }))
        .unwrap();
        let payload = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {
                            "name": "local__write_file",
                            "arguments": raw_arguments
                        }
                    }]
                }
            }]
        });

        let completion = parse_openai_chat_completion_payload(&payload).unwrap();
        let LlmAssistantBlock::ToolUse { input, .. } = &completion.assistant_blocks[0] else {
            panic!("expected tool use");
        };

        assert_eq!(input["path"], "large.md");
        assert!(input["text"].as_str().unwrap().len() > 15_000);
        assert!(
            input["text"]
                .as_str()
                .unwrap()
                .contains("| columnA | columnB |")
        );
    }

    #[test]
    fn repairs_openai_write_file_arguments_with_literal_markdown_newlines() {
        let raw_arguments = "{\"path\":\"report.md\",\"mode\":\"overwrite\",\"text\":\"# Report\n\n| columnA | columnB |\n| --- | --- |\n| valueforA | Value for B |\n\n```markdown\nbody\n```\"}";
        let payload = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {
                            "name": "local__write_file",
                            "arguments": raw_arguments
                        }
                    }]
                }
            }]
        });

        let completion = parse_openai_chat_completion_payload(&payload).unwrap();
        let LlmAssistantBlock::ToolUse { input, .. } = &completion.assistant_blocks[0] else {
            panic!("expected tool use");
        };

        assert_eq!(input["path"], "report.md");
        assert_eq!(input["mode"], "overwrite");
        assert!(
            input["text"]
                .as_str()
                .unwrap()
                .contains("```markdown\nbody\n```")
        );
    }

    #[test]
    fn parses_anthropic_write_file_input_when_returned_as_serialized_json() {
        let large_markdown = "# Heading\n\n".repeat(2_000);
        let raw_input = serde_json::to_string(&json!({
            "path": "anthropic-large.md",
            "text": large_markdown,
        }))
        .unwrap();
        let payload = json!({
            "stop_reason": "tool_use",
            "content": [
                {"type": "tool_use", "id": "toolu_1", "name": "local__write_file", "input": raw_input}
            ]
        });

        let completion = parse_anthropic_chat_completion_payload(&payload).unwrap();
        let LlmAssistantBlock::ToolUse { input, .. } = &completion.assistant_blocks[0] else {
            panic!("expected tool use");
        };

        assert_eq!(input["path"], "anthropic-large.md");
        assert!(input["text"].as_str().unwrap().len() > 15_000);
    }

    #[test]
    fn parses_openai_length_finish_reason() {
        let payload = json!({
            "choices": [{
                "finish_reason": "length",
                "message": {
                    "content": "partial"
                }
            }]
        });

        let completion = parse_openai_chat_completion_payload(&payload).unwrap();

        assert_eq!(completion.stop_reason, LlmStopReason::MaxTokens);
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
