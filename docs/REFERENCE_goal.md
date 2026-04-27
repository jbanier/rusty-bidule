# bidule2 Reference

`bidule2` is an investigation assistant for CSIRT workflows. It combines:

- an Azure OpenAI-backed agent,
- MCP servers that expose security tools,
- local tools for job tracking and skill execution,
- a filesystem-backed conversation store,
- and CLI/web interfaces over the same orchestrator.

This document is the engineering and architecture reference for the repository as it exists today.

## Table Of Contents

1. [Repository Layout](#repository-layout)
2. [Runtime Architecture](#runtime-architecture)
3. [Execution Model](#execution-model)
4. [Configuration Model](#configuration-model)
5. [Conversations, Evidence, And Jobs](#conversations-evidence-and-jobs)
6. [Skills](#skills)
7. [Recipes](#recipes)
8. [Local Tools](#local-tools)
9. [MCP Integration](#mcp-integration)
10. [Authentication And OAuth](#authentication-and-oauth)
11. [CLI Interface](#cli-interface)
12. [Web Interface And API](#web-interface-and-api)
13. [Tracing And Logging](#tracing-and-logging)
14. [Operational Notes](#operational-notes)

## Repository Layout

```text
bidule2/
├── bidule2/
│   ├── __main__.py              # click entrypoint
│   ├── agent/                   # orchestrator, agent bootstrap, skills, recipes
│   ├── config/                  # validated config loader and model helpers
│   ├── data/                    # filesystem conversation store
│   ├── interfaces/              # CLI and Flask web interface
│   ├── mcp_support/             # MCP client abstractions
│   ├── auth.py                  # OAuth state and flows for MCP servers
│   ├── auto_pull.py             # scheduler for remembered long-running jobs
│   ├── logging_config.py        # process-level logging setup
│   └── paths.py                 # canonical repository paths
├── config/
│   ├── config.example.yaml
│   └── config.yaml              # local working config, not normally committed
├── data/
│   ├── conversations/
│   └── oauth_tokens/
├── docs/
│   └── REFERENCE.md
├── recipes/
├── skills/
├── tests/
└── README.md
```

Important runtime paths from [bidule2/paths.py](/Users/jbanier/Documents/work/code/bidule2/bidule2/paths.py):

- `CONFIG_DIR = ROOT_DIR / "config"`
- `DATA_DIR = ROOT_DIR / "data"`
- `CONVERSATIONS_DIR = DATA_DIR / "conversations"`
- `OAUTH_TOKENS_DIR = DATA_DIR / "oauth_tokens"`
- `LOG_DIR = ROOT_DIR / "logs"`
- `SKILLS_DIR = ROOT_DIR / "skills"`
- `RECIPES_DIR = ROOT_DIR / "recipes"`

## Runtime Architecture

The runtime is centered on `AgentOrchestrator` in [bidule2/agent/orchestrator.py](/Users/jbanier/Documents/work/code/bidule2/bidule2/agent/orchestrator.py).

Main components:

- `ConversationStore`: persists conversations, logs, compactions, and remembered jobs.
- `SkillRepository`: loads Agent Skills from `.agents/skills/` plus the legacy bundled `skills/*/SKILL.md`.
- `RecipeRepository`: loads `recipes/*/RECIPE.md`.
- `MCPAuthManager`: resolves static headers or OAuth-derived headers for MCP servers.
- OpenAI Agents SDK agent: drives tool calling and response generation.
- `ToolExecutor`: runs skill scripts or MCP-backed skill tools and saves outputs.
- `AutoPullRuntime`: background poller for remembered long-running jobs.

High-level flow:

1. The process starts from [bidule2/__main__.py](/Users/jbanier/Documents/work/code/bidule2/bidule2/__main__.py).
2. Logging is initialized.
3. The selected interface creates or reuses an `AgentOrchestrator`.
4. The orchestrator loads config, skills, recipes, local tools, and MCP metadata.
5. Each conversation turn is persisted to disk before and after agent execution.
6. Tool output is saved as evidence under the conversation directory.

## Execution Model

### CLI path

The CLI in [bidule2/interfaces/cli.py](/Users/jbanier/Documents/work/code/bidule2/bidule2/interfaces/cli.py):

- starts `AutoPullRuntime`,
- picks the latest conversation by default when one exists,
- dispatches slash commands locally,
- and sends non-command input to `AgentOrchestrator.run_turn()`.

The CLI is synchronous from the user's perspective, but the orchestrator still records timing and tool metadata for every assistant reply.

### Web path

The Flask app in [bidule2/interfaces/web.py](/Users/jbanier/Documents/work/code/bidule2/bidule2/interfaces/web.py):

- creates one orchestrator instance,
- starts `AutoPullRuntime`,
- and executes message and compaction requests as background `WebRunJob`s.

This means the web API is job-oriented:

- `POST /api/conversations/<id>/messages` returns `202 Accepted` with a `job_id`
- `POST /api/conversations/<id>/compact` also returns a `job_id`
- `GET /api/jobs/<job_id>` returns progress events and the final conversation record

Only one running web job is allowed per conversation at a time.

### Agent turn lifecycle

For a normal message turn:

1. Persist the user message to `conversation.json`.
2. Append a `turn_started` entry to `logs/conversation.log`.
3. Build the input context from messages, compaction state, selected recipe, and enabled MCP servers.
4. Prepare MCP server connections and authentication.
5. Run the OpenAI Agents SDK loop.
6. Save every tool result as evidence.
7. Persist the assistant reply with metadata:
   - `assistant_index`
   - `tool_call_count`
   - `timing.tool_seconds`
   - `timing.llm_seconds`
   - `timing.total_seconds`

The orchestrator caps the agent loop at `_MAX_TURNS = 10`.

## Configuration Model

The validated configuration model is defined in [bidule2/config/loader.py](/Users/jbanier/Documents/work/code/bidule2/bidule2/config/loader.py). The active file is `config/config.yaml`.

Top-level keys:

| Key | Type | Notes |
| --- | --- | --- |
| `prompt` | string | Base system prompt |
| `mcp_servers` | list | Remote MCP definitions |
| `mcp_runtime` | object | MCP connection lifecycle settings |
| `azure_openai` | object or null | Required for normal inference |
| `tracing` | object | Tracing backend settings |

### `mcp_servers[]`

Fields:

| Field | Type | Notes |
| --- | --- | --- |
| `name` | string | Unique identifier |
| `transport` | string | Common values: `streamable_http`, `sse`, `stdio` |
| `url` | URL | MCP endpoint |
| `headers` | map | Static headers merged with auth-derived headers |
| `timeout` | float or null | Transport timeout |
| `sse_read_timeout` | float or null | SSE read timeout |
| `client_session_timeout_seconds` | float or null | MCP client session timeout |
| `auth` | object | `static_headers` or `oauth_public` |

### `auth`

`MCPAuthConfig` supports:

- `type`
- `client_id`
- `client_secret`
- `token_endpoint_auth_method`
- `scopes`
- `authorization_endpoint`
- `token_endpoint`
- `registration_endpoint`
- `resource`
- `redirect_uri`
- `redirect_host`
- `redirect_port`
- `redirect_path`
- `callback_timeout_seconds`
- `open_browser`
- `use_dynamic_client_registration`
- `client_name`

### `mcp_runtime`

Fields:

- `connect_timeout_seconds`
- `cleanup_timeout_seconds`
- `connect_in_parallel`

### `azure_openai`

Fields:

- `api_key`
- `api_version`
- `endpoint`
- `deployment`
- `temperature`
- `top_p`
- `max_output_tokens`

If `azure_openai` is absent, the orchestrator still runs but returns a fallback assistant message instead of performing LLM inference.

### `tracing`

Fields:

- `provider`
- `phoenix_endpoint`
- `phoenix_project`

## Conversations, Evidence, And Jobs

The filesystem store is implemented in [bidule2/data/storage.py](/Users/jbanier/Documents/work/code/bidule2/bidule2/data/storage.py).

### Conversation directory layout

Each conversation gets a folder under `data/conversations/`:

```text
data/conversations/convo-20260402153000-1a2b3c4d/
├── conversation.json
├── job_state.json
├── logs/
│   └── conversation.log
├── tool_output/
└── compactions/
```

The conversation ID format is timestamp plus a short random suffix:

```text
convo-YYYYMMDDHHMMSS-<8 hex chars>
```

### `conversation.json`

Stored fields:

- `conversation_id`
- `created_at`
- `updated_at`
- `messages`
- `pending_recipe`
- `enabled_mcp_servers`
- `active_compaction`

`messages[]` contains:

- `role`
- `content`
- `timestamp`
- optional `metadata`

Assistant metadata includes:

- `assistant_index`
- `tool_call_count`
- `timing`

### Evidence files

There are two evidence-save paths in the code:

- `save_tool_evidence()` in [bidule2/agent/blocks.py](/Users/jbanier/Documents/work/code/bidule2/bidule2/agent/blocks.py) for agent-executed tool calls
- `run_tool_and_save_output()` in the same file for skill execution results

Current behavior:

- outputs are saved as `.txt` files, not `.json`
- filenames contain a sanitized tool name plus a microsecond timestamp
- the per-conversation log receives a matching `tool_call:` audit entry

### Conversation log

`logs/conversation.log` stores:

- structured agent events via `append_log_entry()`
- plain message append lines
- `tool_call:` entries with serialized arguments

This log is append-only in normal operation and is the fastest audit artifact to inspect during debugging.

### Compactions

Compaction checkpoints live under `compactions/` and are referenced by `active_compaction` in `conversation.json`.

Compaction is triggered by:

- CLI: `/compact`
- Web API: `POST /api/conversations/<id>/compact`

The compaction logic is owned by the orchestrator, not by the interfaces.

### Remembered jobs and auto-pull

Long-running remote jobs are stored in `job_state.json`.

The local job tools support:

- manual tracking of remote transaction IDs
- automated follow-up when `mode` is set for auto-pull

`AutoPullRuntime` in [bidule2/auto_pull.py](/Users/jbanier/Documents/work/code/bidule2/bidule2/auto_pull.py):

- scans due jobs every second by default
- claims a per-job lock in the store
- runs `orchestrator.run_automation_turn(...)`
- updates retrieval state or last error

This is how bidule2 can continue polling a remote system after the original user turn has finished.

## Skills

Skills use the Agent Skills `SKILL.md` format. Rusty Bidule scans user-level
cross-client skill directories and keeps `skills/<name>/SKILL.md` as the
bundled legacy repo location. Project-level `.agents/.claude/.rusty-bidule`
skill directories are skipped unless the project root is trusted or the skill
policy is set to `always`.

The skill repository supports three practical modes:

- guidance-only skills with instructions and no executable tools
- script-backed skills with one or more tools in `scripts/`
- MCP-backed skills that map a skill tool to a remote MCP server tool

### Skill structure

Typical layout:

```text
skills/webex-room-conversation/
├── SKILL.md
└── scripts/
    ├── webex_auth.py
    └── webex_room_message_fetch.py
```

### Expected `SKILL.md` content

Important frontmatter keys:

- `name`
- `description`
- `compatibility`
- `metadata`
- `allowed-tools`

Recognized body sections include:

- `## When to use`
- `## Constraints`
- `## Authentication setup`

A `Tools:` block can define executable tools in three styles:

- full YAML object list
- shorthand `slug: path`
- shorthand path list

The agent sees a compact skill catalog first. `local__activate_skill` loads the
full `SKILL.md` body and resource listing when a skill matches the task, while
`local__run_skill` remains the execution path for script-backed tools.
Activated skill content is stored per conversation and restored after
compaction.

### Script execution contract

When a skill tool points to `script: ...`, `ToolExecutor`:

1. resolves the script relative to the skill root
2. prevents path escape outside the skill directory
3. selects an interpreter by file suffix
4. converts payload keys into CLI flags

Conversion rules:

- `_internal` keys are ignored
- `snake_case` becomes `--snake-case`
- boolean `true` becomes a bare flag
- other values become `--flag value`

The script is expected to write useful output to stdout. That raw stdout is saved as evidence.

### MCP-backed skill tools

When a skill tool uses `server` plus `tool`, the executor routes it through the MCP client layer instead of a local script. The result is still saved in the conversation evidence directory.

### `run_skill`

The agent-facing local tool is `run_skill(skill_name, parameters)`.

Current behavior:

- exact skill-name match first
- fuzzy match fallback through the skill repository
- all tools defined for the matched skill are executed
- result text returned to the agent is truncated to 8000 characters per saved output preview

## Recipes

Recipes are stored under `recipes/<name>/RECIPE.md`.

They are parsed into structured fields that can affect only the next turn.

### Recipe sections

Important parts:

- frontmatter: metadata such as `name`, `title`, `description`, `keywords`
- `Instructions:`
- `Initial Prompt:`
- `Config:`
- `Response Template:`

### What recipes can change

A recipe can:

- inject instructions into the next turn
- prefill an initial user prompt
- restrict MCP servers for the next turn
- restrict local tools for the next turn
- wrap the final assistant response in a Jinja2 template

Recipes are selected per conversation and then consumed by the next normal turn.

## Local Tools

Local tools are defined in [bidule2/agent/local_tools.py](/Users/jbanier/Documents/work/code/bidule2/bidule2/agent/local_tools.py).

Base local tools:

- `sleep`
- `remember_job`
- `update_job`
- `get_job`
- `list_jobs`
- `forget_job`

Conditionally available tools:

- `run_skill`
- `configure_mcp_servers`

### `sleep`

Simple bounded pause:

- max 300 seconds
- returns actual elapsed duration

### Job tools

These tools persist remote-job state into the current conversation store.

Important fields supported across `remember_job` and `update_job` include:

- `alias`
- `transaction_id`
- `source_tool`
- `status`
- `notes`
- `mode`
- `poll_interval_seconds`
- `next_poll_at`
- `lease_expires_at`
- `result_expires_at`
- `automation_prompt`
- `retrieval_state`
- `result_artifacts_json`
- `last_error`

### `configure_mcp_servers`

This tool is only present when an MCP selection controller is supplied. It allows the agent to change conversation-scoped MCP availability with one of:

- `enable`
- `disable`
- `only`
- `reset`

The change affects subsequent turns, not the currently running tool set.

## MCP Integration

bidule2 uses MCP in two related ways:

1. the main agent can call MCP-exposed tools directly
2. skills can route specific skill tools through an MCP server

The lower-level MCP helper code lives in [bidule2/mcp_support/client.py](/Users/jbanier/Documents/work/code/bidule2/bidule2/mcp_support/client.py).

### Startup and discovery

At startup or refresh:

- configured MCP connections are built
- tool catalogs are discovered
- MCP tool schemas are converted into OpenAI function definitions
- a routing map is built from agent-visible tool name to `(server_name, mcp_tool_name)`

If multiple servers expose the same MCP tool name, the generated OpenAI function name is prefixed with the server name slug to avoid collisions.

### Per-conversation server selection

Each conversation may carry `enabled_mcp_servers` in `conversation.json`.

This can be changed through:

- CLI `/mcp ...`
- web API `/api/conversations/<id>/mcp-servers`
- recipe config overrides for the next turn
- agent-side `configure_mcp_servers`

When no override exists, all configured MCP servers are considered enabled.

### Error handling

The orchestrator catches MCP preparation and run failures and persists a user-visible assistant error message rather than crashing the process. MCP connection failures are also logged as `mcp_connection_failed` events in the conversation log.

## Authentication And OAuth

OAuth support is implemented in [bidule2/auth.py](/Users/jbanier/Documents/work/code/bidule2/bidule2/auth.py).

### Storage

OAuth registration and token state is persisted under:

```text
data/oauth_tokens/<server-name>.json
```

`OAuthStateStore.save()` attempts to apply mode `0600` on those files.

### CLI OAuth flow

For CLI-driven OAuth:

- bidule2 can open the system browser
- a temporary loopback HTTP listener receives the callback
- PKCE verifier/state are used during the flow
- refresh tokens are reused when available

CLI login can be initiated explicitly with `/login [server]` or triggered automatically when an unauthenticated OAuth-backed server is first needed.

### Web OAuth flow

The web API exposes:

- `GET /api/mcp/oauth-servers`
- `POST /api/mcp/oauth-servers/<server_name>/start`
- `GET /oauth/callback/<server_name>`

The web route starts the authorization URL generation, then the callback completes token exchange and renders an HTML result page.

## CLI Interface

The CLI command surface in [bidule2/interfaces/cli.py](/Users/jbanier/Documents/work/code/bidule2/bidule2/interfaces/cli.py) includes:

- `/help`
- `/list`
- `/new`
- `/use <id>`
- `/show [id]`
- `/history`
- `/delete <id>`
- `/login [server]`
- `/model [choice]`
- `/mcp`
- `/mcp enable <name...>`
- `/mcp disable <name...>`
- `/mcp only <name...>`
- `/mcp reset`
- `/compact`
- `/jobs`
- `/recipes`
- `/recipe use <name>`
- `/recipe show <name>`
- `/recipe clear`
- `/logging [LEVEL]`
- `/exit`
- `/quit`

Behavior notes:

- multiline input is delimited by `<<<` and `>>>`
- session-only input history is enabled when `readline` is available
- `/model` fetches models live from the Azure endpoint and can persist a selected deployment

## Web Interface And API

The Flask app is created by `create_app()` in [bidule2/interfaces/web.py](/Users/jbanier/Documents/work/code/bidule2/bidule2/interfaces/web.py).

### Safety behavior

`run_web()` refuses to bind the Flask development server to a non-loopback host unless `BIDULE_ALLOW_UNSAFE_WEB=1` is set. This is an intentional safety guard around an otherwise unprotected development server.

### Routes

UI and health:

- `GET /`
- `GET /healthz`

Config:

- `GET /api/config`
- `PUT /api/config`

Conversations:

- `GET /api/conversations`
- `POST /api/conversations`
- `GET /api/conversations/<conversation_id>`
- `DELETE /api/conversations/<conversation_id>`

Execution:

- `POST /api/conversations/<conversation_id>/messages`
- `POST /api/conversations/<conversation_id>/compact`
- `GET /api/jobs/<job_id>`

Recipes and MCP selection:

- `GET /api/recipes`
- `POST /api/conversations/<conversation_id>/recipe`
- `DELETE /api/conversations/<conversation_id>/recipe`
- `GET /api/conversations/<conversation_id>/mcp-servers`
- `PUT /api/conversations/<conversation_id>/mcp-servers`
- `DELETE /api/conversations/<conversation_id>/mcp-servers`

Remembered jobs:

- `GET /api/conversations/<conversation_id>/jobs`

OAuth:

- `GET /api/mcp/oauth-servers`
- `POST /api/mcp/oauth-servers/<server_name>/start`
- `GET /oauth/callback/<server_name>`

### Route validation

The web layer validates:

- conversation ID pattern
- job ID pattern
- recipe and server name pattern
- config payload shape via Pydantic validation

API errors are normalized into JSON for `/api/*` routes.

## Tracing And Logging

Tracing setup is controlled through the orchestrator and [bidule2/agent/tracing_setup.py](/Users/jbanier/Documents/work/code/bidule2/bidule2/agent/tracing_setup.py).

Supported providers:

- `none`
- `console`
- `phoenix`

Logging exists at two levels:

- process-level logs under `logs/bidule.log`
- per-conversation audit logs under `data/conversations/<id>/logs/conversation.log`

The per-conversation log is the better source for reconstructing what happened in a specific investigation.

## Operational Notes

- The repository is alpha-quality and intentionally filesystem-centric.
- The web server is Flask development server based, not a production deployment target.
- Tool outputs are preserved as raw text artifacts, which is useful for auditability but can create large conversation directories over time.
- The orchestrator is designed to stay resilient when MCP or Azure configuration is incomplete; degraded behavior is preferred over process crashes.
- The most important alignment point when changing the system is keeping the store schema, orchestrator behavior, and documentation in sync. Much of the product behavior is encoded in those three places rather than behind a separate API contract layer.
