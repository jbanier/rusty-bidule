# rusty-bidule Reference

This document is the engineering reference for `rusty-bidule`. It describes
how the application is composed, how a turn is executed, what gets persisted,
and which interfaces and configuration surfaces exist today.

## System Overview

`rusty-bidule` is a single-process Rust application with three operator-facing
modes:

- TUI mode through Ratatui and Crossterm
- Web mode through Axum plus a small browser UI
- One-shot CLI mode for a single turn

All three modes share the same orchestration core:

1. Load YAML configuration
2. Initialize logging
3. Construct the orchestrator and persistent stores
4. Accept a user message through TUI, web, or `--once`
   - Before dispatch, preprocess inline `@file` references in the user text
5. Discover tools, call Azure OpenAI, execute tools, and persist results

## High-Level Architecture

Core runtime components:

- `src/main.rs`: CLI parsing, config loading, logging bootstrap, interface selection
- `src/orchestrator.rs`: turn execution, tool advertisement, tool loop, recipe injection, compaction
- `src/azure.rs`: Azure OpenAI request/response handling
- `src/mcp_runtime.rs`: MCP server management, tool discovery, tool invocation, OAuth-aware connectivity
- `src/local_tools.rs`: built-in local tool definitions and execution
- `src/skills.rs`: skill loading from `skills/*/SKILL.md`
- `src/recipes.rs`: recipe loading from `recipes/*/RECIPE.md`
- `src/conversation_store.rs`: persisted conversation state and audit logs
- `src/tool_evidence.rs`: captured tool output persistence
- `src/oauth.rs`: OAuth public-client flow support for MCP servers
- `src/ui.rs`: terminal interface and slash-command handling
- `src/web.rs`: browser UI hosting, REST API, and async job registry
- `src/types.rs`: shared domain types such as conversations, permissions, and progress events
- `src/logging.rs`: file and optional console logging setup

## Runtime Boundaries

There is no separate worker service, database, or message broker.

- Persistence is filesystem-backed under `data/` and `var/`
- Concurrency is in-process with Tokio tasks
- Per-conversation turns are serialized with conversation-level async locks
- Web message submission is asynchronous and tracked through an in-memory job registry

This makes the project easy to run locally, but it also means there is no
cross-process coordination or hard isolation boundary between tool execution and
the main app.

## Turn Lifecycle

The main turn path lives in `Orchestrator::run_turn`.

1. Acquire the per-conversation lock
2. Persist the incoming user message
3. Load conversation state, including pending recipe, MCP filter, and agent permissions
4. Resolve the effective MCP filter
5. Discover MCP tools if network access is allowed
6. Build local tool definitions
7. Merge local and MCP tools into the Azure tool list, truncating if needed
8. Build the prompt stack from:
   - base config prompt
   - conversation history
   - active recipe instructions
   - MCP degradation notes
   - skill capability summary
   - active permission summary
   - optional compaction context
9. Call Azure OpenAI
10. Execute any requested local or MCP tool calls
11. Persist tool evidence and append audit logs
12. Repeat the Azure/tool loop until a final assistant reply or iteration limit
13. Persist the assistant reply with timing and tool metadata

Current hard limits:

- Maximum agent iterations per turn: `10`
- Maximum tools advertised to Azure: `128`

If MCP discovery fails, the application degrades rather than aborting the turn.
The failure is surfaced to both logs and the model context.

## Prompt Composition

Before a normal user turn is recorded, the app preprocesses prompt text for
inline file references:

- `@path/to/file.md` reads a local text file and replaces the token inline
- `\@path/to/file.md` keeps a literal `@...` without expansion
- relative paths resolve from the detected project root
- expansion requires conversation filesystem read permission
- unresolved or unreadable references fail the submission before the turn runs

The effective system context is assembled dynamically. Inputs can include:

- `prompt` from configuration
- recipe instructions from the active `RECIPE.md`
- skill capability summary from loaded skills
- current permission summary
- notes about degraded MCP availability
- historical messages and optionally an active compaction checkpoint

The app does not inject arbitrary skill Markdown into the model. Skills are
reduced to structured capability metadata.

Recipes are instruction bundles, not a workflow runner. They can constrain
available MCP/local tools and supply reusable investigation guidance, but they
do not currently provide deterministic branching, stateful step outputs, or
hard guarantees that the model will execute every described step.

