use std::{collections::HashMap, path::Path, process::Stdio, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::{
    Client, StatusCode,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::mpsc,
    task::JoinHandle,
};
use tracing::{debug, info, warn};
use url::Url;

use crate::{
    config::{McpAuthConfig, McpRuntimeConfig, McpServerConfig, ToolEnvironmentConfig},
    oauth::OAuthProvider,
};

const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26"];
const PREFERRED_PROTOCOL_VERSION: &str = SUPPORTED_PROTOCOL_VERSIONS[0];

#[derive(Debug, Clone)]
pub struct McpTool {
    pub server_name: String,
    pub original_name: String,
    pub external_name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transport {
    StreamableHttp,
    Sse,
    Stdio,
}

#[derive(Debug)]
pub struct McpManager {
    client: Client,
    oauth: OAuthProvider,
    runtime: McpRuntimeConfig,
    tool_environment: ToolEnvironmentConfig,
    servers: Vec<ServerState>,
    tool_index: HashMap<String, (usize, String)>,
    next_id: u64,
}

#[derive(Debug)]
struct ServerState {
    config: McpServerConfig,
    transport: Transport,
    session_id: Option<String>,
    protocol_version: Option<String>,
    /// For SSE: the endpoint URL to POST requests to.
    sse_endpoint: Option<String>,
    /// For legacy SSE: JSON-RPC messages received from the server event stream.
    sse_inbox: Option<mpsc::Receiver<Value>>,
    sse_task: Option<JoinHandle<()>>,
    stdio: Option<StdioState>,
    initialized: bool,
}

#[derive(Debug)]
struct StdioState {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

#[derive(Debug)]
struct SessionExpiredError;

impl std::fmt::Display for SessionExpiredError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MCP session expired")
    }
}

impl std::error::Error for SessionExpiredError {}

#[derive(Debug)]
struct HttpStatusError {
    server_name: String,
    status: StatusCode,
    body: String,
}

impl std::fmt::Display for HttpStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "MCP server '{}' returned HTTP {}: {}",
            self.server_name, self.status, self.body
        )
    }
}

