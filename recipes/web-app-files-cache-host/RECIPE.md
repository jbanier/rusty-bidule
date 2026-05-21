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

Initial Prompt:
Assess file handling, cache behavior, CORS, clickjacking, and host-header posture for the scoped web app.

Response Template:
## {{ recipe_title }}

{{ response }}
