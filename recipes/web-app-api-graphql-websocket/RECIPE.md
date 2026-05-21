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

Initial Prompt:
Assess API, GraphQL, and WebSocket posture for the scoped web application.

Response Template:
## {{ recipe_title }}

{{ response }}