## Configuration Model

Default config discovery:

- default path: `config/config.local.yaml`
- search behavior: discover project root by walking upward from the current working directory
- override: `--config <PATH>` or `RUSTY_BIDULE_CONFIG`

Top-level keys:

| Key | Required | Notes |
|-----|----------|-------|
| `azure_openai` | yes | Azure endpoint, deployment, API version, and key |
| `prompt` | no | Extra system prompt text |
| `data_dir` | no | Defaults to `data` |
| `agent_permissions` | no | Default permissions applied to new conversations |
| `mcp_runtime` | no | Shared MCP timeout configuration |
| `mcp_servers` | no | List of configured MCP servers |
| `tracing` | no | Logging/tracing mode |

### Secret Resolution

Selected string fields support `env:VARNAME` indirection. Resolution happens
when configuration is loaded.

Current resolution targets:

- `azure_openai.api_key`
- `azure_openai.endpoint`
- `mcp_servers[].url`
- `mcp_servers[].headers[*]`
- `mcp_servers[].auth.client_id`
- `mcp_servers[].auth.client_secret`
- `mcp_servers[].auth.redirect_uri`
- `mcp_servers[].auth.resource`

### Azure OpenAI

`azure_openai` fields:

| Field | Required | Notes |
|-------|----------|-------|
| `api_key` | yes | Supports `env:` |
| `api_version` | yes | Azure API version string |
| `endpoint` | yes | Supports `env:` |
| `deployment` | yes | Azure deployment/model name |
| `temperature` | no | Default `0.2` |
| `top_p` | no | Default `1.0` |
| `max_output_tokens` | no | Default `1200` |

### Agent Permissions

`agent_permissions` fields:

| Field | Notes |
|-------|-------|
| `allow_network` | Enables networked tool execution such as MCP calls |
| `filesystem` | One of `none`, `read_only`, `read_write` |
| `yolo` | Bypasses internal permission checks |

Permission defaults are copied into each new conversation. In the TUI, the
conversation can then diverge from config defaults.

### MCP Runtime

`mcp_runtime` fields:

| Field | Notes |
|-------|-------|
| `connect_timeout_seconds` | Shared connection timeout for MCP server contact |

### MCP Servers

`mcp_servers[]` fields:

| Field | Required | Notes |
|-------|----------|-------|
| `name` | yes | Stable server identifier |
| `transport` | yes | `streamable_http` or `sse` |
| `url` | yes | Supports `env:` |
| `headers` | no | Extra request headers, values support `env:` |
| `timeout` | no | Per-server request timeout |
| `sse_read_timeout` | no | Per-server SSE read timeout |
| `client_session_timeout_seconds` | no | Per-server client session timeout |
| `auth` | no | OAuth configuration |

Supported auth config:

| `auth.type` | Meaning |
|-------------|---------|
| `oauth_public` | Browser-based OAuth public-client flow |

`oauth_public` fields:

| Field | Notes |
|-------|-------|
| `scopes` | Requested scopes |
| `client_id` | Required if dynamic registration is disabled |
| `client_secret` | Optional |
| `token_endpoint_auth_method` | Defaults to `none` |
| `resource` | Optional resource/audience |
| `redirect_uri` | Required callback URI |
| `redirect_host` | Optional callback host override |
| `redirect_port` | Optional callback port override |
| `redirect_path` | Optional callback path override |
| `callback_timeout_seconds` | Defaults to `300` |
| `open_browser` | Defaults to `true` |
| `use_dynamic_client_registration` | Enables DCR flow |

### Tracing

Supported tracing providers:

| Provider | Behavior |
|----------|----------|
| `none` | File logging only |
| `console` | File logging plus console logs |
| `phoenix` | Accepted config value, currently warns and falls back to file logging |

## Conversation Model

Each conversation is persisted as a structured object with:

- `conversation_id`
- `created_at`
- `updated_at`
- `pending_recipe`
- `enabled_mcp_servers`
- `active_compaction`
- `agent_permissions`
- `messages`

Assistant messages carry metadata for:

- assistant reply index
- cumulative tool execution seconds
- LLM inference seconds
- total turn duration
- tool call count

`enabled_mcp_servers` semantics:

- `None`: all configured MCP servers are active
- `Some([])`: all configured MCP servers are filtered out
- `Some(["name-a", "name-b"])`: only those configured servers are active

