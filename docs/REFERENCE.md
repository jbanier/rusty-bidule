# rusty-bidule Reference

`rusty-bidule` is a Rust CSIRT investigation client. It combines:

- an Azure OpenAI chat-completions agent,
- optional MCP servers for remote security tools,
- local tools for time, job tracking, skills, and investigation memory,
- Markdown skills and recipes loaded from the repository,
- filesystem-backed conversations, evidence, and OAuth state,
- TUI, one-shot CLI, and web/API interfaces over the same orchestrator.

This document describes the Rust client as implemented in this repository.

## Repository Layout

```text
src/
  main.rs                 process entrypoint
  orchestrator.rs         agent loop, tool routing, prompts, recipe application
  local_tools.rs          built-in local tools and skill execution
  mcp_runtime.rs          MCP discovery and tool execution
  skills.rs               SKILL.md registry and tool metadata
  recipes.rs              RECIPE.md registry and prompt/config overlays
  conversation_store.rs   conversations, scratchpads, findings, jobs, memory
  web.rs                  Axum web server and JSON API
  ui.rs                   terminal UI and slash commands
config/
  config.example.yaml     example runtime config
skills/
  */SKILL.md              skill metadata plus optional local scripts
recipes/
  */RECIPE.md             prompt/config recipes
data/
  conversations/          durable conversation state
  oauth/                  MCP OAuth registration and token state
  findings.json           global finding records
var/
  bidule.log              process log by default
```

`config/config.local.yaml`, `data/`, and `var/` are local runtime state and are not required to exist before first run.

## Runtime Model

The process loads config, initializes `ConversationStore`, loads skills and recipes from the project root, creates an MCP manager, and then starts one of three interfaces:

- TUI: default interactive terminal client.
- One-shot CLI: `--once <message>` sends one prompt and exits.
- Web: `--interface web` starts the Axum API and browser UI.

For each normal turn:

1. The user message is persisted to `conversation.json`.
2. Conversation permissions, pending recipe, MCP filters, local-tool filters, skills, and durable memory are loaded.
3. MCP tools are discovered only when network permission is active.
4. Local tools and a ranked subset of MCP tools are advertised to the model.
5. The model may call tools for up to 10 agent iterations.
6. Tool outputs are saved as evidence.
7. The final assistant reply is stored with timing and tool-call metadata.
8. A pending recipe is cleared after a non-automation turn.

Recipes are prompt guidance and configuration overlays. They are not executable scripts or deterministic workflow engines.

## Configuration

The config model is defined in `src/config.rs`. Main top-level keys:

| Key | Purpose |
| --- | --- |
| `prompt` | Optional base system prompt text. |
| `data_dir` | Runtime data root. Defaults to `data`. |
| `llm_provider` | Provider selector. Supported values are `azure_openai` and `azure_anthropic`. |
| `azure_openai` | Azure OpenAI endpoint, deployment, API key, and sampling settings. |
| `azure_anthropic` | Azure Anthropic endpoint, deployment, API key, and sampling settings. |
| `mcp_servers` | Remote MCP server definitions. |
| `mcp_runtime` | MCP connection timeout and parallelism settings. |
| `local_tools` | Local execution timeout and allowed CLI binaries. |
| `agent_permissions` | Default per-conversation network/filesystem/yolo permissions. |
| `tracing` | Log path and filtering settings. |

Per-conversation permissions can differ from defaults and are stored in each conversation record.

## Persistence

Each conversation lives under `data/conversations/<conversation-id>/`:

```text
conversation.json
scratchpad.md
investigation_memory.json
job_state.json
logs/conversation.log
tool_output/
compactions/
```

The store also keeps:

- `data/findings.json` for global finding records,
- `data/exports/` for exported conversation summaries,
- `data/oauth/` for MCP OAuth token/client state.

### Investigation Memory

Investigation memory is durable structured JSON for case carry-over. It is injected into the system prompt when present and can be managed through local tools.

Stable fields:

- `updated_at`
- `updated_by`
- `summary`
- `entities`
- `timeline`
- `decisions`
- `hypotheses`
- `trusted_sources`
- `unresolved_questions`

`updated_at` and `updated_by` are metadata fields set by the local update tool. `entities`, `timeline`, `decisions`, `hypotheses`, `trusted_sources`, and `unresolved_questions` are arrays of JSON values so skills and agents can store domain-shaped records without schema migrations. Merge updates deduplicate exact JSON array entries, and memory search indexes readable field/value lines rather than compact raw JSON.

## Local Tools

Built-in local tools are advertised as model tools when enabled by the current conversation/recipe:

| Tool | Purpose |
| --- | --- |
| `local__sleep` | Wait between polling operations. |
| `local__time` | Return UTC/local time and relative windows. |
| `local__configure_mcp_servers` | Update the conversation-scoped MCP server filter. |
| `local__exec_cli` | Execute an explicitly allowlisted bare CLI command with direct argv. |
| `local__run_skill` | Execute script-backed skill tools. |
| `local__remember_job` | Store a long-running job or transaction alias. |
| `local__update_job` | Update stored job metadata. |
| `local__get_job` | Retrieve one stored job. |
| `local__list_jobs` | List stored jobs. |
| `local__forget_job` | Remove a stored job. |
| `local__get_investigation_memory` | Return this conversation's durable memory. |
| `local__update_investigation_memory` | Merge or replace this conversation's durable memory. |
| `local__clear_investigation_memory` | Clear this conversation's durable memory. |
| `local__search_conversation_memories` | Search durable memories across conversations. |

