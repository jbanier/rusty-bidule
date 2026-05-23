---
name: web-app-browser-evidence
title: Web App Browser Evidence Review
description: Organize browser-assisted observations for JavaScript-heavy or authenticated flows, including screenshots, DOM routes, forms, and client-side security checks.
keywords: web, browser, javascript, dom, screenshots
safety_profile: passive
requires_active_authorization: false
methodology:
  - OWASP WSTG-CLNT
  - OWASP WSTG-SESS
---

Instructions:
Use this recipe after browser inspection or browser automation. Screenshots and DOM observations support evidence but do not replace HTTP request/response proof for validated findings.

Config:
  local_tools:
    - local__activate_skill
    - local__run_skill
    - local__get_investigation_memory
    - local__update_investigation_memory
  max_agent_iterations: 6
  continuation_increment: 4

Workflow:
  type: supervised_steps
  steps:
    - name: Normalize browser observations
      prompt: |
        Activate and run web-browser-evidence with available page URLs, routes, screenshots, forms, storage keys, and observations. Separate authenticated and anonymous observations when possible.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
    - name: Extract client routes
      prompt: |
        If HTML or JavaScript files/text are available, activate and run web-js-route-extractor. Fetch URLs only when active authorization is clear. Summarize routes, API paths, WebSocket URLs, source maps, and parameters as inventory leads.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Evidence gaps
      prompt: |
        Summarize client-side findings, required HTTP evidence, missing screenshots or flows, and validation next steps. Update memory with durable route inventory and gaps.
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory

Initial Prompt:
Organize browser-assisted evidence for scoped web application flows.

Response Template:
## {{ recipe_title }}

{{ response }}

