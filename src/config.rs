use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::types::AgentPermissions;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    pub prompt: Option<String>,
    pub data_dir: Option<PathBuf>,
    pub llm_provider: Option<LlmProvider>,
    pub azure_openai: Option<AzureOpenAiConfig>,
    pub azure_anthropic: Option<AzureAnthropicConfig>,
    #[serde(default)]
    pub agent_permissions: AgentPermissions,
    #[serde(default)]
    pub local_tools: LocalToolsConfig,
    #[serde(default)]
    pub mcp_runtime: McpRuntimeConfig,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
    #[serde(default)]
    pub tracing: Option<TracingConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TracingConfig {
    #[serde(default)]
    pub provider: TracingProvider,
    pub phoenix_endpoint: Option<String>,
    pub phoenix_project: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TracingProvider {
    #[default]
    None,
    Console,
    Phoenix,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AzureOpenAiConfig {
    pub api_key: String,
    pub api_version: String,
    pub endpoint: String,
    pub deployment: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_top_p")]
    pub top_p: f32,
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AzureAnthropicConfig {
    pub api_key: String,
    #[serde(default)]
    pub api_version: Option<String>,
    #[serde(default)]
    pub anthropic_version: Option<String>,
    pub endpoint: String,
    pub deployment: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u32,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmProvider {
    AzureOpenAi,
    AzureAnthropic,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct McpRuntimeConfig {
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_seconds: u64,
    #[serde(default = "default_cleanup_timeout")]
    pub cleanup_timeout_seconds: u64,
    #[serde(default)]
    pub connect_in_parallel: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LocalToolsConfig {
    #[serde(default = "default_local_tool_execution_timeout_seconds")]
    pub execution_timeout_seconds: u64,
    #[serde(default = "default_allowed_cli_tools")]
    pub allowed_cli_tools: Vec<String>,
}

impl Default for LocalToolsConfig {
    fn default() -> Self {
        Self {
            execution_timeout_seconds: default_local_tool_execution_timeout_seconds(),
            allowed_cli_tools: default_allowed_cli_tools(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: String,
    #[serde(default)]
    pub url: String,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub timeout: Option<u64>,
    pub sse_read_timeout: Option<u64>,
    pub client_session_timeout_seconds: Option<u64>,
    pub auth: Option<McpAuthConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpAuthConfig {
    OauthPublic(Box<McpOauthPublicConfig>),
    StaticHeaders(McpStaticHeadersConfig),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpOauthPublicConfig {
    #[serde(default)]
    pub scopes: Vec<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    #[serde(default = "default_token_endpoint_auth_method")]
    pub token_endpoint_auth_method: String,
    pub resource: Option<String>,
    pub redirect_uri: String,
    pub redirect_host: Option<String>,
    pub redirect_port: Option<u16>,
    pub redirect_path: Option<String>,
    #[serde(default = "default_callback_timeout_seconds")]
    pub callback_timeout_seconds: u64,
    #[serde(default = "default_true")]
    pub open_browser: bool,
    #[serde(default)]
    pub use_dynamic_client_registration: bool,
    pub client_name: Option<String>,
    pub authorization_endpoint: Option<String>,
    pub token_endpoint: Option<String>,
    pub registration_endpoint: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpStaticHeadersConfig {
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

impl AppConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;
        let mut config: AppConfig =
            serde_yaml::from_str(&raw).context("failed to parse YAML configuration")?;
        config.resolve_secrets()?;
        config.validate()?;
        Ok(config)
    }

    pub fn data_dir(&self) -> PathBuf {
        self.data_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("data"))
    }

    fn resolve_secrets(&mut self) -> Result<()> {
        if let Some(azure_openai) = &mut self.azure_openai {
            azure_openai.api_key = resolve_value(&azure_openai.api_key)?;
            azure_openai.endpoint = resolve_value(&azure_openai.endpoint)?;
        }
        if let Some(azure_anthropic) = &mut self.azure_anthropic {
            azure_anthropic.api_key = resolve_value(&azure_anthropic.api_key)?;
            azure_anthropic.endpoint = resolve_value(&azure_anthropic.endpoint)?;
        }
        for server in &mut self.mcp_servers {
            server.url = resolve_value(&server.url)?;
            if let Some(command) = &mut server.command {
                *command = resolve_value(command)?;
            }
            for arg in &mut server.args {
                *arg = resolve_value(arg)?;
            }
            for value in server.headers.values_mut() {
                *value = resolve_value(value)?;
            }
            if let Some(auth) = &mut server.auth {
                match auth {
                    McpAuthConfig::OauthPublic(auth) => {
                        if let Some(client_id) = &mut auth.client_id {
                            *client_id = resolve_value(client_id)?;
                        }
                        if let Some(client_secret) = &mut auth.client_secret {
                            *client_secret = resolve_value(client_secret)?;
                        }
                        auth.redirect_uri = resolve_value(&auth.redirect_uri)?;
                        if let Some(resource) = &mut auth.resource {
                            *resource = resolve_value(resource)?;
                        }
                        if let Some(endpoint) = &mut auth.authorization_endpoint {
                            *endpoint = resolve_value(endpoint)?;
                        }
                        if let Some(endpoint) = &mut auth.token_endpoint {
                            *endpoint = resolve_value(endpoint)?;
                        }
                        if let Some(endpoint) = &mut auth.registration_endpoint {
                            *endpoint = resolve_value(endpoint)?;
                        }
                    }
                    McpAuthConfig::StaticHeaders(static_headers) => {
                        for value in static_headers.headers.values_mut() {
                            *value = resolve_value(value)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        validate_azure_openai_config("azure_openai", self.azure_openai.as_ref())?;
        validate_azure_anthropic_config("azure_anthropic", self.azure_anthropic.as_ref())?;

        match self.effective_llm_provider() {
            Some(LlmProvider::AzureOpenAi) if self.azure_openai.is_none() => {
                bail!("llm_provider selects azure_openai but azure_openai is not configured");
            }
            Some(LlmProvider::AzureAnthropic) if self.azure_anthropic.is_none() => {
                bail!("llm_provider selects azure_anthropic but azure_anthropic is not configured");
            }
            _ => {}
        }

        for server in &self.mcp_servers {
            if server.transport != "streamable_http"
                && server.transport != "sse"
                && server.transport != "stdio"
            {
                bail!(
                    "unsupported transport '{}' for server '{}'; only streamable_http, sse, and stdio are supported",
                    server.transport,
                    server.name
                );
            }
            if server.transport == "stdio" {
                if server
                    .command
                    .as_deref()
                    .is_none_or(|command| command.trim().is_empty())
                {
                    bail!("mcp_servers[].command must not be empty when transport is stdio");
                }
            } else if server.url.trim().is_empty() {
                bail!("mcp_servers[].url must not be empty");
            }
            if let Some(auth) = &server.auth {
                match auth {
                    McpAuthConfig::OauthPublic(auth) => {
                        if auth.redirect_uri.trim().is_empty() {
                            bail!("mcp_servers[].auth.redirect_uri must not be empty");
                        }
                        if auth.use_dynamic_client_registration
                            && auth.token_endpoint_auth_method.trim().is_empty()
                        {
                            bail!(
                                "mcp_servers[].auth.token_endpoint_auth_method must not be empty when dynamic registration is enabled"
                            );
                        }
                        if !auth.use_dynamic_client_registration && auth.client_id.is_none() {
                            bail!(
                                "mcp_servers[].auth.client_id is required when dynamic registration is disabled"
                            );
                        }
                    }
                    McpAuthConfig::StaticHeaders(_) => {}
                }
            }
        }
        Ok(())
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let payload =
            serde_yaml::to_string(self).context("failed to serialize YAML configuration")?;
        fs::write(path, payload)
            .with_context(|| format!("failed to write config file {}", path.display()))
    }

    pub fn effective_llm_provider(&self) -> Option<LlmProvider> {
        if let Some(provider) = self.llm_provider {
            return Some(provider);
        }
        if self.azure_openai.is_some() {
            return Some(LlmProvider::AzureOpenAi);
        }
        if self.azure_anthropic.is_some() {
            return Some(LlmProvider::AzureAnthropic);
        }
        None
    }
}

fn validate_azure_openai_config(label: &str, config: Option<&AzureOpenAiConfig>) -> Result<()> {
    let Some(config) = config else {
        return Ok(());
    };
    if config.api_key.trim().is_empty() {
        bail!("{label}.api_key must not be empty");
    }
    if config.endpoint.trim().is_empty() {
        bail!("{label}.endpoint must not be empty");
    }
    if config.deployment.trim().is_empty() {
        bail!("{label}.deployment must not be empty");
    }
    if config.api_version.trim().is_empty() {
        bail!("{label}.api_version must not be empty");
    }
    Ok(())
}

fn validate_azure_anthropic_config(
    label: &str,
    config: Option<&AzureAnthropicConfig>,
) -> Result<()> {
    let Some(config) = config else {
        return Ok(());
    };
    if config.api_key.trim().is_empty() {
        bail!("{label}.api_key must not be empty");
    }
    if config.endpoint.trim().is_empty() {
        bail!("{label}.endpoint must not be empty");
    }
    if config.deployment.trim().is_empty() {
        bail!("{label}.deployment must not be empty");
    }
    if let Some(version) = config.anthropic_version.as_deref() {
        if version.trim().is_empty() {
            bail!("{label}.anthropic_version must not be empty when set");
        }
        if !is_anthropic_version(version.trim()) {
            bail!(
                "{label}.anthropic_version must look like an Anthropic API version such as 2023-06-01"
            );
        }
    }
    Ok(())
}

impl AzureAnthropicConfig {
    pub fn effective_anthropic_version(&self) -> String {
        self.anthropic_version
            .as_deref()
            .map(str::trim)
            .filter(|version| !version.is_empty())
            .map(str::to_string)
            .or_else(|| {
                self.api_version
                    .as_deref()
                    .map(str::trim)
                    .filter(|version| is_anthropic_version(version))
                    .map(str::to_string)
            })
            .unwrap_or_else(default_anthropic_version)
    }

    pub fn ignored_api_version(&self) -> Option<&str> {
        let has_explicit_anthropic_version = self
            .anthropic_version
            .as_deref()
            .map(str::trim)
            .is_some_and(|version| !version.is_empty());
        if has_explicit_anthropic_version {
            return None;
        }
        self.api_version
            .as_deref()
            .map(str::trim)
            .filter(|version| !version.is_empty() && !is_anthropic_version(version))
    }

    pub fn effective_top_p(&self) -> Option<f32> {
        self.top_p.filter(|top_p| *top_p < 0.99)
    }
}

fn resolve_value(value: &str) -> Result<String> {
    if let Some(var_name) = value.strip_prefix("env:") {
        let var_name = var_name.trim();
        let resolved = std::env::var(var_name)
            .with_context(|| format!("environment variable {var_name} is not set"))?;
        if resolved.trim().is_empty() {
            bail!("environment variable {var_name} resolved to an empty value");
        }
        Ok(resolved)
    } else {
        Ok(value.to_string())
    }
}

fn default_allowed_cli_tools() -> Vec<String> {
    ["nmap", "vt", "dig", "whois", "nslookup"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

const fn default_temperature() -> f32 {
    0.2
}

const fn default_top_p() -> f32 {
    1.0
}

const fn default_max_output_tokens() -> u32 {
    1200
}

fn default_anthropic_version() -> String {
    "2023-06-01".to_string()
}

fn is_anthropic_version(version: &str) -> bool {
    let bytes = version.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
}

const fn default_connect_timeout() -> u64 {
    180
}

const fn default_cleanup_timeout() -> u64 {
    10
}

const fn default_local_tool_execution_timeout_seconds() -> u64 {
    180
}

fn default_token_endpoint_auth_method() -> String {
    "none".to_string()
}

const fn default_callback_timeout_seconds() -> u64 {
    300
}

const fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::types::FilesystemAccess;

    use super::{AppConfig, AzureAnthropicConfig, LlmProvider};

    #[test]
    fn resolves_env_backed_secrets() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        unsafe {
            std::env::set_var("TEST_AOAI_KEY", "super-secret");
        }
        fs::write(
            &path,
            r#"
azure_openai:
  api_key: env:TEST_AOAI_KEY
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/
  deployment: gpt-4.1
mcp_servers:
  - name: demo
    transport: streamable_http
    url: http://127.0.0.1:5000/mcp
    headers: {}
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert_eq!(
            config.azure_openai.as_ref().map(|cfg| cfg.api_key.as_str()),
            Some("super-secret")
        );
        assert_eq!(config.local_tools.execution_timeout_seconds, 180);
    }

    #[test]
    fn resolves_env_backed_anthropic_secrets() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        unsafe {
            std::env::set_var("TEST_AANTH_KEY", "anthropic-secret");
        }
        fs::write(
            &path,
            r#"
azure_anthropic:
  api_key: env:TEST_AANTH_KEY
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/anthropic/
  deployment: claude-opus-4-6
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert_eq!(
            config
                .azure_anthropic
                .as_ref()
                .map(|cfg| cfg.api_key.as_str()),
            Some("anthropic-secret")
        );
        assert_eq!(
            config
                .azure_anthropic
                .as_ref()
                .map(AzureAnthropicConfig::effective_anthropic_version)
                .as_deref(),
            Some("2023-06-01")
        );
        assert_eq!(
            config.effective_llm_provider(),
            Some(LlmProvider::AzureAnthropic)
        );
    }

    #[test]
    fn prefers_explicit_anthropic_version_when_set() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
azure_anthropic:
  api_key: test
  anthropic_version: 2023-01-01
  endpoint: https://example.invalid/anthropic/
  deployment: claude-opus-4-6
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert_eq!(
            config
                .azure_anthropic
                .as_ref()
                .map(AzureAnthropicConfig::effective_anthropic_version)
                .as_deref(),
            Some("2023-01-01")
        );
    }

    #[test]
    fn defaults_to_openai_when_both_providers_are_configured_and_selector_is_omitted() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
azure_openai:
  api_key: test
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/
  deployment: gpt-4.1
azure_anthropic:
  api_key: test
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/anthropic/
  deployment: claude-opus-4-6
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert_eq!(
            config.effective_llm_provider(),
            Some(LlmProvider::AzureOpenAi)
        );
    }

    #[test]
    fn rejects_missing_selected_provider_block() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
llm_provider: azure_anthropic
azure_openai:
  api_key: test
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/
  deployment: gpt-4.1
"#,
        )
        .unwrap();

        let err = AppConfig::load(&path).unwrap_err();
        assert!(format!("{err:#}").contains(
            "llm_provider selects azure_anthropic but azure_anthropic is not configured"
        ));
    }

    #[test]
    fn allows_empty_mcp_servers() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
azure_openai:
  api_key: test
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/
  deployment: gpt-4.1
mcp_servers: []
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert!(config.mcp_servers.is_empty());
        assert_eq!(config.local_tools.execution_timeout_seconds, 180);
    }

    #[test]
    fn allows_omitted_mcp_servers_field() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
azure_openai:
  api_key: test
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/
  deployment: gpt-4.1
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert!(config.mcp_servers.is_empty());
        assert_eq!(config.local_tools.execution_timeout_seconds, 180);
    }

    #[test]
    fn parses_local_tool_timeout_override() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
azure_openai:
  api_key: test
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/
  deployment: gpt-4.1
local_tools:
  execution_timeout_seconds: 240
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert_eq!(config.local_tools.execution_timeout_seconds, 240);
        assert_eq!(
            config.local_tools.allowed_cli_tools,
            vec!["nmap", "vt", "dig", "whois", "nslookup"]
        );
    }

    #[test]
    fn parses_allowed_cli_tool_override() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
azure_openai:
  api_key: test
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/
  deployment: gpt-4.1
local_tools:
  allowed_cli_tools:
    - whois
    - dig
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert_eq!(config.local_tools.execution_timeout_seconds, 180);
        assert_eq!(config.local_tools.allowed_cli_tools, vec!["whois", "dig"]);
    }

    #[test]
    fn parses_agent_permissions_block() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
azure_openai:
  api_key: test
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/
  deployment: gpt-4.1
agent_permissions:
  allow_network: true
  filesystem: read_write
  yolo: false
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert!(config.agent_permissions.allow_network);
        assert_eq!(
            config.agent_permissions.filesystem,
            FilesystemAccess::ReadWrite
        );
        assert!(!config.agent_permissions.yolo);
    }

    #[test]
    fn accepts_stdio_mcp_servers_without_url() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
azure_openai:
  api_key: test
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/
  deployment: gpt-4.1
mcp_servers:
  - name: chrome-devtools
    transport: stdio
    command: npx
    args:
      - -y
      - chrome-devtools-mcp@latest
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert_eq!(config.mcp_servers.len(), 1);
        assert_eq!(config.mcp_servers[0].transport, "stdio");
        assert_eq!(config.mcp_servers[0].command.as_deref(), Some("npx"));
        assert_eq!(
            config.mcp_servers[0].args,
            vec!["-y", "chrome-devtools-mcp@latest"]
        );
        assert!(config.mcp_servers[0].url.is_empty());
    }
}
