# rusty-bidule — Reference Guide

rusty-bidule is a Rust operator tool for CSIRT workflows. It uses Azure OpenAI for
reasoning, can discover and call MCP tools, can execute selected local skill
scripts, and persists conversations and tool evidence on disk.

## Filesystem Layout

```text
rusty-bidule/
├── src/
│   ├── main.rs
│   ├── orchestrator.rs
│   ├── web.rs
│   ├── ui.rs
│   ├── config.rs
│   ├── mcp_runtime.rs
│   ├── oauth.rs
│   ├── local_tools.rs
│   ├── skills.rs
│   ├── recipes.rs
│   └── static/index.html
├── config/config.example.yaml
├── data/conversations/
├── skills/<skill-name>/SKILL.md
├── recipes/<recipe-name>/RECIPE.md
└── var/bidule.log
```

## Configuration

The default config path is `config/config.local.yaml`, searched upward from the
current working directory. You can override it with `--config` or
`RUSTY_BIDULE_CONFIG`.

Top-level keys:

| Key | Required | Notes |
|-----|----------|-------|
| `azure_openai` | yes | Azure endpoint, deployment, API version, and key |
| `prompt` | no | Extra system prompt text |
| `data_dir` | no | Defaults to `data` |
| `agent_permissions` | no | Default per-conversation tool permissions |
| `mcp_runtime` | no | MCP timeout settings |
| `mcp_servers` | no | Optional list of MCP servers |
| `tracing` | no | Logging/tracing mode |

`mcp_servers` may be omitted or set to `[]` for a local-tools-only setup.

Current `agent_permissions` fields:

| Field | Notes |
|-------|-------|
| `allow_network` | Allows networked tool execution such as MCP tool calls |
| `filesystem` | One of `none`, `read_only`, `read_write` |
| `yolo` | Bypasses the internal tool permission checks |

Current `mcp_runtime` fields:

| Field | Notes |
|-------|-------|
| `connect_timeout_seconds` | Connection timeout used when contacting MCP servers |

Supported tracing providers:

| Provider | Behavior |
|----------|----------|
| `none` | File logging only |
| `console` | File logging plus console logs |
| `phoenix` | Accepted config value, but currently only emits a warning and falls back to file logging |

## Skills

Skills are loaded from `skills/<skill-name>/SKILL.md`.

Current runtime use of `SKILL.md` is intentionally small:

- `name`
- `description`
- the `Tools:` block

Supported `Tools:` forms:

```text
Tools:
  - name: Retrieve Emails
    slug: retrieve_emails
    description: Fetch emails from a folder.
    script: scripts/retrieve_emails.py
```

```text
Tools:
  retrieve_emails: scripts/retrieve_emails.py
```

```text
Tools:
  - scripts/retrieve_emails.py
```

Skill tool fields currently recognized:

| Field | Notes |
|-------|-------|
| `name` | Optional display name |
| `slug` | Internal identifier |
| `description` | Optional model-facing description |
| `script` | Relative path to a local executable script |
| `server` | Marks the tool as MCP-backed metadata rather than a locally executable script |
| `network` | Optional boolean; declares that the tool needs network access |
| `filesystem` | Optional; one of `none`, `read_only`, `read_write` |

Current limitations:

- Only script-backed skill tools are locally executable.
- Skill Markdown body text is not injected into the model.
- MCP-backed skill entries are metadata only.
- The agent is expected to execute script-backed skills through `local__run_skill`.
- Permission checks are policy enforcement, not OS-level sandboxing of child processes.

## Recipes

Recipes are loaded from `recipes/<recipe-name>/RECIPE.md`.

Recognized frontmatter:

| Key | Notes |
|-----|-------|
| `name` | Machine-readable identifier |
| `title` | Optional UI label |
| `description` | Optional summary |
| `keywords` | Optional search tags |

Recognized body sections:

| Section | Notes |
|---------|-------|
| `Instructions:` | Injected into the system prompt |
| `Initial Prompt:` | Loaded into the TUI input box when the recipe is activated |
| `Config:` | Currently only `mcp_servers` is enforced |
| `Response Template:` | Plain text wrapper using `{{ recipe_title }}` and `{{ response }}` |