impl std::error::Error for HttpStatusError {}

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
            tool_environment: ToolEnvironmentConfig::default(),
            servers: servers
                .into_iter()
                .map(|config| {
                    let transport = match config.transport.as_str() {
                        "sse" => Transport::Sse,
                        "stdio" => Transport::Stdio,
                        _ => Transport::StreamableHttp,
                    };
                    ServerState {
                        config,
                        transport,
                        session_id: None,
                        protocol_version: None,
                        sse_endpoint: None,
                        sse_inbox: None,
                        sse_task: None,
                        stdio: None,
                        initialized: false,
                    }
                })
                .collect(),
            tool_index: HashMap::new(),
            next_id: 1,
        })
    }

    pub fn with_tool_environment(mut self, config: ToolEnvironmentConfig) -> Self {
        self.tool_environment = config;
        self
    }

    pub async fn list_tools(&mut self) -> Result<Vec<McpTool>> {
        self.list_tools_filtered(None).await
    }

    pub async fn list_tools_filtered(&mut self, filter: Option<&[String]>) -> Result<Vec<McpTool>> {
        debug!("listing MCP tools across configured servers");
        let mut all_tools = Vec::new();
        self.tool_index.clear();
        let mut last_error = None;

        for index in 0..self.servers.len() {
            // Apply server filter if provided
            if let Some(allowed) = filter
                && !allowed.contains(&self.servers[index].config.name)
            {
                continue;
            }

            match self.list_server_tools(index).await {
                Ok(tools) => {
                    for tool in tools {
                        self.tool_index.insert(
                            tool.external_name.clone(),
                            (index, tool.original_name.clone()),
                        );
                        all_tools.push(tool);
                    }
                }
                Err(err) => last_error = Some(err),
            }
        }

        if all_tools.is_empty()
            && let Some(err) = last_error
        {
            bail!("no MCP tools available: {err}");
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
        let auth = self.oauth.authorize_server_forced(&server.config).await?;
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
        let request_id = self.next_request_id();
        let request = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "tools/call",
            "params": {
                "name": original_name,
                "arguments": arguments,
            }
        });
        let result = self
            .post_jsonrpc_with_timeout(server_index, &request, request_id, "tools/call")
            .await?;
        Ok(normalize_tool_result(&result))
    }

    async fn list_server_tools(&mut self, server_index: usize) -> Result<Vec<McpTool>> {
        self.ensure_initialized(server_index).await?;
        debug!(server = %self.servers[server_index].config.name, "requesting tools/list");
        let request_id = self.next_request_id();
        let request = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "tools/list",
            "params": {}
        });
        let result = self
            .post_jsonrpc_with_timeout(server_index, &request, request_id, "tools/list")
            .await?;
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

        match self.servers[server_index].transport {
            Transport::Sse => {
                self.sse_connect(server_index).await?;
            }
            Transport::Stdio => {
                self.ensure_stdio_started(server_index).await?;
            }
            Transport::StreamableHttp => {}
        }

        let request = json!({
            "jsonrpc": "2.0",
            "id": self.next_request_id(),
            "method": "initialize",
            "params": {
                "protocolVersion": PREFERRED_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "rusty-bidule",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }
        });
        let result = match self.post_jsonrpc(server_index, &request).await {
            Ok(result) => result,
            Err(err)
                if self.servers[server_index].transport == Transport::StreamableHttp
                    && should_fallback_to_legacy_sse(&err) =>
            {
                warn!(
                    server = %self.servers[server_index].config.name,
                    error = %err,
                    "streamable HTTP initialize failed; trying legacy SSE transport"
                );
                self.reset_server_session(server_index);
                self.servers[server_index].transport = Transport::Sse;
                self.sse_connect(server_index).await?;
                self.post_jsonrpc(server_index, &request).await?
            }
            Err(err) => return Err(err),
        };
        let negotiated = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("initialize result missing protocolVersion"))?;
        if !SUPPORTED_PROTOCOL_VERSIONS.contains(&negotiated) {
            self.reset_server_session(server_index);
            bail!(
                "server '{}' negotiated unsupported MCP protocol version '{}'",
                self.servers[server_index].config.name,
                negotiated
            );
        }
        self.servers[server_index].protocol_version = Some(negotiated.to_string());

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

    async fn ensure_stdio_started(&mut self, server_index: usize) -> Result<()> {
        if self.servers[server_index].stdio.is_some() {
            return Ok(());
        }

        let server = self.servers[server_index].config.clone();
        let command = server
            .command
            .as_deref()
            .ok_or_else(|| anyhow!("server '{}' is missing a stdio command", server.name))?;
        info!(server = %server.name, command, "starting MCP stdio server");

        let mut cmd = Command::new(command);
        cmd.args(&server.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        self.tool_environment.apply_to_command(&mut cmd)?;

        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn MCP stdio server '{}'", server.name))?;
        let stdin = child.stdin.take().ok_or_else(|| {
            anyhow!(
                "failed to capture stdin for MCP stdio server '{}'",
                server.name
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            anyhow!(
                "failed to capture stdout for MCP stdio server '{}'",
                server.name
            )
        })?;

        self.servers[server_index].stdio = Some(StdioState {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        });
        Ok(())
    }

    /// Connect to an SSE endpoint and parse the initial `endpoint` event.
    async fn sse_connect(&mut self, server_index: usize) -> Result<()> {
        let server = &self.servers[server_index];
        let auth = self.oauth.authorize_server(&server.config).await?;
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            HeaderValue::from_static("text/event-stream"),
        );
        if let Some(token) = auth.as_ref() {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", token.access_token))?,
            );
        }
        for (name, value) in &server.config.headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())?;
            let header_value = HeaderValue::from_str(value)?;
            headers.insert(header_name, header_value);
        }

        let url = server.config.url.clone();
        let timeout_secs = self.server_session_timeout(server_index);

        let response = tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            self.client.get(&url).headers(headers).send(),
        )
        .await
        .with_context(|| format!("timed out connecting to SSE endpoint {url}"))?
        .with_context(|| format!("failed to connect to SSE endpoint {url}"))?;

        let status = response.status();
        if !status.is_success() {
            bail!("SSE endpoint returned HTTP {status}");
        }

        let (tx, rx) = mpsc::channel(128);
        let mut response = response;
        let mut parser = SseEventParser::default();
        let endpoint_url = tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            read_sse_endpoint_event(&mut response, &mut parser, &tx),
        )
        .await
        .with_context(|| format!("timed out waiting for SSE endpoint event from {url}"))??;
        let endpoint_url = resolve_sse_endpoint_url(&url, &endpoint_url)?;
        let server_name = self.servers[server_index].config.name.clone();
        let sse_task = tokio::spawn(read_sse_messages(server_name.clone(), response, parser, tx));

        debug!(server = %server_name, endpoint = %endpoint_url, "SSE endpoint discovered");
        self.servers[server_index].sse_endpoint = Some(endpoint_url);
        self.servers[server_index].sse_inbox = Some(rx);
        self.servers[server_index].sse_task = Some(sse_task);
        Ok(())
    }

    async fn post_jsonrpc_with_reinit(
        &mut self,
        server_index: usize,
        body: &Value,
    ) -> Result<Value> {
        match self.post_jsonrpc(server_index, body).await {
            Ok(result) => Ok(result),
            Err(err)
                if err.downcast_ref::<SessionExpiredError>().is_some()
                    && self.servers[server_index].transport == Transport::StreamableHttp =>
            {
                warn!(
                    server = %self.servers[server_index].config.name,
                    "MCP session expired; reinitializing and retrying request"
                );
                self.reset_server_session(server_index);
                self.ensure_initialized(server_index).await?;
                self.post_jsonrpc(server_index, body).await
            }
            Err(err) => Err(err),
        }
    }

    async fn post_jsonrpc_with_timeout(
        &mut self,
        server_index: usize,
        body: &Value,
        request_id: u64,
        method: &str,
    ) -> Result<Value> {
        let timeout_seconds = self.server_session_timeout(server_index);
        match tokio::time::timeout(
            Duration::from_secs(timeout_seconds),
            self.post_jsonrpc_with_reinit(server_index, body),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                warn!(
                    server = %self.servers[server_index].config.name,
                    request_id,
                    method,
                    timeout_seconds,
                    "timed out waiting for MCP request"
                );
                self.post_cancelled_notification(server_index, request_id, "client timeout")
                    .await;
                Err(anyhow!(
                    "timed out while waiting for {} from server '{}'",
                    method,
                    self.servers[server_index].config.name
                ))
            }
        }
    }

    async fn post_jsonrpc(&mut self, server_index: usize, body: &Value) -> Result<Value> {
        let response_body = match self.servers[server_index].transport {
            Transport::Stdio => {
                let (_, response_body) = self.post_stdio(server_index, body).await?;
                response_body
            }
            Transport::Sse => self.post_sse(server_index, body).await?,
            Transport::StreamableHttp => {
                let (headers, response_body) = self.post(server_index, body).await?;
                self.capture_session_id(server_index, &headers);
                response_body
            }
        };
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
        match self.servers[server_index].transport {
            Transport::Stdio => {
                self.send_stdio_message(server_index, body).await?;
            }
            Transport::Sse => {
                self.post_sse(server_index, body).await?;
            }
            Transport::StreamableHttp => {
                let (headers, _) = self.post(server_index, body).await?;
                self.capture_session_id(server_index, &headers);
            }
        }
        Ok(())
    }

    async fn post_cancelled_notification(
        &mut self,
        server_index: usize,
        request_id: u64,
        reason: &str,
    ) {
        let body = json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {
                "requestId": request_id,
                "reason": reason,
            }
        });
        if let Err(err) = self.post_notification(server_index, &body).await {
            warn!(
                server = %self.servers[server_index].config.name,
                request_id,
                error = %err,
                "failed to send MCP cancelled notification after timeout"
            );
        }
    }

    async fn post(&self, server_index: usize, body: &Value) -> Result<(HeaderMap, Value)> {
        let server = &self.servers[server_index];
        if server.transport == Transport::Stdio {
            bail!(
                "internal error: stdio transport requires mutable access for server '{}'",
                server.config.name
            );
        }
        let auth = self.oauth.authorize_server(&server.config).await?;

        // For SSE transport use the discovered endpoint URL
        let url = if server.transport == Transport::Sse {
            server
                .sse_endpoint
                .as_deref()
                .unwrap_or(&server.config.url)
                .to_string()
        } else {
            server.config.url.clone()
        };

        debug!(
            server = %server.config.name,
            authenticated = auth.is_some(),
            has_session = server.session_id.is_some(),
            transport = ?server.transport,
            "issuing MCP POST request"
        );
        let request = self
            .client
            .post(&url)
            .headers(build_headers(
                &server.config,
                server.session_id.as_deref(),
                server.protocol_version.as_deref(),
                auth.as_ref().map(|token| token.access_token.as_str()),
            )?)
            .json(body);

        let response = request.send().await.with_context(|| {
            format!(
                "failed to reach MCP server '{}' at {}",
                server.config.name, url
            )
        })?;

        let status = response.status();
        let headers = response.headers().clone();
        let text = response.text().await?;
        if !status.is_success() {
            if status == StatusCode::NOT_FOUND
                && server.transport == Transport::StreamableHttp
                && server.session_id.is_some()
            {
                warn!(
                    server = %server.config.name,
                    "streamable HTTP session appears expired"
                );
                return Err(SessionExpiredError.into());
            }
            warn!(server = %server.config.name, %status, "MCP server returned non-success status");
            return Err(HttpStatusError {
                server_name: server.config.name.clone(),
                status,
                body: text,
            }
            .into());
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

    async fn post_sse(&mut self, server_index: usize, body: &Value) -> Result<Value> {
        let expected_id = body.get("id").cloned();
        let (headers, response_body) = self.post_sse_http(server_index, body).await?;
        self.capture_session_id(server_index, &headers);

        if let Some(response_body) = response_body {
            return Ok(response_body);
        }

        if let Some(expected_id) = expected_id {
            return self.read_sse_response(server_index, &expected_id).await;
        }

        Ok(Value::Null)
    }

    async fn post_sse_http(
        &self,
        server_index: usize,
        body: &Value,
    ) -> Result<(HeaderMap, Option<Value>)> {
        let server = &self.servers[server_index];
        let url = server.sse_endpoint.as_deref().ok_or_else(|| {
            anyhow!(
                "SSE endpoint for server '{}' is not connected",
                server.config.name
            )
        })?;
        let auth = self.oauth.authorize_server(&server.config).await?;

        debug!(
            server = %server.config.name,
            authenticated = auth.is_some(),
            transport = ?server.transport,
            endpoint = %url,
            "issuing MCP SSE POST request"
        );
        let response = self
            .client
            .post(url)
            .headers(build_headers(
                &server.config,
                server.session_id.as_deref(),
                server.protocol_version.as_deref(),
                auth.as_ref().map(|token| token.access_token.as_str()),
            )?)
            .json(body)
            .send()
            .await
            .with_context(|| {
                format!(
                    "failed to reach MCP SSE server '{}' at {}",
                    server.config.name, url
                )
            })?;

        let status = response.status();
        let headers = response.headers().clone();
        let text = response.text().await?;
        if !status.is_success() {
            warn!(server = %server.config.name, %status, "MCP SSE server returned non-success status");
            return Err(HttpStatusError {
                server_name: server.config.name.clone(),
                status,
                body: text,
            }
            .into());
        }

        if status == StatusCode::ACCEPTED || text.trim().is_empty() {
            return Ok((headers, None));
        }

        let value = parse_mcp_response_body(&text)
            .with_context(|| format!("failed to parse MCP SSE POST response body: {text}"))?;
        Ok((headers, Some(value)))
    }

    async fn read_sse_response(
        &mut self,
        server_index: usize,
        expected_id: &Value,
    ) -> Result<Value> {
        let server_name = self.servers[server_index].config.name.clone();
        let inbox = self.servers[server_index]
            .sse_inbox
            .as_mut()
            .ok_or_else(|| anyhow!("SSE stream for server '{server_name}' is not connected"))?;

        loop {
            match inbox.recv().await {
                Some(message) if message.get("id") == Some(expected_id) => return Ok(message),
                Some(message) => {
                    debug!(
                        server = %server_name,
                        expected_id = %expected_id,
                        received = %message,
                        "ignoring non-matching SSE MCP message"
                    );
                }
                None => bail!("SSE stream for server '{server_name}' closed before response"),
            }
        }
    }

    async fn post_stdio(
        &mut self,
        server_index: usize,
        body: &Value,
    ) -> Result<(HeaderMap, Value)> {
        self.send_stdio_message(server_index, body).await?;
        let expected_id = body.get("id").cloned();
        let response = match expected_id {
            Some(expected_id) => self.read_stdio_response(server_index, &expected_id).await?,
            None => Value::Null,
        };
        Ok((HeaderMap::new(), response))
    }

    async fn send_stdio_message(&mut self, server_index: usize, body: &Value) -> Result<()> {
        self.ensure_stdio_started(server_index).await?;
        let payload =
            serde_json::to_vec(body).context("failed to serialize MCP stdio JSON-RPC payload")?;
        let frame = format!("Content-Length: {}\r\n\r\n", payload.len());
        let server_name = self.servers[server_index].config.name.clone();
        let stdio = self.servers[server_index]
            .stdio
            .as_mut()
            .ok_or_else(|| anyhow!("stdio transport for server '{server_name}' is not running"))?;
        stdio
            .stdin
            .write_all(frame.as_bytes())
            .await
            .with_context(|| format!("failed to write stdio frame header to '{server_name}'"))?;
        stdio
            .stdin
            .write_all(&payload)
            .await
            .with_context(|| format!("failed to write stdio frame body to '{server_name}'"))?;
        stdio
            .stdin
            .flush()
            .await
            .with_context(|| format!("failed to flush stdio frame to '{server_name}'"))?;
        Ok(())
    }

    async fn read_stdio_response(
        &mut self,
        server_index: usize,
        expected_id: &Value,
    ) -> Result<Value> {
        loop {
            let message = self.read_stdio_message(server_index).await?;
            if message.get("id") == Some(expected_id) {
                return Ok(message);
            }
            debug!(
                server = %self.servers[server_index].config.name,
                expected_id = %expected_id,
                received = %message,
                "ignoring non-matching stdio MCP message"
            );
        }
    }

    async fn read_stdio_message(&mut self, server_index: usize) -> Result<Value> {
        let server_name = self.servers[server_index].config.name.clone();
        let stdio = self.servers[server_index]
            .stdio
            .as_mut()
            .ok_or_else(|| anyhow!("stdio transport for server '{server_name}' is not running"))?;
        read_stdio_frame(&mut stdio.stdout)
            .await
            .with_context(|| format!("failed to read stdio message from '{server_name}'"))
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

    fn reset_server_session(&mut self, server_index: usize) {
        self.servers[server_index].session_id = None;
        self.servers[server_index].protocol_version = None;
        self.servers[server_index].initialized = false;
        if self.servers[server_index].transport == Transport::Sse {
            self.servers[server_index].sse_endpoint = None;
            self.servers[server_index].sse_inbox = None;
            if let Some(task) = self.servers[server_index].sse_task.take() {
                task.abort();
            }
        }
        if let Some(mut stdio) = self.servers[server_index].stdio.take() {
            let _ = stdio.child.start_kill();
        }
    }

    fn next_request_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

async fn read_stdio_frame<R>(stdout: &mut R) -> Result<Value>
where
    R: AsyncBufRead + Unpin,
{
    let mut content_length = None;
    let mut line = String::new();

    loop {
        line.clear();
        let read = stdout
            .read_line(&mut line)
            .await
            .context("failed reading stdio frame header")?;
        if read == 0 {
            bail!("MCP stdio server closed stdout");
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .with_context(|| format!("invalid Content-Length header: {header}"))?,
            );
        }
    }

    let content_length =
        content_length.ok_or_else(|| anyhow!("missing Content-Length header in stdio frame"))?;
    let mut payload = vec![0u8; content_length];
    stdout
        .read_exact(&mut payload)
        .await
        .context("failed reading stdio frame payload")?;
    serde_json::from_slice(&payload).context("failed to parse stdio frame JSON payload")
}

#[derive(Debug)]
enum SseEvent {
    Endpoint(String),
    Message(Value),
}

#[derive(Debug, Default)]
struct SseEventParser {
    current_event: Option<String>,
    data_lines: Vec<String>,
    pending: Vec<u8>,
}

impl SseEventParser {
    fn push_bytes(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>> {
        self.pending.extend_from_slice(chunk);
        let mut events = Vec::new();

        while let Some(newline_index) = self.pending.iter().position(|byte| *byte == b'\n') {
            let mut line = self.pending.drain(..=newline_index).collect::<Vec<_>>();
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = std::str::from_utf8(&line).context("SSE stream contained invalid UTF-8")?;
            self.process_line(line, &mut events)?;
        }

        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<SseEvent>> {
        let mut events = Vec::new();
        if !self.pending.is_empty() {
            let pending = std::mem::take(&mut self.pending);
            let line =
                std::str::from_utf8(&pending).context("SSE stream contained invalid UTF-8")?;
            self.process_line(line.trim_end_matches('\r'), &mut events)?;
        }
        if let Some(event) = self.flush_event()? {
            events.push(event);
        }
        Ok(events)
    }

    fn process_line(&mut self, line: &str, events: &mut Vec<SseEvent>) -> Result<()> {
        if line.is_empty() {
            if let Some(event) = self.flush_event()? {
                events.push(event);
            }
            return Ok(());
        }

        if line.starts_with(':') {
            return Ok(());
        }

        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => self.current_event = Some(value.to_string()),
            "data" if self.current_event.as_deref() == Some("endpoint") => {
                self.current_event = None;
                self.data_lines.clear();
                events.push(SseEvent::Endpoint(value.trim().to_string()));
            }
            "data" => self.data_lines.push(value.to_string()),
            _ => {}
        }
        Ok(())
    }

    fn flush_event(&mut self) -> Result<Option<SseEvent>> {
        if self.data_lines.is_empty() {
            self.current_event = None;
            return Ok(None);
        }

        let event_type = self
            .current_event
            .take()
            .unwrap_or_else(|| "message".to_string());
        let payload = std::mem::take(&mut self.data_lines).join("\n");

        match event_type.as_str() {
            "endpoint" => Ok(Some(SseEvent::Endpoint(payload.trim().to_string()))),
            "message" => {
                if payload == "[DONE]" {
                    return Ok(None);
                }
                let value = serde_json::from_str(&payload)
                    .with_context(|| format!("failed to parse SSE event payload: {payload}"))?;
                Ok(Some(SseEvent::Message(value)))
            }
            _ => Ok(None),
        }
    }
}

async fn read_sse_endpoint_event(
    response: &mut reqwest::Response,
    parser: &mut SseEventParser,
    tx: &mpsc::Sender<Value>,
) -> Result<String> {
    while let Some(chunk) = response
        .chunk()
        .await
        .context("failed reading SSE endpoint stream")?
    {
        for event in parser.push_bytes(&chunk)? {
            match event {
                SseEvent::Endpoint(endpoint) => return Ok(endpoint),
                SseEvent::Message(value) => {
                    tx.send(value)
                        .await
                        .context("failed to queue early SSE message")?;
                }
            }
        }
    }

    for event in parser.finish()? {
        match event {
            SseEvent::Endpoint(endpoint) => return Ok(endpoint),
            SseEvent::Message(value) => {
                tx.send(value)
                    .await
                    .context("failed to queue early SSE message")?;
            }
        }
    }

    bail!("SSE endpoint event not found in response")
}

async fn read_sse_messages(
    server_name: String,
    mut response: reqwest::Response,
    mut parser: SseEventParser,
    tx: mpsc::Sender<Value>,
) {
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => match parser.push_bytes(&chunk) {
                Ok(events) => {
                    for event in events {
                        match event {
                            SseEvent::Endpoint(endpoint) => {
                                debug!(server = %server_name, endpoint, "ignoring duplicate SSE endpoint event");
                            }
                            SseEvent::Message(value) => {
                                if tx.send(value).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    warn!(server = %server_name, error = %err, "failed to parse MCP SSE stream");
                    return;
                }
            },
            Ok(None) => break,
            Err(err) => {
                warn!(server = %server_name, error = %err, "failed reading MCP SSE stream");
                return;
            }
        }
    }

    match parser.finish() {
        Ok(events) => {
            for event in events {
                if let SseEvent::Message(value) = event
                    && tx.send(value).await.is_err()
                {
                    return;
                }
            }
        }
        Err(err) => warn!(server = %server_name, error = %err, "failed to finish MCP SSE stream"),
    }
}

fn resolve_sse_endpoint_url(base_url: &str, endpoint: &str) -> Result<String> {
    if Url::parse(endpoint).is_ok() {
        return Ok(endpoint.to_string());
    }

    let base = Url::parse(base_url).with_context(|| format!("invalid SSE base URL: {base_url}"))?;
    Ok(base
        .join(endpoint)
        .with_context(|| format!("invalid SSE endpoint URL: {endpoint}"))?
        .to_string())
}

fn should_fallback_to_legacy_sse(err: &anyhow::Error) -> bool {
    let Some(status_error) = err.downcast_ref::<HttpStatusError>() else {
        return false;
    };

    matches!(
        status_error.status,
        StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
    ) || (status_error.status == StatusCode::BAD_REQUEST
        && status_error.body.to_ascii_lowercase().contains("sessionid"))
}

/// Parse the endpoint URL from an SSE `endpoint` event.
#[cfg(test)]
fn parse_sse_endpoint_event(body: &str) -> Option<String> {
    let mut parser = SseEventParser::default();
    let mut events = parser.push_bytes(body.as_bytes()).ok()?;
    events.extend(parser.finish().ok()?);
    events.into_iter().find_map(|event| match event {
        SseEvent::Endpoint(endpoint) => Some(endpoint),
        SseEvent::Message(_) => None,
    })
}

fn build_headers(
    config: &McpServerConfig,
    session_id: Option<&str>,
    protocol_version: Option<&str>,
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
    if let Some(McpAuthConfig::StaticHeaders(static_headers)) = &config.auth {
        for (name, value) in &static_headers.headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())?;
            let header_value = HeaderValue::from_str(value)?;
            headers.insert(header_name, header_value);
        }
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
    if let Some(protocol_version) = protocol_version {
        headers.insert(
            HeaderName::from_static("mcp-protocol-version"),
            HeaderValue::from_str(protocol_version)?,
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
    use std::{
        collections::HashMap,
        convert::Infallible,
        fs,
        os::unix::fs::PermissionsExt,
        sync::{
            Arc, Mutex, MutexGuard, OnceLock,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use axum::{
        Json, Router,
        extract::{Query, State},
        http::{HeaderMap as AxumHeaderMap, StatusCode},
        response::{
            IntoResponse,
            sse::{Event, Sse},
        },
        routing::{get, post},
    };
    use reqwest::header::ACCEPT;
    use serde_json::json;
    use tempfile::tempdir;
    use tokio::{
        io::BufReader as TokioBufReader,
        net::{TcpListener, TcpStream},
        sync::{Mutex as TokioMutex, mpsc as TokioMpsc},
    };
    use tokio_stream::{StreamExt as TokioStreamExt, wrappers::ReceiverStream};

    use crate::config::{McpRuntimeConfig, McpServerConfig, ToolEnvironmentConfig};

    use super::{
        McpManager, PREFERRED_PROTOCOL_VERSION, ServerState, Transport, build_headers,
        normalize_tool_result, parse_mcp_response_body, parse_sse_endpoint_event, read_stdio_frame,
    };

    fn mock_mcp_server_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn acquire_mock_mcp_server_test_lock() -> MutexGuard<'static, ()> {
        match mock_mcp_server_test_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

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
                command: None,
                args: Vec::new(),
                headers: Default::default(),
                timeout: Some(30),
                sse_read_timeout: Some(300),
                client_session_timeout_seconds: Some(30),
                auth: None,
            },
            transport: Transport::StreamableHttp,
            session_id: None,
            protocol_version: None,
            sse_endpoint: None,
            sse_inbox: None,
            sse_task: None,
            stdio: None,
            initialized: false,
        };

        let headers = build_headers(&server.config, None, None, None).unwrap();
        let accept = headers.get(ACCEPT).unwrap().to_str().unwrap();

        assert!(accept.contains("application/json"));
        assert!(accept.contains("text/event-stream"));
    }

    #[test]
    fn includes_negotiated_protocol_version_header_when_present() {
        let headers = build_headers(
            &McpServerConfig {
                name: "fastmcp".to_string(),
                transport: "streamable_http".to_string(),
                url: "http://127.0.0.1:8000/mcp".to_string(),
                command: None,
                args: Vec::new(),
                headers: Default::default(),
                timeout: Some(30),
                sse_read_timeout: Some(300),
                client_session_timeout_seconds: Some(30),
                auth: None,
            },
            Some("session-123"),
            Some("2025-06-18"),
            None,
        )
        .unwrap();

        assert_eq!(
            headers
                .get("mcp-protocol-version")
                .unwrap()
                .to_str()
                .unwrap(),
            "2025-06-18"
        );
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

    #[test]
    fn parses_sse_endpoint_event() {
        let body = "event: endpoint\ndata: /messages?sessionId=abc123\n\n";

        assert_eq!(
            parse_sse_endpoint_event(body).as_deref(),
            Some("/messages?sessionId=abc123")
        );
    }

    #[test]
    fn parses_sse_endpoint_event_without_blank_delimiter() {
        let body = "event: endpoint\ndata: ?sessionId=abc123";

        assert_eq!(
            parse_sse_endpoint_event(body).as_deref(),
            Some("?sessionId=abc123")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn parses_stdio_framed_jsonrpc_result() {
        let payload =
            "{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"tools\":[{\"name\":\"hello\"}]}}";
        let frame = format!("Content-Length: {}\r\n\r\n{}", payload.len(), payload);
        let mut reader = TokioBufReader::new(frame.as_bytes());

        let parsed = read_stdio_frame(&mut reader).await.unwrap();

        assert_eq!(parsed["result"]["tools"][0]["name"], "hello");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stdio_server_receives_tool_environment() {
        let dir = tempdir().unwrap();
        let bin_dir = dir.path().join("tool-bin");
        let env_file = dir.path().join("stdio-env.txt");
        let script_path = dir.path().join("stdio-env.sh");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(
            &script_path,
            "#!/bin/sh\nprintf '%s\\n' \"$MCP_STDIO_TOOL_ENV_TEST\" > \"$1\"\nprintf '%s\\n' \"${PATH%%:*}\" >> \"$1\"\nsleep 1\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).unwrap();

        let mut manager = McpManager::new(
            dir.path(),
            McpRuntimeConfig::default(),
            vec![McpServerConfig {
                name: "stdio-env".to_string(),
                transport: "stdio".to_string(),
                url: String::new(),
                command: Some("/bin/sh".to_string()),
                args: vec![
                    script_path.display().to_string(),
                    env_file.display().to_string(),
                ],
                headers: HashMap::new(),
                timeout: Some(30),
                sse_read_timeout: None,
                client_session_timeout_seconds: Some(30),
                auth: None,
            }],
        )
        .unwrap()
        .with_tool_environment(ToolEnvironmentConfig {
            pass_through: Vec::new(),
            variables: HashMap::from([(
                "MCP_STDIO_TOOL_ENV_TEST".to_string(),
                "stdio-value".to_string(),
            )]),
            path_prepend: vec![bin_dir.clone()],
        });

        manager.ensure_stdio_started(0).await.unwrap();
        for _ in 0..50 {
            if env_file.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        manager.reset_server_session(0);

        let contents = fs::read_to_string(&env_file).unwrap();
        let mut lines = contents.lines();
        let expected_path = bin_dir.display().to_string();
        assert_eq!(lines.next(), Some("stdio-value"));
        assert_eq!(lines.next(), Some(expected_path.as_str()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_unsupported_negotiated_protocol_version() {
        let _guard = acquire_mock_mcp_server_test_lock();
        let dir = tempdir().unwrap();
        let state = Arc::new(MockServerState {
            initialize_count: AtomicUsize::new(0),
            saw_protocol_header: AtomicUsize::new(0),
            expire_first_tool_list: false,
            negotiated_version: "2099-01-01".to_string(),
            delayed_tool_list_ms: 0,
            cancelled_request_count: AtomicUsize::new(0),
        });
        let addr = spawn_mock_mcp_server(state.clone()).await;
        let mut manager = build_test_manager(dir.path(), &format!("http://{addr}/mcp"));

        let err = manager.list_tools().await.unwrap_err();

        let message = format!("{err:#}");
        assert!(
            message.contains("unsupported MCP protocol version"),
            "{message}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retries_after_streamable_http_session_expiry() {
        let _guard = acquire_mock_mcp_server_test_lock();
        let dir = tempdir().unwrap();
        let state = Arc::new(MockServerState {
            initialize_count: AtomicUsize::new(0),
            saw_protocol_header: AtomicUsize::new(0),
            expire_first_tool_list: true,
            negotiated_version: PREFERRED_PROTOCOL_VERSION.to_string(),
            delayed_tool_list_ms: 0,
            cancelled_request_count: AtomicUsize::new(0),
        });
        let addr = spawn_mock_mcp_server(state.clone()).await;
        let mut manager = build_test_manager(dir.path(), &format!("http://{addr}/mcp"));

        let tools = manager.list_tools().await.unwrap();

        assert_eq!(tools.len(), 1);
        assert_eq!(state.initialize_count.load(Ordering::SeqCst), 2);
        assert!(state.saw_protocol_header.load(Ordering::SeqCst) >= 1);
        assert_eq!(
            manager.servers[0].protocol_version.as_deref(),
            Some(PREFERRED_PROTOCOL_VERSION)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn legacy_sse_transport_uses_advertised_session_endpoint() {
        let _guard = acquire_mock_mcp_server_test_lock();
        let dir = tempdir().unwrap();
        let state = Arc::new(LegacySseState::default());
        let addr = spawn_legacy_sse_server(state.clone()).await;
        let mut manager = McpManager::new(
            dir.path(),
            McpRuntimeConfig::default(),
            vec![McpServerConfig {
                name: "burp".to_string(),
                transport: "sse".to_string(),
                url: format!("http://{addr}/sse"),
                command: None,
                args: Vec::new(),
                headers: HashMap::new(),
                timeout: Some(30),
                sse_read_timeout: Some(30),
                client_session_timeout_seconds: Some(30),
                auth: None,
            }],
        )
        .unwrap();

        let tools = manager.list_tools().await.unwrap();
        let result = manager.call_tool("burp__hello", json!({})).await.unwrap();

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].external_name, "burp__hello");
        assert_eq!(result, "world");
        assert_eq!(state.initialize_count.load(Ordering::SeqCst), 1);
        assert_eq!(state.missing_session_count.load(Ordering::SeqCst), 0);
        manager.reset_server_session(0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn streamable_http_can_fallback_to_legacy_sse_endpoint() {
        let _guard = acquire_mock_mcp_server_test_lock();
        let dir = tempdir().unwrap();
        let state = Arc::new(LegacySseState::default());
        let addr = spawn_legacy_sse_server(state.clone()).await;
        let mut manager = McpManager::new(
            dir.path(),
            McpRuntimeConfig::default(),
            vec![McpServerConfig {
                name: "burp".to_string(),
                transport: "streamable_http".to_string(),
                url: format!("http://{addr}/sse"),
                command: None,
                args: Vec::new(),
                headers: HashMap::new(),
                timeout: Some(30),
                sse_read_timeout: Some(30),
                client_session_timeout_seconds: Some(30),
                auth: None,
            }],
        )
        .unwrap();

        let tools = manager.list_tools().await.unwrap();

        assert_eq!(tools.len(), 1);
        assert_eq!(manager.servers[0].transport, Transport::Sse);
        assert_eq!(state.initialize_count.load(Ordering::SeqCst), 1);
        assert_eq!(state.missing_session_count.load(Ordering::SeqCst), 1);
        manager.reset_server_session(0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sends_cancelled_notification_when_request_times_out() {
        let _guard = acquire_mock_mcp_server_test_lock();
        let dir = tempdir().unwrap();
        let state = Arc::new(MockServerState {
            initialize_count: AtomicUsize::new(0),
            saw_protocol_header: AtomicUsize::new(0),
            expire_first_tool_list: false,
            negotiated_version: PREFERRED_PROTOCOL_VERSION.to_string(),
            delayed_tool_list_ms: 1_500,
            cancelled_request_count: AtomicUsize::new(0),
        });
        let addr = spawn_mock_mcp_server(state.clone()).await;
        let mut manager = McpManager::new(
            dir.path(),
            McpRuntimeConfig {
                connect_timeout_seconds: 1,
                cleanup_timeout_seconds: 10,
                connect_in_parallel: false,
            },
            vec![McpServerConfig {
                name: "demo".to_string(),
                transport: "streamable_http".to_string(),
                url: format!("http://{addr}/mcp"),
                command: None,
                args: Vec::new(),
                headers: HashMap::new(),
                timeout: Some(1),
                sse_read_timeout: None,
                client_session_timeout_seconds: Some(1),
                auth: None,
            }],
        )
        .unwrap();

        let err = manager.list_tools().await.unwrap_err();

        assert!(format!("{err:#}").contains("timed out while waiting for tools/list"));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(state.cancelled_request_count.load(Ordering::SeqCst), 1);
    }

    #[derive(Default)]
    struct LegacySseState {
        sender: TokioMutex<Option<TokioMpsc::Sender<String>>>,
        initialize_count: AtomicUsize,
        missing_session_count: AtomicUsize,
    }

    async fn legacy_sse_handler(State(state): State<Arc<LegacySseState>>) -> impl IntoResponse {
        let (tx, rx) = TokioMpsc::channel::<String>(16);
        *state.sender.lock().await = Some(tx);

        let endpoint = Event::default()
            .event("endpoint")
            .data("/messages?sessionId=legacy-1");
        let messages = ReceiverStream::new(rx).map(|payload| {
            Ok::<Event, Infallible>(Event::default().event("message").data(payload))
        });
        let stream = tokio_stream::iter(vec![Ok::<Event, Infallible>(endpoint)]).chain(messages);

        Sse::new(stream)
    }

    async fn legacy_sse_message_handler(
        State(state): State<Arc<LegacySseState>>,
        Query(params): Query<HashMap<String, String>>,
        Json(body): Json<serde_json::Value>,
    ) -> impl IntoResponse {
        if params.get("sessionId").map(String::as_str) != Some("legacy-1") {
            state.missing_session_count.fetch_add(1, Ordering::SeqCst);
            return (
                StatusCode::BAD_REQUEST,
                "sessionId query parameter is not provided",
            )
                .into_response();
        }

        let method = body
            .get("method")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if body.get("id").is_none() {
            return StatusCode::ACCEPTED.into_response();
        }

        let id = body.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let result = match method {
            "initialize" => {
                state.initialize_count.fetch_add(1, Ordering::SeqCst);
                json!({
                    "protocolVersion": PREFERRED_PROTOCOL_VERSION,
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "legacy-sse",
                        "version": "1.0.0"
                    }
                })
            }
            "tools/list" => json!({
                "tools": [{
                    "name": "hello",
                    "description": "demo tool",
                    "inputSchema": {"type": "object", "properties": {}}
                }]
            }),
            "tools/call" => json!({
                "content": [{
                    "type": "text",
                    "text": "world"
                }]
            }),
            _ => json!({}),
        };
        let response = json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        });

        let Some(sender) = state.sender.lock().await.clone() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        if sender.send(response.to_string()).await.is_err() {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }

        (StatusCode::ACCEPTED, "Accepted").into_response()
    }

    async fn spawn_legacy_sse_server(state: Arc<LegacySseState>) -> std::net::SocketAddr {
        let app = Router::new()
            .route(
                "/sse",
                get(legacy_sse_handler).post(legacy_sse_message_handler),
            )
            .route("/messages", post(legacy_sse_message_handler))
            .with_state(state);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = TcpListener::from_std(listener).unwrap();
                ready_tx.send(()).unwrap();
                axum::serve(listener, app).await.unwrap();
            });
        });
        ready_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("legacy SSE test server did not start");
        wait_for_mock_mcp_server(addr).await;
        addr
    }

    struct MockServerState {
        initialize_count: AtomicUsize,
        saw_protocol_header: AtomicUsize,
        expire_first_tool_list: bool,
        negotiated_version: String,
        delayed_tool_list_ms: u64,
        cancelled_request_count: AtomicUsize,
    }

    fn build_test_manager(data_dir: &std::path::Path, url: &str) -> McpManager {
        McpManager::new(
            data_dir,
            McpRuntimeConfig::default(),
            vec![McpServerConfig {
                name: "demo".to_string(),
                transport: "streamable_http".to_string(),
                url: url.to_string(),
                command: None,
                args: Vec::new(),
                headers: HashMap::new(),
                timeout: Some(30),
                sse_read_timeout: None,
                client_session_timeout_seconds: Some(30),
                auth: None,
            }],
        )
        .unwrap()
    }

    async fn mock_mcp_handler(
        State(state): State<Arc<MockServerState>>,
        headers: AxumHeaderMap,
        Json(body): Json<serde_json::Value>,
    ) -> impl IntoResponse {
        let method = body
            .get("method")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let id = body.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let session_header = headers
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        if headers.contains_key("mcp-protocol-version") {
            state.saw_protocol_header.fetch_add(1, Ordering::SeqCst);
        }

        if method == "notifications/cancelled" {
            state.cancelled_request_count.fetch_add(1, Ordering::SeqCst);
            return StatusCode::ACCEPTED.into_response();
        }

        if method == "tools/list"
            && state.expire_first_tool_list
            && state.initialize_count.load(Ordering::SeqCst) == 1
            && session_header.as_deref() == Some("session-1")
        {
            return (StatusCode::NOT_FOUND, Json(json!({"error": "expired"}))).into_response();
        }
        if method == "tools/list" && state.delayed_tool_list_ms > 0 {
            tokio::time::sleep(Duration::from_millis(state.delayed_tool_list_ms)).await;
        }

        let mut response_headers = AxumHeaderMap::new();
        let result = match method {
            "initialize" => {
                let count = state.initialize_count.fetch_add(1, Ordering::SeqCst) + 1;
                response_headers.insert(
                    "Mcp-Session-Id",
                    format!("session-{count}").parse().unwrap(),
                );
                json!({
                    "protocolVersion": state.negotiated_version,
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "demo",
                        "version": "1.0.0"
                    }
                })
            }
            "tools/list" => json!({
                "tools": [{
                    "name": "hello",
                    "description": "demo tool",
                    "inputSchema": {"type": "object", "properties": {}}
                }]
            }),
            _ => json!({}),
        };

        let mut response = Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }))
        .into_response();
        *response.status_mut() = StatusCode::OK;
        response.headers_mut().extend(response_headers);
        response
    }

    async fn spawn_mock_mcp_server(state: Arc<MockServerState>) -> std::net::SocketAddr {
        let app = Router::new()
            .route("/mcp", post(mock_mcp_handler))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        wait_for_mock_mcp_server(addr).await;
        addr
    }

    async fn wait_for_mock_mcp_server(addr: std::net::SocketAddr) {
        let mut last_error = None;
        for _ in 0..100 {
            match TcpStream::connect(addr).await {
                Ok(stream) => {
                    drop(stream);
                    return;
                }
                Err(err) => {
                    last_error = Some(err);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
        panic!(
            "mock MCP server did not start at {addr}: {}",
            last_error
                .map(|err| err.to_string())
                .unwrap_or_else(|| "unknown error".to_string())
        );
    }
}