Permission checks are enforced before local tools run:

- networked skills and `local__exec_cli` require network permission,
- read-only skill/file operations require filesystem read permission,
- memory/job updates require filesystem write permission,
- `yolo` bypasses these application-level checks.

## Skills

Skills are loaded from `skills/<skill-name>/SKILL.md`. A skill file uses YAML frontmatter plus Markdown sections. Canonical Rust-native shape:

```markdown
---
name: gmail-read
description: Reads Gmail messages from a saved read-only OAuth token.
keywords: gmail, email, inbox
---

# Gmail Read

Tools:
  - name: Read Gmail Messages
    slug: gmail_read_messages
    description: Read Gmail messages matching a query.
    script: scripts/gmail_read_messages.py
    network: true
    filesystem: read_only

## When to use

- Check recent inbox messages.
```

The shared document parser supports frontmatter plus labeled sections such as:

- `Tools:`
- `## When to use`
- `## Constraints`
- `## Authentication setup`
- `## Output`
- `## Edge cases`

Tool metadata fields:

| Field | Notes |
| --- | --- |
| `name` | Human-readable tool name. |
| `slug` | Required local tool selector. |
| `description` | Prompt-facing tool description. |
| `script` | Script path relative to the skill directory. |
| `network` | Boolean network requirement. |
| `filesystem` | `none`, `read_only`, or `read_write`. |
| `mcp` | Metadata-only MCP-backed marker. |
| `server` | Optional MCP server label for metadata-only skills. |

Script-backed tools are executed with:

```json
{
  "skill_name": "gmail-read",
  "tool_slug": "gmail_read_messages",
  "parameters": "{\"query\":\"is:unread newer_than:1d\"}"
}
```

`parameters` is a JSON string. Object keys become CLI flags by converting underscores to hyphens.

Python scripts run via `python3`, falling back to `python`. Non-Python scripts run directly and must be executable.

### Pending Skill Jobs

A skill script can return this JSON envelope to store long-running remote work:

```json
{
  "status": "pending",
  "job": {
    "alias": "splunk-search",
    "transaction_id": "sid-123",
    "status": "running",
    "poll_interval_seconds": 30,
    "automation_prompt": "Poll this job and summarize the result."
  }
}
```

The local runner stores the job in `job_state.json`. `AutoPullRuntime` can later continue jobs whose mode is `auto_pull`.

## Recipes

Recipes are loaded from `recipes/<recipe-name>/RECIPE.md`. They use the same shared frontmatter/section parser as skills.

Canonical shape:

```markdown
---
name: morning-routine
title: Morning Shift Handover
description: Summarize overnight CSIRT activity.
keywords: morning, handover, night
---

Instructions:
Use local__time, collect the required sources, and write concise operator notes.

Config:
  local_tools:
    - local__time
    - local__run_skill
  mcp_servers:
    - splunk

Workflow:
  type: guided_collection

Initial Prompt:
I need a summary of the last 12 hours.

Response Template:
## {{ recipe_title }}

{{ response }}
```

Supported recipe sections:

- `Instructions:` - prompt guidance injected into the system prompt.
- `Config:` - optional `local_tools` and `mcp_servers` filters for the turn.
- `Workflow:` - preserved as model guidance, not executed deterministically.
- `Initial Prompt:` - loaded into the TUI input when a recipe is selected.
- `Response Template:` - simple `{{ recipe_title }}` and `{{ response }}` replacement.

If a recipe restricts `local_tools`, only those local tools are advertised for the turn.

## MCP

MCP servers are configured in `mcp_servers`. Network permission must be enabled before MCP discovery or tool calls happen. The orchestrator advertises a ranked subset of local and MCP tools to stay under the provider tool limit.

MCP tool names are externalized as `<server>__<tool>`. The original server and tool names are retained internally for execution.

OAuth-capable MCP servers use `auth.type: oauth_public`. Token/client state is stored under `data/oauth/`.

## Interfaces

### TUI

The TUI supports chat plus slash commands for:

- conversation navigation,
- recipe activation,
- MCP filters,
- permissions,
- scratchpad,
- findings,
- local search,
- compaction,
- logging/help/exit.

### Web/API

The web server exposes:

- `GET /healthz`
- `GET/POST /api/conversations`
- `GET/PUT/DELETE /api/conversations/{id}`
- `POST /api/conversations/{id}/messages`
- `POST /api/conversations/{id}/compact`
- `POST/DELETE /api/conversations/{id}/recipe`
- `GET/PUT/DELETE /api/conversations/{id}/mcp-servers`
- `GET /api/conversations/{id}/mcp-statuses`
- `GET /api/conversations/{id}/jobs`
- `GET/PUT /api/conversations/{id}/scratchpad`
- `GET/POST /api/conversations/{id}/findings`
- `PUT/DELETE /api/findings/{finding_id}`
- `GET /api/search`
- `GET /api/recipes`
- `GET/DELETE /api/jobs/{job_id}`
- MCP OAuth helper routes under `/api/mcp/oauth-servers`

`/api/search` searches conversation messages, scratchpads, findings, and investigation memory.

## Current Non-Goals

The Rust client does not currently implement:

- deterministic iterative recipe execution,
- scheduled recipes/prompts,
- outbound delivery wrappers,
- command-backed skill adapters beyond `local__exec_cli`,
- full compatibility with the older Python `bidule2` internals.

Those concepts may be added later, but current recipes and skills should target the Rust-native `local__run_skill` and local-tool contracts.
