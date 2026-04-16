# rusty-bidule

`rusty-bidule` is a prototype Rust client for investigation workflows. It pairs
Azure-hosted LLM reasoning with MCP tools, optional local skill scripts,
persistent conversation state, and both terminal and browser interfaces.

The project is aimed at operators who want a tool-grounded assistant rather than
an open-ended chat shell.

## What It Does

- Runs interactive conversations in a Ratatui terminal UI
- Exposes a lightweight web UI and REST API
- Calls Azure OpenAI or Azure Anthropic chat completions for reasoning
- Discovers and invokes MCP tools from configured servers
- Executes selected local skill scripts through `local__run_skill`
- Executes configured allowlisted local CLI tools through `local__exec_cli`
- Persists conversations, compactions, logs, OAuth state, and tool evidence
- Supports OAuth public-client login for MCP servers that require browser auth

## Prototype Status

This repository is intentionally still a working prototype.

- Interfaces and config shape may evolve
- Operational hardening is incomplete
- Azure + MCP is the primary path that gets attention
- Some provider or schema mismatches are normalized pragmatically rather than
  handled as full protocol parity

Treat it as an operator tool under active iteration, not a finished platform.

## Requirements

- Rust toolchain with Cargo
- Network access to your configured Azure LLM endpoint
- Zero or more reachable MCP servers if you want MCP-backed tools
- A local browser if you use OAuth-enabled MCP servers

## Quick Start

1. Copy the sample config:

   ```bash
   cp config/config.example.yaml config/config.local.yaml
   ```

2. Export your provider key:

   ```bash
   export AZURE_OPENAI_API_KEY='your-key-here'
   ```

3. Edit `config/config.local.yaml` with your selected provider block,
   endpoint, deployment, and any MCP server settings.

4. Launch the terminal UI:

   ```bash
   cargo run
   ```

For a one-shot run:

```bash
cargo run -- --once "Summarize the latest findings"
```

For the web interface:

```bash
cargo run -- --interface web --port 8080
```

Prompt input also supports inline file inclusion with `@path/to/file.md`. The
app replaces each reference with a labeled block containing that file's text
before the turn is sent. Relative paths are resolved from the detected project
root. Use `\@file.md` to keep a literal `@file.md` in the prompt instead of
expanding it.

Inline file inclusion requires filesystem read permission for the active
conversation. If access is disabled, or the file cannot be resolved or read,
the submission fails immediately.

Recipes are currently guidance overlays for the model, not a deterministic
playbook engine. They can bias tool usage and structure, but they do not yet
enforce mandatory multi-step branching, variable passing, or guaranteed tool
execution order.

## Running Modes

### Interactive TUI

```bash
cargo run
```

Use this mode when you want an operator-facing terminal workflow with
conversation browsing, recipes, permission controls, and inline progress.

### Web Interface

```bash
cargo run -- --interface web --host 127.0.0.1 --port 8080
```

This starts an Axum server that serves the browser UI at `/` and exposes a REST
API under `/api/...`.

### One-shot Mode

```bash
cargo run -- --once "Summarize the latest findings"
```

You can target an existing conversation:

```bash
cargo run -- --conversation convo-20260318171242-0df4dd9f --once "List involved assets"
```

One-shot mode prints the final reply to `stdout` and progress updates to
`stderr`.

## Configuration

The default config file is `config/config.local.yaml`. The binary searches
upward from the current working directory to find the project root and then uses
that path.

You can override the path with either:

- `RUSTY_BIDULE_CONFIG=/path/to/config.yaml`
- `cargo run -- --config /path/to/config.yaml`

### Minimal Local-Only Configuration

If you want to start without MCP servers, keep `mcp_servers` empty or omit it:

```yaml
prompt: |
  You are a CSIRT investigation assistant.

llm_provider: azure_openai

azure_openai:
  api_key: env:AZURE_OPENAI_API_KEY
  api_version: 2025-03-01-preview
  endpoint: https://example.cognitiveservices.azure.com/
  deployment: gpt-4.1

mcp_servers: []
```

### LLM Provider Selection

Use `llm_provider` to select the active backend:

```yaml
llm_provider: azure_openai
```

If `llm_provider` is omitted, the app defaults to `azure_openai` when that
block is present; otherwise it falls back to `azure_anthropic` when only that
block is configured.

### Azure OpenAI

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

### Azure Anthropic

```yaml
azure_anthropic:
  api_key: env:AZURE_ANTHROPIC_API_KEY
  anthropic_version: 2023-06-01
  endpoint: https://example.services.ai.azure.com/anthropic/
  deployment: claude-opus-4-6
  temperature: 0.2
  max_output_tokens: 1200
```

Values prefixed with `env:` are resolved from environment variables at startup.
That resolution is supported for both LLM providers, MCP URLs, MCP header
values, and OAuth client settings.

`azure_anthropic` uses Anthropic's version header, not Azure OpenAI preview API
versions. If you omit `anthropic_version`, the client defaults to `2023-06-01`.
For Anthropic requests, set either `temperature` or `top_p`, not both. The
default path uses `temperature`; if you set a non-default `top_p`, it replaces
`temperature` in the outgoing request.

### Agent Permissions

The app applies default tool permissions from config to every new conversation:

```yaml
agent_permissions:
  allow_network: false
  filesystem: read_only
  yolo: false
```