Conversation IDs are validated ASCII identifiers before filesystem access.

## Persistence Layout

Expected filesystem layout:

```text
rusty-bidule/
├── src/
├── config/config.example.yaml
├── skills/<skill-name>/SKILL.md
├── recipes/<recipe-name>/RECIPE.md
├── data/
│   ├── conversations/<conversation-id>/
│   └── oauth/
└── var/bidule.log
```

Per-conversation data lives under `data/conversations/<conversation-id>/`.

Stored artifacts:

- `conversation.json`: canonical conversation state
- `logs/conversation.log`: per-conversation audit trail
- `tool_output/*.txt`: captured tool outputs
- `compactions/*.json`: saved compaction summaries

OAuth state is stored separately under `data/oauth/`.

## Skills

Skills are loaded from `skills/<skill-name>/SKILL.md`.

Current runtime use of `SKILL.md` is intentionally narrow:

- skill name
- skill description
- `Tools:` block

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

Recognized skill tool fields:

| Field | Notes |
|-------|-------|
| `name` | Optional display name |
| `slug` | Internal identifier |
| `description` | Optional model-facing description |
| `script` | Relative path to a local executable script |
| `server` | Metadata for MCP-backed capability entries |
| `network` | Optional boolean capability hint |
| `filesystem` | Optional; `none`, `read_only`, or `read_write` |

Current limitations:

- Only script-backed skill tools are locally executable
- Skill body Markdown is not injected into the prompt
- MCP-backed skill entries are metadata only
- Permission checks are policy checks, not OS sandboxing of child processes

## Recipes

Recipes are loaded from `recipes/<recipe-name>/RECIPE.md`.

Recognized frontmatter:

| Key | Notes |
|-----|-------|
| `name` | Machine-readable identifier |
| `title` | Optional UI label |
| `description` | Optional summary |
| `keywords` | Optional search tags |

Recognized sections:

| Section | Notes |
|---------|-------|
| `Instructions:` | Injected into the system prompt |
| `Initial Prompt:` | Loaded into the TUI input box on activation |
| `Config:` | Currently enforces `mcp_servers` |
| `Response Template:` | Plain text wrapper using `{{ recipe_title }}` and `{{ response }}` |

Recipes are prompt/configuration assets, not executable scripts. They do not
currently restrict built-in local tools.

## Built-in Local Tools

Always-advertised local tools:

- `local__sleep`
- `local__remember_job`
- `local__get_job`
- `local__list_jobs`
- `local__forget_job`
- `local__time`
- `local__exec_cli`
- `local__run_skill`

Behavior notes:

- `local__run_skill` executes a local script from a selected skill definition
- `local__exec_cli` executes only config-allowlisted binary names with direct argv execution
- `local__time` provides current UTC/local time and relative window calculations for prompt grounding
- job memory tools depend on filesystem access
- local tools are advertised even when MCP is disabled

## MCP Behavior

If MCP servers are configured and network access is allowed, the app discovers
tools at turn time and advertises them to Azure.

Effective MCP server selection can come from:

- the conversation state
- a recipe `Config: mcp_servers:`

The recipe filter takes precedence over the conversation filter.

If network access is disabled:

- MCP tools are hidden from the model
- MCP tool execution is rejected unless YOLO mode is enabled

If no MCP servers are configured, the app continues with local tools only.

## Interfaces

### CLI

Supported arguments:

| Option | Meaning |
|--------|---------|
| `--config <PATH>` | Override config path |
| `--interface <tui|web>` | Select interface |
| `--host <HOST>` | Web bind address |
| `--port <PORT>` | Web port |
| `--once <MESSAGE>` | Run one turn and exit |
| `--conversation <ID>` | Conversation to use with `--once` |

### TUI Commands

