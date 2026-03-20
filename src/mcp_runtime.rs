use std::{collections::HashMap, path::Path, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::{
    Client,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde_json::{Value, json};
use tracing::{debug, info, warn};

use crate::{
    config::{McpRuntimeConfig, McpServerConfig},
    oauth::OAuthProvider,
};

#[derive(Debug, Clone)]
pub struct McpTool {
    pub server_name: String,
    pub original_name: String,
    pub external_name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug)]
pub struct McpManager {
    client: Client,
    oauth: OAuthProvider,
    runtime: McpRuntimeConfig,
    servers: Vec<ServerState>,
    tool_index: HashMap<String, (usize, String)>,
    next_id: u64,
}

#[derive(Debug)]
struct ServerState {
    config: McpServerConfig,
    session_id: Option<String>,
    initialized: bool,
}

impl McpManager {
    pub fn new(
        data_dir: impl AsRef<Path>,
        runtime: McpRuntimeConfig,
        servers: Vec<McpServerConfig>,
    ) -> Result<Self> {
        let client = Client::builder()
            .user_agent(format!("rusty-bidule/{}", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(runtime.connect_timeout_seconds))
            .build()
            .context("failed to build HTTP client")?;
        let oauth = OAuthProvider::new(data_dir)?;
        info!(server_count = servers.len(), "initialized MCP runtime");

        Ok(Self {
            client,
            oauth,
            runtime,
            servers: servers
                .into_iter()
                .map(|config| ServerState {
                    config,
                    session_id: None,
                    initialized: false,
                })
                .collect(),
            tool_index: HashMap::new(),
            next_id: 1,
        })
    }

    pub async fn list_tools(&mut self) -> Result<Vec<McpTool>> {
        debug!("listing MCP tools across configured servers");
        let mut all_tools = Vec::new();
        self.tool_index.clear();
        let mut last_error = None;

        for index in 0..self.servers.len() {
            let timeout_seconds = self.server_session_timeout(index);
            let list_future = self.list_server_tools(index);
            match tokio::time::timeout(Duration::from_secs(timeout_seconds), list_future).await {
                Ok(Ok(tools)) => {
                    for tool in tools {
                        self.tool_index.insert(
                            tool.external_name.clone(),
                            (index, tool.original_name.clone()),
                        );
                        all_tools.push(tool);
                    }
                }
                Ok(Err(err)) => last_error = Some(err),
                Err(_) => {
                    warn!(server = %self.servers[index].config.name, "timed out while listing tools");
                    last_error = Some(anyhow!(
                        "timed out while listing tools from server '{}'",
                        self.servers[index].config.name
                    ));
                }
            }
        }

        if all_tools.is_empty() {
            if let Some(err) = last_error {
                bail!("no MCP tools available: {err}");
            }
        }

        Ok(all_tools)
    }

    pub async fn login_server(&mut self, server_name: &str) -> Result<()> {
        let index = self
            .servers
            .iter()
            .position(|server| server.config.name == server_name)
            .ok_or_else(|| anyhow!("unknown MCP server '{server_name}'"))?;

        info!(server = server_name, "starting explicit MCP login");
        let server = &self.servers[index];
        let auth = self.oauth.authorize_server(&server.config).await?;
        if auth.is_none() {
            bail!("server '{server_name}' is not configured for OAuth login");
        }

        info!(server = server_name, "MCP login completed");
        Ok(())
    }

    pub async fn call_tool(&mut self, external_name: &str, arguments: Value) -> Result<String> {
        if !self.tool_index.contains_key(external_name) {
            self.list_tools().await?;
        }
        let (server_index, original_name) = self
            .tool_index
            .get(external_name)
            .cloned()
            .ok_or_else(|| anyhow!("unknown MCP tool {external_name}"))?;

        self.ensure_initialized(server_index).await?;
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.next_request_id(),
            "method": "tools/call",
            "params": {
                "name": original_name,
                "arguments": arguments,
            }
        });
        let result = self.post_jsonrpc(server_index, &request).await?;
        Ok(normalize_tool_result(&result))
    }

    async fn list_server_tools(&mut self, server_index: usize) -> Result<Vec<McpTool>> {
        self.ensure_initialized(server_index).await?;
        debug!(server = %self.servers[server_index].config.name, "requesting tools/list");
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.next_request_id(),
            "method": "tools/list",
            "params": {}
        });
        let result = self.post_jsonrpc(server_index, &request).await?;
        let tools = result
            .get("tools")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("tools/list did not return a tools array"))?;

        let server_name = self.servers[server_index].config.name.clone();
        let mut mapped = Vec::new();
        for tool in tools {
            let original_name = tool
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("tool entry missing name"))?
                .to_string();
            let external_name = format!(
                "{}__{}",
                sanitize_name(&server_name),
                sanitize_name(&original_name)
            );
            mapped.push(McpTool {
                server_name: server_name.clone(),
                original_name,
                external_name,
                description: tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("MCP tool")
                    .to_string(),
                input_schema: tool
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
            });
        }

        info!(server = %server_name, tool_count = mapped.len(), "listed MCP tools");
        Ok(mapped)
    }

    async fn ensure_initialized(&mut self, server_index: usize) -> Result<()> {
        if self.servers[server_index].initialized {
            return Ok(());
        }
        info!(server = %self.servers[server_index].config.name, "initializing MCP server session");

        let request = json!({
            "jsonrpc": "2.0",
            "id": self.next_request_id(),
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {
                    "name": "rusty-bidule",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }
        });
        self.post_jsonrpc(server_index, &request).await?;

        let notify = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        });
        self.post_notification(server_index, &notify).await?;
        self.servers[server_index].initialized = true;
        info!(server = %self.servers[server_index].config.name, "MCP server initialized");
        Ok(())
    }

    async fn post_jsonrpc(&mut self, server_index: usize, body: &Value) -> Result<Value> {
        let (headers, response_body) = self.post(server_index, body).await?;
        self.capture_session_id(server_index, &headers);
        if response_body.is_null() {
            bail!("server returned an empty JSON-RPC response");
        }
        if let Some(error) = response_body.get("error") {
            bail!("MCP error: {}", serde_json::to_string_pretty(error)?);
        }
        response_body
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow!("JSON-RPC response missing result"))
    }

    async fn post_notification(&mut self, server_index: usize, body: &Value) -> Result<()> {
        let (headers, _) = self.post(server_index, body).await?;
        self.capture_session_id(server_index, &headers);
        Ok(())
    }

    async fn post(&self, server_index: usize, body: &Value) -> Result<(HeaderMap, Value)> {
        let server = &self.servers[server_index];
        let auth = self.oauth.authorize_server(&server.config).await?;
        debug!(
            server = %server.config.name,
            authenticated = auth.is_some(),
            has_session = server.session_id.is_some(),
            "issuing MCP POST request"
        );
        let mut request = self
            .client
            .post(&server.config.url)
            .headers(build_headers(
                &server.config,
                server.session_id.as_deref(),
                auth.as_ref().map(|token| token.access_token.as_str()),
            )?)
            .json(body);

        request = request.timeout(Duration::from_secs(
            self.server_session_timeout(server_index),
        ));

        let response = request.send().await.with_context(|| {
            format!(
                "failed to reach MCP server '{}' at {}",
                server.config.name, server.config.url
            )
        })?;

        let status = response.status();
        let headers = response.headers().clone();
        let text = response.text().await?;
        if !status.is_success() {
            warn!(server = %server.config.name, %status, "MCP server returned non-success status");
            bail!(
                "MCP server '{}' returned HTTP {}: {}",
                server.config.name,
                status,
                text
            );
        }
        let value = if text.trim().is_empty() {
            Value::Null
        } else {
            parse_mcp_response_body(&text)
                .with_context(|| format!("failed to parse MCP response body: {text}"))?
        };
        debug!(server = %server.config.name, %status, "parsed MCP response body");
        Ok((headers, value))
    }

    fn server_session_timeout(&self, server_index: usize) -> u64 {
        let server = &self.servers[server_index].config;
        server
            .client_session_timeout_seconds
            .or(server.timeout)
            .or(server.sse_read_timeout)
            .unwrap_or(self.runtime.connect_timeout_seconds)
    }

    fn capture_session_id(&mut self, server_index: usize, headers: &HeaderMap) {
        let session_id = headers
            .get("Mcp-Session-Id")
            .or_else(|| headers.get("MCP-Session-Id"))
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        if let Some(session_id) = session_id {
            self.servers[server_index].session_id = Some(session_id);
            debug!(server = %self.servers[server_index].config.name, "captured MCP session id");
        }
    }

    fn next_request_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

