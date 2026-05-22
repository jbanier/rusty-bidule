---
name: web-app-api-graphql-websocket
title: Web App API, GraphQL, And WebSocket Review
description: Assess API and realtime surfaces including OpenAPI, GraphQL, WebSocket auth, object authorization, batching, replay, and tampering.
keywords: web, api, graphql, websocket
---

Instructions:
Confirm API/WebSocket endpoints are in validated scope. Use credentials only within their authorized roles.

Use:
- `web-api-graphql` for OpenAPI/GraphQL review,
- `web-websocket` for WebSocket command planning and checklist,
- `web-access-control-matrix` when comparing API object access across roles.

Config:
  local_tools:
    - local__activate_skill
    - local__run_skill
    - local__exec_cli
    - local__read_file
    - local__get_investigation_memory
    - local__update_investigation_memory
  max_agent_iterations: 8
  continuation_increment: 5

Workflow:
  type: supervised_steps
  steps:
    - name: API and GraphQL review
      prompt: |
        Read scope and endpoint notes from memory. Activate and run web-api-graphql for scoped OpenAPI, REST, and GraphQL review, using local__read_file for provided specs only. Summarize exposed surfaces, auth assumptions, and evidence references.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__read_file
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: WebSocket review
      prompt: |
        Activate and run web-websocket for scoped WebSocket endpoints. Summarize connection requirements, message types, auth/replay/tamper checks, and gaps without dumping raw frames.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__exec_cli
        - local__get_investigation_memory
    - name: Role and object comparison
      prompt: |
        If role or object authorization observations exist, activate and run web-access-control-matrix for API/GraphQL/WebSocket comparisons. Otherwise, state the missing role/object evidence.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__exec_cli
        - local__get_investigation_memory
    - name: Surface summary
      prompt: |
        Summarize confirmed API, GraphQL, and WebSocket findings, unresolved authorization comparisons, and next focused checks. Update investigation memory with durable endpoint inventory and gaps.
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory

Initial Prompt:
Assess API, GraphQL, and WebSocket posture for the scoped web application.

Response Template:
## {{ recipe_title }}

{{ response }}
