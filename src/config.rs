use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::types::AgentPermissions;

pub const DEFAULT_MAX_ADVERTISED_TOOLS: usize = 128;
pub const DEFAULT_MAX_AGENT_ITERATIONS: usize = 10;
pub const DEFAULT_CONTINUATION_INCREMENT: usize = 10;
pub const DEFAULT_MAX_TOTAL_AGENT_ITERATIONS: usize = 50;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    pub prompt: Option<String>,
    pub data_dir: Option<PathBuf>,
    pub llm_provider: Option<LlmProvider>,
    pub azure_openai: Option<AzureOpenAiConfig>,
    pub openai: Option<OpenAiConfig>,
    pub azure_anthropic: Option<AzureAnthropicConfig>,
    pub openai_compatible: Option<OpenAiCompatibleConfig>,
    pub adk: Option<AdkConfig>,
    #[serde(default)]
    pub agent_permissions: AgentPermissions,
    #[serde(default)]
    pub local_tools: LocalToolsConfig,
    #[serde(default)]
    pub tool_environment: ToolEnvironmentConfig,
    #[serde(default)]
    pub agent: AgentRuntimeConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub mcp_runtime: McpRuntimeConfig,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
    #[serde(default)]
    pub tracing: Option<TracingConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentRuntimeConfig {
    #[serde(default = "default_max_agent_iterations")]
    pub max_iterations_per_turn: usize,
    #[serde(default = "default_continuation_increment")]
    pub continuation_increment: usize,
    #[serde(default = "default_max_total_agent_iterations")]
    pub max_total_iterations_per_turn: usize,
}