fn build_headers(
    config: &McpServerConfig,
    session_id: Option<&str>,
    bearer_token: Option<&str>,
) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        reqwest::header::ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    for (name, value) in &config.headers {
        let header_name = HeaderName::from_bytes(name.as_bytes())?;
        let header_value = HeaderValue::from_str(value)?;
        headers.insert(header_name, header_value);
    }
    if let Some(bearer_token) = bearer_token {
        headers.insert(
            reqwest::header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {bearer_token}"))?,
        );
    }
    if let Some(session_id) = session_id {
        headers.insert(
            HeaderName::from_static("mcp-session-id"),
            HeaderValue::from_str(session_id)?,
        );
    }
    Ok(headers)
}

fn parse_mcp_response_body(body: &str) -> Result<Value> {
    if let Ok(value) = serde_json::from_str(body) {
        return Ok(value);
    }

    parse_sse_response_body(body)
}

fn parse_sse_response_body(body: &str) -> Result<Value> {
    let mut current_data_lines = Vec::new();
    let mut parsed_messages = Vec::new();

    for line in body.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            if let Some(value) = flush_sse_event(&mut current_data_lines)? {
                parsed_messages.push(value);
            }
            continue;
        }

        if let Some(data) = trimmed.strip_prefix("data:") {
            current_data_lines.push(data.trim_start().to_string());
        }
    }

    if let Some(value) = flush_sse_event(&mut current_data_lines)? {
        parsed_messages.push(value);
    }

    parsed_messages
        .into_iter()
        .rev()
        .find(|value| value.get("result").is_some() || value.get("error").is_some())
        .ok_or_else(|| anyhow!("response was neither JSON nor SSE-framed JSON-RPC"))
}

