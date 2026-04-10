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
    pub azure_openai: Option<AzureOpenAiConfig>,
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
}

impl Default for LocalToolsConfig {
    fn default() -> Self {
        Self {
            execution_timeout_seconds: default_local_tool_execution_timeout_seconds(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: String,
    pub url: String,
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
        for server in &mut self.mcp_servers {
            server.url = resolve_value(&server.url)?;
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
        if let Some(azure_openai) = &self.azure_openai {
            if azure_openai.api_key.trim().is_empty() {
                bail!("azure_openai.api_key must not be empty");
            }
            if azure_openai.endpoint.trim().is_empty() {
                bail!("azure_openai.endpoint must not be empty");
            }
            if azure_openai.deployment.trim().is_empty() {
                bail!("azure_openai.deployment must not be empty");
            }
            if azure_openai.api_version.trim().is_empty() {
                bail!("azure_openai.api_version must not be empty");
            }
        }
        for server in &self.mcp_servers {
            if server.transport != "streamable_http" && server.transport != "sse" {
                bail!(
                    "unsupported transport '{}' for server '{}'; only streamable_http and sse are supported",
                    server.transport,
                    server.name
                );
            }
            if server.url.trim().is_empty() {
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

const fn default_temperature() -> f32 {
    0.2
}

const fn default_top_p() -> f32 {
    1.0
}

const fn default_max_output_tokens() -> u32 {
    1200
}

const fn default_connect_timeout() -> u64 {
    15
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

    use super::AppConfig;

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
}