| Command | Behavior |
|---------|----------|
| `/new` | Create a conversation |
| `/list [all|archived]` | List active, all, or archived conversations |
| `/use <id>` | Switch conversation |
| `/show [id]` | Switch to and display a conversation |
| `/title [text]` | Set or clear the current conversation title |
| `/archive [id]` | Archive the current or specified conversation |
| `/unarchive <id>` | Restore an archived conversation |
| `/export [id]` | Write a local JSON session summary under `data/exports/` |
| `/delete <id>` | Delete a conversation |
| `/login <server>` | Start MCP OAuth login |
| `/compact` | Compact current conversation |
| `/recipes` | List recipes |
| `/recipe use <name>` | Activate a recipe |
| `/recipe show <name>` | Show recipe instructions |
| `/recipe clear` | Clear active recipe |
| `/mcp` or `/mcp status` | List configured MCP servers and current filter state |
| `/mcp reset|enable|disable|only ...` | Manage per-conversation MCP filter |
| `/permissions` | Show active permissions |
| `/permissions network on|off` | Toggle network access |
| `/permissions fs none|read|write` | Set filesystem access |
| `/permissions yolo on|off` | Toggle YOLO mode |
| `/permissions reset` | Reset to config defaults |
| `/yolo on|off` | Shortcut for YOLO toggle |
| `/scratch [show|set|append|clear]` | Manage the per-conversation scratchpad |
| `/findings [list|add|update|remove]` | Manage structured local findings, including tags, confidence, and artifact references |
| `/search <query>` | Search conversations, scratchpads, and findings locally |
| `/logging` | Show current logging note |
| `/exit` or `/quit` | Exit the TUI |

### Web API

Routes currently exposed by the Axum server:

| Method | Path | Notes |
|--------|------|-------|
| `GET` | `/` | Browser UI shell |
| `GET` | `/healthz` | Health check |
| `GET` | `/api/conversations` | List conversations, defaults to active only; `?include_archived=true` includes archived |
| `POST` | `/api/conversations` | Create conversation |
| `GET` | `/api/conversations/{id}` | Load conversation |
| `PUT` | `/api/conversations/{id}` | Update conversation metadata such as the title |
| `DELETE` | `/api/conversations/{id}` | Delete conversation |
| `POST` | `/api/conversations/{id}/archive` | Archive conversation |
| `POST` | `/api/conversations/{id}/unarchive` | Restore conversation |
| `GET` | `/api/conversations/{id}/export-summary` | Generate and return the current JSON session summary |
| `POST` | `/api/conversations/{id}/export-summary` | Generate and save the JSON session summary under `data/exports/` |
| `POST` | `/api/conversations/{id}/messages` | Submit a message, preprocesses inline `@file` references, returns async job id |
| `POST` | `/api/conversations/{id}/compact` | Compact conversation |
| `POST` | `/api/conversations/{id}/recipe` | Set active recipe |
| `DELETE` | `/api/conversations/{id}/recipe` | Clear active recipe |
| `GET` | `/api/conversations/{id}/mcp-servers` | Get effective conversation MCP filter |
| `PUT` | `/api/conversations/{id}/mcp-servers` | Set conversation MCP filter |
| `DELETE` | `/api/conversations/{id}/mcp-servers` | Reset conversation MCP filter |
| `GET` | `/api/conversations/{id}/scratchpad` | Load conversation scratchpad |
| `PUT` | `/api/conversations/{id}/scratchpad` | Save conversation scratchpad |
| `GET` | `/api/conversations/{id}/findings` | List findings for a conversation |
| `POST` | `/api/conversations/{id}/findings` | Add a finding to a conversation |
| `PUT` | `/api/findings/{finding_id}` | Replace finding metadata such as note, tags, confidence, or artifact reference |
| `DELETE` | `/api/findings/{finding_id}` | Remove a stored finding |
| `GET` | `/api/search?q=...` | Search conversations, scratchpads, and findings locally |
| `GET` | `/api/recipes` | List recipes |
| `GET` | `/api/jobs/{job_id}` | Poll async job state |
| `DELETE` | `/api/jobs/{job_id}` | Delete async job state |

Web message delivery is asynchronous. `POST /api/conversations/{id}/messages`
returns `202 Accepted` with a `job_id`; clients then poll `/api/jobs/{job_id}`.

Completed jobs are kept in memory for up to one hour unless deleted earlier.

## Logging And Observability

Primary log destinations:

- `var/bidule.log`: application-level logs
- `data/conversations/<id>/logs/conversation.log`: per-conversation audit trail

One-shot mode also emits progress updates to `stderr`.

## Limitations And Risks

- Tool permission checks are application-level, not OS sandboxing
- Web job state is in-memory and disappears on process restart
- Completed web jobs are TTL-based rather than durably queued
- Large tool inventories are truncated to satisfy Azure limits
- The app is single-process and filesystem-backed, so it is not designed for high-concurrency multi-node deployment