Recipes do not currently restrict built-in local tools.
Recipes are prompt/configuration assets, not executable scripts.

## Conversations And Evidence

Each conversation lives under `data/conversations/<conversation-id>/`.

Stored files:

- `conversation.json`: messages, timestamps, recipe pointer, MCP filter, compaction pointer
- `logs/conversation.log`: per-conversation audit log
- `tool_output/*.txt`: captured tool outputs
- `compactions/*.json`: stored compaction summaries

Conversation IDs are validated ASCII identifiers before any filesystem access.

## TUI Commands

| Command | Behavior |
|---------|----------|
| `/new` | Create a conversation |
| `/list` | List conversations |
| `/use <id>` | Switch conversation |
| `/show [id]` | Switch to and display a conversation |
| `/delete <id>` | Delete a conversation |
| `/login <server>` | Start MCP OAuth login |
| `/compact` | Compact current conversation |
| `/recipes` | List recipes |
| `/recipe use <name>` | Activate a recipe |
| `/recipe show <name>` | Show recipe instructions |
| `/recipe clear` | Clear active recipe |
| `/mcp` or `/mcp status` | List configured MCP servers and current enabled/disabled state |
| `/mcp reset|enable|disable|only ...` | Manage per-conversation MCP filter |
| `/permissions` | Show active agent permissions |
| `/permissions network on|off` | Toggle networked tool access |
| `/permissions fs none|read|write` | Set filesystem access level |
| `/permissions yolo on|off` | Toggle YOLO mode |
| `/permissions reset` | Reset permissions to config defaults |
| `/yolo on|off` | Shortcut for toggling YOLO mode |
| `/model` | Show current model note |
| `/logging` | Show current logging note |
| `/exit` or `/quit` | Exit the TUI |

## Web API

Routes currently exposed by the Axum server:

| Method | Path |
|--------|------|
| `GET` | `/healthz` |
| `GET` | `/api/conversations` |
| `POST` | `/api/conversations` |
| `GET` | `/api/conversations/{id}` |
| `DELETE` | `/api/conversations/{id}` |
| `POST` | `/api/conversations/{id}/messages` |
| `POST` | `/api/conversations/{id}/compact` |
| `POST` | `/api/conversations/{id}/recipe` |
| `DELETE` | `/api/conversations/{id}/recipe` |
| `GET` | `/api/conversations/{id}/mcp-servers` |
| `PUT` | `/api/conversations/{id}/mcp-servers` |
| `DELETE` | `/api/conversations/{id}/mcp-servers` |
| `GET` | `/api/recipes` |
| `GET` | `/api/jobs/{job_id}` |
| `DELETE` | `/api/jobs/{job_id}` |

Web jobs are transient delivery state for the browser UI. Completed jobs are
retained in memory for up to one hour unless deleted earlier by the client.

## Built-in Local Tools

Always advertised local tools:

- `local__sleep`
- `local__remember_job`
- `local__get_job`
- `local__list_jobs`
- `local__forget_job`
- `local__run_skill`

`local__run_skill` executes a local script from the selected skill definition.
Job memory tools require filesystem read/write permission depending on the operation.

## MCP Behavior

If MCP servers are configured, rusty-bidule lists tools from them at turn time and
advertises those tools to the model. Per-conversation MCP filtering can come from:

- the active conversation state
- a recipe `Config: mcp_servers:`

`enabled_mcp_servers = None` means every configured server is active.
`enabled_mcp_servers = []` means all configured servers are filtered out.

If no MCP servers are configured, the application still runs with local tools only.
If you want local-only execution, omit `mcp_servers`, set it to `[]`, or set the
conversation MCP filter to an empty list.
If agent network permission is disabled, MCP tools are hidden from the model and
MCP tool calls are rejected unless YOLO mode is enabled.

## Running

```bash
# Interactive TUI
cargo run

# Web UI
cargo run -- --interface web --port 8080

# One-shot turn
cargo run -- --once "Summarize the latest findings"
```