impl Default for AgentRuntimeConfig {
    fn default() -> Self {
        Self {
            max_iterations_per_turn: default_max_agent_iterations(),
            continuation_increment: default_continuation_increment(),
            max_total_iterations_per_turn: default_max_total_agent_iterations(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct ToolEnvironmentConfig {
    #[serde(default)]
    pub pass_through: Vec<String>,
    #[serde(default)]
    pub variables: HashMap<String, String>,
    #[serde(default)]
    pub path_prepend: Vec<PathBuf>,
}

impl ToolEnvironmentConfig {
    pub fn apply_to_command(&self, cmd: &mut tokio::process::Command) -> Result<()> {
        for name in &self.pass_through {
            let Some(value) = std::env::var_os(name) else {
                bail!("tool_environment.pass_through variable {name} is not set");
            };
            if value.as_os_str().is_empty() {
                bail!("tool_environment.pass_through variable {name} resolved to an empty value");
            }
            cmd.env(name, value);
        }

        for (name, value) in &self.variables {
            cmd.env(name, value);
        }

        if !self.path_prepend.is_empty() {
            let mut paths = self.path_prepend.clone();
            let base_path = self
                .variables
                .get("PATH")
                .map(OsString::from)
                .or_else(|| std::env::var_os("PATH"));
            if let Some(base_path) = base_path
                && !base_path.as_os_str().is_empty()
            {
                paths.extend(std::env::split_paths(&base_path));
            }
            let joined = std::env::join_paths(paths)
                .context("failed to build child PATH from tool_environment.path_prepend")?;
            cmd.env("PATH", joined);
        }

        Ok(())
    }

    fn resolve_values(&mut self) -> Result<()> {
        for (name, value) in &mut self.variables {
            validate_env_name("tool_environment.variables", name)?;
            *value = resolve_value(value)?;
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        for name in &self.pass_through {
            validate_env_name("tool_environment.pass_through", name)?;
            let Some(value) = std::env::var_os(name) else {
                bail!("tool_environment.pass_through variable {name} is not set");
            };
            if value.as_os_str().is_empty() {
                bail!("tool_environment.pass_through variable {name} resolved to an empty value");
            }
        }
        for name in self.variables.keys() {
            validate_env_name("tool_environment.variables", name)?;
        }
        for path in &self.path_prepend {
            if path.as_os_str().is_empty() {
                bail!("tool_environment.path_prepend entries must not be empty");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SkillsConfig {
    #[serde(default)]
    pub project_skills: ProjectSkillsPolicy,
    #[serde(default)]
    pub trusted_project_roots: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectSkillsPolicy {
    #[default]
    TrustedOnly,
    Always,
    Disabled,
}

impl SkillsConfig {
    pub fn allows_project_skill_dirs(&self, project_root: &Path) -> bool {
        match self.project_skills {
            ProjectSkillsPolicy::Always => true,
            ProjectSkillsPolicy::Disabled => false,
            ProjectSkillsPolicy::TrustedOnly => {
                let project_root = normalize_path_for_compare(project_root);
                self.trusted_project_roots
                    .iter()
                    .any(|trusted| normalize_path_for_compare(trusted) == project_root)
            }
        }
    }
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
    #[serde(default = "default_max_advertised_tools")]
    pub max_advertised_tools: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenAiConfig {
    pub api_key: String,
    #[serde(default = "default_openai_endpoint")]
    pub endpoint: String,
    pub model: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_top_p")]
    pub top_p: f32,
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u32,
    #[serde(default = "default_max_advertised_tools")]
    pub max_advertised_tools: usize,
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
    #[serde(default = "default_max_advertised_tools")]
    pub max_advertised_tools: usize,
    #[serde(default)]
    pub input_cost_per_million_tokens: Option<f64>,
    #[serde(default)]
    pub output_cost_per_million_tokens: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenAiCompatibleConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_top_p")]
    pub top_p: f32,
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u32,
    #[serde(default = "default_max_advertised_tools")]
    pub max_advertised_tools: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AdkConfig {
    pub provider: AdkProvider,
    pub api_key: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    pub model: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_top_p")]
    pub top_p: f32,
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u32,
    #[serde(default = "default_max_advertised_tools")]
    pub max_advertised_tools: usize,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdkProvider {
    Gemini,
    #[serde(rename = "openai", alias = "open_ai")]
    OpenAi,
    #[serde(rename = "openai_compatible", alias = "open_ai_compatible")]
    OpenAiCompatible,
    Anthropic,
    AzureAi,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmProvider {
    #[serde(rename = "azure_openai", alias = "azure_open_ai")]
    AzureOpenAi,
    #[serde(rename = "openai", alias = "open_ai")]
    OpenAi,
    #[serde(rename = "azure_anthropic")]
    AzureAnthropic,
    #[serde(rename = "openai_compatible", alias = "open_ai_compatible")]
    OpenAiCompatible,
    Adk,
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
    #[serde(default = "default_local_tool_job_execution_timeout_seconds")]
    pub job_execution_timeout_seconds: u64,
    #[serde(default = "default_local_tool_job_wait_timeout_seconds")]
    pub job_wait_timeout_seconds: u64,
    #[serde(default = "default_local_tool_job_poll_interval_seconds")]
    pub job_poll_interval_seconds: u64,
    #[serde(default = "default_allowed_cli_tools")]
    pub allowed_cli_tools: Vec<String>,
    #[serde(default = "default_max_file_read_bytes")]
    pub max_file_read_bytes: u64,
    #[serde(default = "default_max_file_write_bytes")]
    pub max_file_write_bytes: u64,
    #[serde(default = "default_max_directory_entries")]
    pub max_directory_entries: usize,
    #[serde(default = "default_max_webfetch_bytes")]
    pub max_webfetch_bytes: u64,
}

impl Default for LocalToolsConfig {
    fn default() -> Self {
        Self {
            execution_timeout_seconds: default_local_tool_execution_timeout_seconds(),
            job_execution_timeout_seconds: default_local_tool_job_execution_timeout_seconds(),
            job_wait_timeout_seconds: default_local_tool_job_wait_timeout_seconds(),
            job_poll_interval_seconds: default_local_tool_job_poll_interval_seconds(),
            allowed_cli_tools: default_allowed_cli_tools(),
            max_file_read_bytes: default_max_file_read_bytes(),
            max_file_write_bytes: default_max_file_write_bytes(),
            max_directory_entries: default_max_directory_entries(),
            max_webfetch_bytes: default_max_webfetch_bytes(),
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
        config.expand_paths()?;
        config.resolve_secrets()?;
        config.validate()?;
        Ok(config)
    }

    fn expand_paths(&mut self) -> Result<()> {
        if let Some(data_dir) = &self.data_dir {
            let path_str = data_dir.to_string_lossy();
            let expanded = shellexpand::tilde(&path_str);
            self.data_dir = Some(PathBuf::from(expanded.as_ref()));
        }
        for trusted_root in &mut self.skills.trusted_project_roots {
            let path_str = trusted_root.to_string_lossy();
            let expanded = shellexpand::tilde(&path_str);
            *trusted_root = PathBuf::from(expanded.as_ref());
        }
        Ok(())
    }

    pub fn data_dir(&self) -> PathBuf {
        self.data_dir.clone().unwrap_or_else(|| {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".rusty")
        })
    }

    fn resolve_secrets(&mut self) -> Result<()> {
        if let Some(azure_openai) = &mut self.azure_openai {
            azure_openai.api_key = resolve_value(&azure_openai.api_key)?;
            azure_openai.endpoint = resolve_value(&azure_openai.endpoint)?;
        }
        if let Some(openai) = &mut self.openai {
            openai.api_key = resolve_value(&openai.api_key)?;
            openai.endpoint = resolve_value(&openai.endpoint)?;
        }
        if let Some(azure_anthropic) = &mut self.azure_anthropic {
            azure_anthropic.api_key = resolve_value(&azure_anthropic.api_key)?;
            azure_anthropic.endpoint = resolve_value(&azure_anthropic.endpoint)?;
        }
        if let Some(openai_compatible) = &mut self.openai_compatible {
            if let Some(api_key) = &mut openai_compatible.api_key
                && !api_key.trim().is_empty()
            {
                *api_key = resolve_value(api_key)?;
            }
            openai_compatible.base_url = resolve_value(&openai_compatible.base_url)?;
        }
        if let Some(adk) = &mut self.adk {
            adk.api_key = resolve_value(&adk.api_key)?;
            if let Some(endpoint) = &mut adk.endpoint
                && !endpoint.trim().is_empty()
            {
                *endpoint = resolve_value(endpoint)?;
            }
        }
        self.tool_environment.resolve_values()?;
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
        validate_openai_config("openai", self.openai.as_ref())?;
        validate_azure_anthropic_config("azure_anthropic", self.azure_anthropic.as_ref())?;
        validate_openai_compatible_config("openai_compatible", self.openai_compatible.as_ref())?;
        validate_adk_config("adk", self.adk.as_ref())?;
        self.tool_environment.validate()?;

        match self.effective_llm_provider() {
            Some(LlmProvider::AzureOpenAi) if self.azure_openai.is_none() => {
                bail!("llm_provider selects azure_openai but azure_openai is not configured");
            }
            Some(LlmProvider::OpenAi) if self.openai.is_none() => {
                bail!("llm_provider selects openai but openai is not configured");
            }
            Some(LlmProvider::AzureAnthropic) if self.azure_anthropic.is_none() => {
                bail!("llm_provider selects azure_anthropic but azure_anthropic is not configured");
            }
            Some(LlmProvider::OpenAiCompatible) if self.openai_compatible.is_none() => {
                bail!(
                    "llm_provider selects openai_compatible but openai_compatible is not configured"
                );
            }
            Some(LlmProvider::Adk) if self.adk.is_none() => {
                bail!("llm_provider selects adk but adk is not configured");
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
        if self.openai.is_some() {
            return Some(LlmProvider::OpenAi);
        }
        if self.azure_anthropic.is_some() {
            return Some(LlmProvider::AzureAnthropic);
        }
        if self.openai_compatible.is_some() {
            return Some(LlmProvider::OpenAiCompatible);
        }
        if self.adk.is_some() {
            return Some(LlmProvider::Adk);
        }
        None
    }

    pub fn effective_max_advertised_tools(&self) -> usize {
        match self.effective_llm_provider() {
            Some(LlmProvider::AzureOpenAi) => self
                .azure_openai
                .as_ref()
                .map(|config| config.max_advertised_tools),
            Some(LlmProvider::OpenAi) => self
                .openai
                .as_ref()
                .map(|config| config.max_advertised_tools),
            Some(LlmProvider::AzureAnthropic) => self
                .azure_anthropic
                .as_ref()
                .map(|config| config.max_advertised_tools),
            Some(LlmProvider::OpenAiCompatible) => self
                .openai_compatible
                .as_ref()
                .map(|config| config.max_advertised_tools),
            Some(LlmProvider::Adk) => self.adk.as_ref().map(|config| config.max_advertised_tools),
            None => None,
        }
        .unwrap_or(DEFAULT_MAX_ADVERTISED_TOOLS)
    }

    pub fn effective_agent_max_iterations(&self) -> usize {
        self.agent.max_iterations_per_turn.max(1)
    }

    pub fn effective_continuation_increment(&self) -> usize {
        self.agent.continuation_increment.max(1)
    }

    pub fn effective_agent_max_total_iterations(&self) -> usize {
        self.agent
            .max_total_iterations_per_turn
            .max(self.effective_agent_max_iterations())
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

fn validate_openai_config(label: &str, config: Option<&OpenAiConfig>) -> Result<()> {
    let Some(config) = config else {
        return Ok(());
    };
    if config.api_key.trim().is_empty() {
        bail!("{label}.api_key must not be empty");
    }
    if config.endpoint.trim().is_empty() {
        bail!("{label}.endpoint must not be empty");
    }
    if config.model.trim().is_empty() {
        bail!("{label}.model must not be empty");
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
    validate_optional_cost_rate(
        label,
        "input_cost_per_million_tokens",
        config.input_cost_per_million_tokens,
    )?;
    validate_optional_cost_rate(
        label,
        "output_cost_per_million_tokens",
        config.output_cost_per_million_tokens,
    )?;
    Ok(())
}

fn validate_optional_cost_rate(label: &str, field: &str, value: Option<f64>) -> Result<()> {
    if let Some(value) = value
        && (!value.is_finite() || value < 0.0)
    {
        bail!("{label}.{field} must be a finite non-negative number");
    }
    Ok(())
}

fn validate_openai_compatible_config(
    label: &str,
    config: Option<&OpenAiCompatibleConfig>,
) -> Result<()> {
    let Some(config) = config else {
        return Ok(());
    };
    if config.base_url.trim().is_empty() {
        bail!("{label}.base_url must not be empty");
    }
    if config.model.trim().is_empty() {
        bail!("{label}.model must not be empty");
    }
    Ok(())
}

fn validate_adk_config(label: &str, config: Option<&AdkConfig>) -> Result<()> {
    let Some(config) = config else {
        return Ok(());
    };
    if config.api_key.trim().is_empty() {
        bail!("{label}.api_key must not be empty");
    }
    if config.model.trim().is_empty() {
        bail!("{label}.model must not be empty");
    }
    if matches!(
        config.provider,
        AdkProvider::OpenAiCompatible | AdkProvider::AzureAi
    ) && config
        .endpoint
        .as_deref()
        .is_none_or(|endpoint| endpoint.trim().is_empty())
    {
        bail!(
            "{label}.endpoint must not be empty when adk.provider is openai_compatible or azure_ai"
        );
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

fn validate_env_name(label: &str, name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("{label} names must not be empty");
    }
    if name.contains('=') || name.contains('\0') {
        bail!("{label} name '{name}' must not contain '=' or NUL");
    }
    Ok(())
}

fn normalize_path_for_compare(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn default_allowed_cli_tools() -> Vec<String> {
    [
        "nmap",
        "vt",
        "dig",
        "whois",
        "nslookup",
        "curl",
        "wafw00f",
        "testssl.sh",
        "httpx",
        "subfinder",
        "dnsx",
        "naabu",
        "nuclei",
        "katana",
        "ffuf",
        "feroxbuster",
        "dalfox",
        "wpscan",
        "wscat",
        "websocat",
        "unzip",
        "rg",
        "python3",
        "arjun",
        "parameth",
        "gau",
        "waybackurls",
        "hakrawler",
        "subjs",
        "gospider",
        "puredns",
        "getallurls",
    ]
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

const fn default_max_advertised_tools() -> usize {
    DEFAULT_MAX_ADVERTISED_TOOLS
}

const fn default_max_agent_iterations() -> usize {
    DEFAULT_MAX_AGENT_ITERATIONS
}

const fn default_continuation_increment() -> usize {
    DEFAULT_CONTINUATION_INCREMENT
}

const fn default_max_total_agent_iterations() -> usize {
    DEFAULT_MAX_TOTAL_AGENT_ITERATIONS
}

fn default_anthropic_version() -> String {
    "2023-06-01".to_string()
}

fn default_openai_endpoint() -> String {
    "https://api.openai.com/v1".to_string()
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

const fn default_local_tool_job_execution_timeout_seconds() -> u64 {
    1200
}

const fn default_local_tool_job_wait_timeout_seconds() -> u64 {
    900
}

const fn default_local_tool_job_poll_interval_seconds() -> u64 {
    5
}

const fn default_max_file_read_bytes() -> u64 {
    16_384
}

const fn default_max_file_write_bytes() -> u64 {
    1_048_576
}

const fn default_max_directory_entries() -> usize {
    1_000
}

const fn default_max_webfetch_bytes() -> u64 {
    262_144
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
    use std::{fs, path::PathBuf};

    use tempfile::tempdir;

    use crate::types::{FilesystemAccess, FilesystemScope};

    use super::{AdkProvider, AppConfig, AzureAnthropicConfig, LlmProvider, ProjectSkillsPolicy};

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
        assert_eq!(config.local_tools.job_execution_timeout_seconds, 1200);
        assert_eq!(config.local_tools.job_wait_timeout_seconds, 900);
        assert_eq!(config.local_tools.job_poll_interval_seconds, 5);
        assert_eq!(config.local_tools.max_file_read_bytes, 16_384);
        assert_eq!(config.local_tools.max_file_write_bytes, 1_048_576);
        assert_eq!(config.local_tools.max_directory_entries, 1_000);
        assert_eq!(config.local_tools.max_webfetch_bytes, 262_144);
        assert_eq!(config.effective_agent_max_iterations(), 10);
        assert_eq!(config.effective_continuation_increment(), 10);
        assert_eq!(config.effective_agent_max_total_iterations(), 50);
        assert!(config.tool_environment.pass_through.is_empty());
        assert!(config.tool_environment.variables.is_empty());
        assert!(config.tool_environment.path_prepend.is_empty());
        assert_eq!(config.effective_max_advertised_tools(), 128);
        assert_eq!(
            config.skills.project_skills,
            ProjectSkillsPolicy::TrustedOnly
        );
    }

    #[test]
    fn resolves_tool_environment_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        unsafe {
            std::env::set_var("RUSTY_BIDULE_TEST_TOOL_PASS", "pass-value");
            std::env::set_var("RUSTY_BIDULE_TEST_TOOL_ENV_VALUE", "resolved-value");
        }
        fs::write(
            &path,
            r#"
azure_openai:
  api_key: test
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/
  deployment: gpt-4.1
tool_environment:
  pass_through:
    - RUSTY_BIDULE_TEST_TOOL_PASS
  variables:
    RUSTY_BIDULE_TEST_DIRECT: direct-value
    RUSTY_BIDULE_TEST_FROM_ENV: env:RUSTY_BIDULE_TEST_TOOL_ENV_VALUE
    RUSTY_BIDULE_TEST_EMPTY_LITERAL: ""
  path_prepend:
    - /opt/rusty-bidule/bin
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();

        assert_eq!(
            config.tool_environment.pass_through,
            vec!["RUSTY_BIDULE_TEST_TOOL_PASS"]
        );
        assert_eq!(
            config
                .tool_environment
                .variables
                .get("RUSTY_BIDULE_TEST_DIRECT")
                .map(String::as_str),
            Some("direct-value")
        );
        assert_eq!(
            config
                .tool_environment
                .variables
                .get("RUSTY_BIDULE_TEST_FROM_ENV")
                .map(String::as_str),
            Some("resolved-value")
        );
        assert_eq!(
            config
                .tool_environment
                .variables
                .get("RUSTY_BIDULE_TEST_EMPTY_LITERAL")
                .map(String::as_str),
            Some("")
        );
        assert_eq!(
            config.tool_environment.path_prepend,
            vec![PathBuf::from("/opt/rusty-bidule/bin")]
        );
    }

    #[test]
    fn rejects_missing_tool_environment_pass_through() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        unsafe {
            std::env::remove_var("RUSTY_BIDULE_TEST_MISSING_TOOL_PASS");
        }
        fs::write(
            &path,
            r#"
azure_openai:
  api_key: test
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/
  deployment: gpt-4.1
tool_environment:
  pass_through:
    - RUSTY_BIDULE_TEST_MISSING_TOOL_PASS
"#,
        )
        .unwrap();

        let err = AppConfig::load(&path).unwrap_err();

        assert!(format!("{err:#}").contains(
            "tool_environment.pass_through variable RUSTY_BIDULE_TEST_MISSING_TOOL_PASS is not set"
        ));
    }

    #[test]
    fn rejects_invalid_tool_environment_variable_names() {
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
tool_environment:
  variables:
    "BAD=NAME": value
"#,
        )
        .unwrap();

        let err = AppConfig::load(&path).unwrap_err();

        assert!(format!("{err:#}").contains("must not contain '=' or NUL"));
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
  input_cost_per_million_tokens: 0.3
  output_cost_per_million_tokens: 15.0
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
            config
                .azure_anthropic
                .as_ref()
                .and_then(|cfg| cfg.input_cost_per_million_tokens),
            Some(0.3)
        );
        assert_eq!(
            config
                .azure_anthropic
                .as_ref()
                .and_then(|cfg| cfg.output_cost_per_million_tokens),
            Some(15.0)
        );
        assert_eq!(
            config.effective_llm_provider(),
            Some(LlmProvider::AzureAnthropic)
        );
    }

    #[test]
    fn resolves_env_backed_openai_compatible_secret() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        unsafe {
            std::env::set_var("TEST_OAI_COMPAT_KEY", "proxy-secret");
        }
        fs::write(
            &path,
            r#"
openai_compatible:
  api_key: env:TEST_OAI_COMPAT_KEY
  base_url: http://127.0.0.1:4000/v1
  model: gpt-5
  max_advertised_tools: 64
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert_eq!(
            config
                .openai_compatible
                .as_ref()
                .and_then(|cfg| cfg.api_key.as_deref()),
            Some("proxy-secret")
        );
        assert_eq!(
            config.effective_llm_provider(),
            Some(LlmProvider::OpenAiCompatible)
        );
        assert_eq!(config.effective_max_advertised_tools(), 64);
    }

    #[test]
    fn resolves_env_backed_openai_secret_and_defaults_endpoint() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        unsafe {
            std::env::set_var("TEST_OPENAI_KEY", "openai-secret");
        }
        fs::write(
            &path,
            r#"
openai:
  api_key: env:TEST_OPENAI_KEY
  model: gpt-5
  max_advertised_tools: 32
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        let openai = config.openai.as_ref().unwrap();
        assert_eq!(openai.api_key, "openai-secret");
        assert_eq!(openai.endpoint, "https://api.openai.com/v1");
        assert_eq!(config.effective_llm_provider(), Some(LlmProvider::OpenAi));
        assert_eq!(config.effective_max_advertised_tools(), 32);
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
    fn rejects_negative_azure_anthropic_cost_rates() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
azure_anthropic:
  api_key: test
  endpoint: https://example.invalid/anthropic/
  deployment: claude-opus-4-6
  input_cost_per_million_tokens: -0.1
"#,
        )
        .unwrap();

        let err = AppConfig::load(&path).unwrap_err();

        assert!(format!("{err:#}").contains(
            "azure_anthropic.input_cost_per_million_tokens must be a finite non-negative number"
        ));
    }

    #[test]
    fn parses_llm_provider_azure_openai_without_extra_underscore() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
llm_provider: azure_openai
azure_openai:
  api_key: test
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/
  deployment: gpt-4.1
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
    fn defaults_to_azure_openai_when_both_azure_providers_are_configured_and_selector_is_omitted() {
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
    fn defaults_to_openai_compatible_when_it_is_the_only_configured_provider() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
openai_compatible:
  base_url: http://127.0.0.1:4000/v1
  model: gpt-5
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert_eq!(
            config.effective_llm_provider(),
            Some(LlmProvider::OpenAiCompatible)
        );
    }

    #[test]
    fn defaults_to_openai_when_it_is_the_only_configured_provider() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
openai:
  api_key: test
  model: gpt-5
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert_eq!(config.effective_llm_provider(), Some(LlmProvider::OpenAi));
    }

    #[test]
    fn parses_selected_adk_provider() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
llm_provider: adk
adk:
  provider: azure_ai
  api_key: test
  endpoint: https://example.invalid
  model: claude-opus-4-6
  max_advertised_tools: 32
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert_eq!(config.effective_llm_provider(), Some(LlmProvider::Adk));
        assert_eq!(
            config.adk.as_ref().map(|adk| adk.provider),
            Some(AdkProvider::AzureAi)
        );
        assert_eq!(config.effective_max_advertised_tools(), 32);
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
    fn rejects_missing_selected_openai_compatible_block() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
llm_provider: openai_compatible
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
            "llm_provider selects openai_compatible but openai_compatible is not configured"
        ));
    }

    #[test]
    fn rejects_missing_selected_adk_block() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
llm_provider: adk
azure_openai:
  api_key: test
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/
  deployment: gpt-4.1
"#,
        )
        .unwrap();

        let err = AppConfig::load(&path).unwrap_err();
        assert!(format!("{err:#}").contains("llm_provider selects adk but adk is not configured"));
    }

    #[test]
    fn rejects_adk_azure_ai_without_endpoint() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
llm_provider: adk
adk:
  provider: azure_ai
  api_key: test
  model: claude-opus-4-6
"#,
        )
        .unwrap();

        let err = AppConfig::load(&path).unwrap_err();
        assert!(format!("{err:#}").contains("adk.endpoint must not be empty"));
    }

    #[test]
    fn rejects_missing_selected_openai_block() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
llm_provider: openai
azure_openai:
  api_key: test
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/
  deployment: gpt-4.1
"#,
        )
        .unwrap();

        let err = AppConfig::load(&path).unwrap_err();
        assert!(
            format!("{err:#}").contains("llm_provider selects openai but openai is not configured")
        );
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
  job_execution_timeout_seconds: 1200
  job_wait_timeout_seconds: 900
  job_poll_interval_seconds: 5
  max_webfetch_bytes: 4096
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert_eq!(config.local_tools.execution_timeout_seconds, 240);
        assert_eq!(config.local_tools.job_execution_timeout_seconds, 1200);
        assert_eq!(config.local_tools.job_wait_timeout_seconds, 900);
        assert_eq!(config.local_tools.job_poll_interval_seconds, 5);
        assert_eq!(config.local_tools.max_webfetch_bytes, 4096);
        assert!(
            config
                .local_tools
                .allowed_cli_tools
                .contains(&"nmap".to_string())
        );
        assert!(
            config
                .local_tools
                .allowed_cli_tools
                .contains(&"nuclei".to_string())
        );
        assert!(
            config
                .local_tools
                .allowed_cli_tools
                .contains(&"websocat".to_string())
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
    fn parses_skill_trust_policy() {
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
skills:
  project_skills: always
  trusted_project_roots:
    - /tmp/trusted-project
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();

        assert_eq!(config.skills.project_skills, ProjectSkillsPolicy::Always);
        assert_eq!(
            config.skills.trusted_project_roots,
            vec![PathBuf::from("/tmp/trusted-project")]
        );
        assert!(config.skills.allows_project_skill_dirs(dir.path()));
    }

    #[test]
    fn trusted_only_skill_policy_requires_matching_project_root() {
        let dir = tempdir().unwrap();
        let other = tempdir().unwrap();
        let config = super::SkillsConfig {
            project_skills: ProjectSkillsPolicy::TrustedOnly,
            trusted_project_roots: vec![dir.path().to_path_buf()],
        };

        assert!(config.allows_project_skill_dirs(dir.path()));
        assert!(!config.allows_project_skill_dirs(other.path()));
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
  filesystem_scope: full
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
        assert_eq!(
            config.agent_permissions.filesystem_scope,
            FilesystemScope::Full
        );
        assert!(!config.agent_permissions.yolo);
    }

    #[test]
    fn parses_agent_iteration_budget_block() {
        let config: AppConfig = serde_yaml::from_str(
            r#"
agent:
  max_iterations_per_turn: 14
  continuation_increment: 6
  max_total_iterations_per_turn: 40
"#,
        )
        .unwrap();

        assert_eq!(config.effective_agent_max_iterations(), 14);
        assert_eq!(config.effective_continuation_increment(), 6);
        assert_eq!(config.effective_agent_max_total_iterations(), 40);
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

    #[test]
    fn expands_tilde_in_data_dir() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            r#"
data_dir: ~/test-data
azure_openai:
  api_key: test
  api_version: 2025-03-01-preview
  endpoint: https://example.invalid/
  deployment: gpt-4.1
"#,
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        let data_dir = config.data_dir();
        assert!(!data_dir.to_string_lossy().contains('~'));
        assert!(data_dir.is_absolute() || data_dir.starts_with(std::env::var("HOME").unwrap()));
    }

    #[test]
    fn defaults_to_home_rusty_when_data_dir_omitted() {
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
        let data_dir = config.data_dir();
        assert!(data_dir.ends_with(".rusty"));
    }
}