fn flush_sse_event(current_data_lines: &mut Vec<String>) -> Result<Option<Value>> {
    if current_data_lines.is_empty() {
        return Ok(None);
    }

    let payload = current_data_lines.join("\n");
    current_data_lines.clear();

    if payload == "[DONE]" {
        return Ok(None);
    }

    let value = serde_json::from_str(&payload)
        .with_context(|| format!("failed to parse SSE event payload: {payload}"))?;
    Ok(Some(value))
}

pub fn normalize_tool_result(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(content) = value.get("content").and_then(Value::as_array) {
        let collected: Vec<String> = content
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| {
                        if item.is_object() {
                            Some(
                                serde_json::to_string_pretty(item)
                                    .unwrap_or_else(|_| item.to_string()),
                            )
                        } else {
                            item.as_str().map(str::to_string)
                        }
                    })
            })
            .collect();
        if !collected.is_empty() {
            return collected.join("\n\n");
        }
    }
    if let Some(structured) = value.get("structuredContent") {
        return serde_json::to_string_pretty(structured).unwrap_or_else(|_| structured.to_string());
    }
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => ch,
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use reqwest::header::ACCEPT;
    use serde_json::json;

    use crate::config::McpServerConfig;

    use super::{ServerState, build_headers, normalize_tool_result, parse_mcp_response_body};

    #[test]
    fn normalizes_text_content_arrays() {
        let payload = json!({
            "content": [
                {"type": "text", "text": "alpha"},
                {"type": "text", "text": "beta"}
            ]
        });
        assert_eq!(normalize_tool_result(&payload), "alpha\n\nbeta");
    }

    #[test]
    fn streamable_http_accept_header_advertises_json_and_sse() {
        let server = ServerState {
            config: McpServerConfig {
                name: "fastmcp".to_string(),
                transport: "streamable_http".to_string(),
                url: "http://127.0.0.1:8000/mcp".to_string(),
                headers: Default::default(),
                timeout: Some(30),
                sse_read_timeout: Some(300),
                client_session_timeout_seconds: Some(30),
                auth: None,
            },
            session_id: None,
            initialized: false,
        };

        let headers = build_headers(&server.config, None, None).unwrap();
        let accept = headers.get(ACCEPT).unwrap().to_str().unwrap();

        assert!(accept.contains("application/json"));
        assert!(accept.contains("text/event-stream"));
    }

    #[test]
    fn parses_sse_wrapped_jsonrpc_result() {
        let body = concat!(
            "event: message\n",
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{\"progress\":50}}\n",
            "\n",
            "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[{\"name\":\"hello\"}]}}\n",
            "\n"
        );

        let parsed = parse_mcp_response_body(body).unwrap();
        assert_eq!(parsed["result"]["tools"][0]["name"], "hello");
    }
}
