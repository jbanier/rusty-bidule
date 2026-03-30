# rusty-bidule

`rusty-bidule` is a **prototype** Rust MCP client with a cyberpunk-style terminal UI.

It is designed to:

- talk to Azure OpenAI for reasoning,
- connect to MCP servers over `streamable_http`,
- preserve conversations and tool evidence on disk,
- and support OAuth login for MCP servers that require browser-based authentication.

## Quick start

If you just want to get the prototype running:

1. Copy the sample config:

   ```bash
   cp config/config.example.yaml config/config.local.yaml
   ```

2. Export your Azure key:

   ```bash
   export AZURE_OPENAI_API_KEY='your-key-here'
   ```

3. Update `config/config.local.yaml` with your real Azure endpoint, deployment, and optional MCP server settings.

4. Launch the TUI:

   ```bash
   cargo run
   ```

For a one-shot run instead of the TUI:

```bash
cargo run -- --once "Summarize the latest findings"
```

## Prototype status

This repository is intentionally a working prototype, not a polished product.

That means:

- interfaces and configuration may still change,
- compatibility is focused on the current Azure + MCP use case,
- operational hardening is incomplete,
- and some behaviors degrade gracefully rather than aiming for full platform parity.

If you adopt it, treat it as an experimental operator tool.

## Current capabilities

- Ratatui/crossterm terminal interface
- Markdown rendering in the message stack
- Latest-first message history with scrolling and a scroll indicator
- Azure OpenAI chat completions via `async-openai`
- MCP tool discovery and tool invocation over `streamable_http`
- Compatibility fixes for FastMCP-style servers
- OAuth public-client login flow for selected MCP servers
- Durable conversation logs and tool evidence under `data/`
- Centralized application logging in `var/bidule.log`
- Headless one-shot execution with `--once`

## Requirements

- Rust toolchain with Cargo
- Network access to your Azure OpenAI endpoint
- Zero or more reachable MCP servers, depending on whether you want MCP-backed tools
- A local browser for OAuth flows if you use `oauth_public`

## Project layout

- `src/` — application code
- `config/config.example.yaml` — example configuration
- `config/config.local.yaml` — local operator config (ignored by git)
- `data/` — conversations, OAuth state, and tool evidence
- `var/bidule.log` — application log file

## Configuration

Copy the example config and adapt it locally:

```bash
cp config/config.example.yaml config/config.local.yaml
```

The default config path is discovered automatically as `config/config.local.yaml`. You can override it with either:

- `RUSTY_BIDULE_CONFIG=/path/to/config.yaml`
- `cargo run -- --config /path/to/config.yaml`

### Azure OpenAI

The current config shape is:

```yaml
azure_openai:
  api_key: env:AZURE_OPENAI_API_KEY
  api_version: 2025-03-01-preview
  endpoint: https://example.cognitiveservices.azure.com/
  deployment: gpt-4.1
  temperature: 0.2
  top_p: 1.0
  max_output_tokens: 1200
```

Recommended practice is to keep the API key out of the YAML file and provide it through the environment:

```bash
export AZURE_OPENAI_API_KEY='your-key-here'
```

### MCP servers

MCP servers are optional. If you configure them, the prototype currently expects
`streamable_http` mode:

```yaml
mcp_servers:
  - name: my-own-mcp
    transport: streamable_http
    url: http://127.0.0.1:5000/mcp
    headers:
      Authorization: Bearer None
```

Extra headers are passed through as configured.

### OAuth-enabled MCP servers

Servers that require browser login can use `auth.type: oauth_public`:

```yaml
  - name: wiz
    transport: streamable_http
    url: https://mcp.app.wiz.io
    headers:
      Wiz-DataCenter: <some values>
    auth:
      type: oauth_public
      scopes:
        - read:all
        - offline_access
      client_id: null
      client_secret: null
      token_endpoint_auth_method: none
      resource: https://mcp.app.wiz.io/
      redirect_uri: http://localhost:8766/callback
      redirect_host: localhost
      redirect_port: 8766
      redirect_path: /callback
      callback_timeout_seconds: 300
      open_browser: true
      use_dynamic_client_registration: true
```

OAuth tokens and client registrations are stored under `data/oauth/`.

## Running the prototype

### Interactive TUI

```bash
cargo run
```

### Headless one-shot mode

```bash
cargo run -- --once "Summarize the latest findings"
```

You can also target a specific stored conversation:

```bash
cargo run -- --conversation convo-20260318171242-0df4dd9f --once "List involved assets"
```

## TUI commands

Inside the terminal UI:

- `/new` — create a new conversation
- `/list` — list known conversations
- `/use <id>` — switch conversation
- `/show [id]` — show conversation details
- `/delete <id>` — delete a conversation
- `/login <server>` — trigger OAuth login for an MCP server
- `/model` — show model-selection note
- `/logging` — show logging note
- `/help` — show command help
- `/exit` or `/quit` — leave the TUI and restore the terminal

### TUI navigation

- `Up` / `Down` — scroll message history
- `PageUp` / `PageDown` — page through history
- `Home` / `End` — jump in history
- enter `<<<` to start multiline input
- enter `>>>` to send multiline input

## Logging and evidence

The prototype writes two kinds of durable output:

- `var/bidule.log` — application-level logs (`DEBUG`, `INFO`, `WARN`, `ERROR`)
- `data/conversations/...` — stored messages, conversation logs, and tool artifacts

This split is intentional:

- `var/bidule.log` helps debug runtime issues,
- while `data/` preserves operator-visible evidence and tool output.

## Validation

Run the test suite with:

```bash
cargo test
```

## Known limitations

- This is still a prototype and not a hardened production client.
- Only `streamable_http` MCP transport is supported today.
- Azure behavior is aligned with the current chat-completions flow, not every Azure/OpenAI feature (still looking from a framework).
- Large MCP tool inventories are truncated before Azure submission to stay within provider limits (128).
- Some MCP schemas require normalization for Azure compatibility.

## Notes for operators

- If the app reports config-loading issues, confirm `AZURE_OPENAI_API_KEY` is exported and the selected config path exists.
- If an MCP server requires OAuth, run `/login <server>` before relying on its tools.
- If something goes wrong, inspect `var/bidule.log` first.