- `allow_network`: controls networked tool use such as MCP calls
- `filesystem`: one of `none`, `read_only`, `read_write`
- `yolo`: bypasses the internal permission checks

In the TUI, these can be changed per conversation with `/permissions ...` and
`/yolo ...`.

### Local Tools

Local built-in tools are configured under:

```yaml
local_tools:
  execution_timeout_seconds: 180
  allowed_cli_tools:
    - nmap
    - vt
    - dig
    - whois
    - nslookup
```

- `local__run_skill` executes script-backed skills
- `local__exec_cli` executes only the listed binary names with direct argv execution
- `local__time` returns current UTC/local time and computes relative windows like last 12 hours or last 2 days
- `local__exec_cli` does not invoke a shell, and does not support pipes, redirects, or path-based commands

### MCP Runtime

Current shared runtime settings:

```yaml
mcp_runtime:
  connect_timeout_seconds: 180
```

### MCP Servers

MCP is optional. Each server supports `streamable_http`, `sse`, or `stdio`
transport:

```yaml
mcp_servers:
  - name: csirt-mcp
    transport: streamable_http
    url: http://127.0.0.1:5000/mcp
    headers:
      Authorization: Bearer None
    timeout: 30
    sse_read_timeout: 300
    client_session_timeout_seconds: 30
```

Configured headers are passed through as-is after `env:` resolution.

For stdio-backed MCP servers, configure the launcher command and args instead
of a URL:

```yaml
  - name: chrome-devtools
    transport: stdio
    command: npx
    args:
      - -y
      - chrome-devtools-mcp@latest
```

### OAuth-enabled MCP Servers

Servers that require browser login can use `auth.type: oauth_public`:

```yaml
  - name: wiz
    transport: streamable_http
    url: https://mcp.app.wiz.io
    headers:
      Wiz-DataCenter: us12
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

OAuth tokens and dynamic client registrations are stored under `data/oauth/`.
Login can be triggered from the TUI with `/login <server>`.

## Project Layout

- `src/` application code
- `config/config.example.yaml` example configuration
- `config/config.local.yaml` local operator config
- `skills/` local skill definitions and scripts
- `recipes/` prompt/config bundles for investigations
- `data/` conversations, OAuth state, and tool evidence
- `var/bidule.log` application log file

## TUI Commands

Conversation management:

- `/new`
- `/list [all|archived]`
- `/use <id>`
- `/show [id]`
- `/title [text]`
- `/archive [id]`
- `/unarchive <id>`
- `/export [id]`
- `/delete <id>`
- `/compact`

Recipes:

- `/recipes`
- `/recipe use <name>`
- `/recipe show <name>`
- `/recipe clear`

MCP and auth:

- `/login <server>`
- `/mcp` or `/mcp status`
- `/mcp reset|enable|disable|only <name...>`

Permissions:

- `/permissions`
- `/permissions network on|off`
- `/permissions fs none|read|write`
- `/permissions yolo on|off`
- `/permissions reset`
- `/yolo on|off`

Local analyst state:

- `/scratch` or `/scratch show`
- `/scratch set <text>`
- `/scratch append <text>`
- `/scratch clear`
- `/findings` or `/findings list`
- `/findings add <kind> <value> [note]`
- `/findings update <finding-id> <kind|value|note|tags|confidence|artifact> <value>`
- `/findings remove <finding-id>`
- `/search <query>`

Other:

- `/logging`
- `/help`
- `/exit` or `/quit`

### TUI Navigation

- `Up` / `Down` scroll message history
- `Ctrl+Up` / `Ctrl+Down` browse session-only input history
- `PageUp` / `PageDown` page through history
- `Home` / `End` jump in history
- enter `<<<` to start multiline input
- enter `>>>` to send multiline input

### Inline File References

You can embed local file contents directly in a prompt:

```text
Summarize the following notes @docs/incident.md
```

The app expands that into a labeled fenced block before dispatch. This works in
the TUI, the web UI/API message submission path, and `--once`.

## Skills And Recipes

Skills are loaded from `skills/<skill-name>/SKILL.md`. On the current
chat-completions transport they are exposed as capability metadata plus
optional script-backed tools executed through `local__run_skill`.

Recipes are loaded from `recipes/<recipe-name>/RECIPE.md`. They let you preload
instructions, an initial prompt, and an MCP server filter for a conversation.

## Persistence, Logging, And Evidence

Durable output is split by purpose:

- `var/bidule.log` stores application-level logs
- `data/conversations/...` stores messages, per-conversation logs, compactions,
  and tool artifacts
- `data/oauth/...` stores OAuth state, tokens, and client registration data

This is deliberate: `var/` is for runtime diagnostics, while `data/` is for
operator-visible state and evidence.

## Validation

Run the test suite with:

```bash
cargo test
```

## Known Limitations

- This is not yet a hardened production client
- MCP filtering and permission checks are application-level controls
- Azure tool advertising is truncated when inventories exceed provider limits
- Some MCP schemas need normalization for Azure compatibility
- The browser UI is intentionally lightweight compared to the TUI

## More Detail

User-facing setup and operation live here in `README.md`.

Engineering details, architecture, runtime behavior, and interface references
live in [`docs/REFERENCE.md`](docs/REFERENCE.md).
