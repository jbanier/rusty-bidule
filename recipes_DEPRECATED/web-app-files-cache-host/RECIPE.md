---
name: web-app-files-cache-host
title: Web App Files, Cache, And Host Header Review
description: Assess upload/download handling, MIME/extension validation, path traversal, cache poisoning/deception, host-header attacks, CORS, and clickjacking posture.
keywords: web, upload, cache, host-header, cors, clickjacking
---

Instructions:
Confirm scope and feature boundaries. Use only benign test files and authorized test accounts.

Use:
- `web-upload-content` for upload/download checklist planning,
- `web-http-baseline` for headers, CORS, frame controls, and cache hints,
- `web-input-probe` for path traversal and host/cache probe planning.

Config:
  local_tools:
    - local__activate_skill
    - local__run_skill
    - local__exec_cli
    - local__get_investigation_memory
    - local__update_investigation_memory
  max_agent_iterations: 8
  continuation_increment: 5

Workflow:
  type: supervised_steps
  steps:
    - name: Upload and download handling
      prompt: |
        Read scope, credentials, and file-feature boundaries from memory. Activate and run web-upload-content for upload/download checklist planning using only benign test files and authorized accounts. Summarize validation, storage, retrieval, and gap observations. Keep output under 60 lines; do not paste raw JSON, file contents, command output, or logs.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Headers, CORS, cache, and framing
      prompt: |
        Activate and run web-http-baseline for scoped headers, CORS, frame controls, and cache hints. Summarize posture observations and evidence references without pasting full raw responses.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__exec_cli
        - local__get_investigation_memory
    - name: Host, cache, and path probe plan
      prompt: |
        Activate and run web-input-probe for path traversal, host-header, cache poisoning, and cache deception planning within authorized boundaries. Summarize confirmed vs planned checks and update investigation memory.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__exec_cli
        - local__get_investigation_memory
        - local__update_investigation_memory

Initial Prompt:
Assess file handling, cache behavior, CORS, clickjacking, and host-header posture for the scoped web app.

Response Template:
## {{ recipe_title }}

{{ response }}
