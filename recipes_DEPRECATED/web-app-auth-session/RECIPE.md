---
name: web-app-auth-session
title: Web App Auth And Session Review
description: Assess authentication, password reset, MFA, cookies, JWT/OAuth, CSRF, account lockout, and session lifecycle posture.
keywords: web, authentication, session, jwt, csrf, oauth
---

Instructions:
Confirm scope, credentials, and role boundaries. Do not brute force or password spray.

Use `web-auth-session-auditor` to analyze collected cookie headers, JWTs, CSRF token names, OAuth notes, and role/session observations. Use `web-http-baseline` for scoped header and cookie collection when authorized.

Report confirmed findings separately from test gaps such as missing credentials or MFA not testable.

Config:
  local_tools:
    - local__activate_skill
    - local__run_skill
    - local__exec_cli
    - local__get_investigation_memory
    - local__update_investigation_memory
  max_agent_iterations: 7
  continuation_increment: 4

Workflow:
  type: supervised_steps
  steps:
    - name: Collect auth and session inputs
      prompt: |
        Read scope and session notes from memory and collect available cookie headers, JWT/OAuth notes, CSRF token names, credentials, roles, and MFA/password-reset constraints. If key inputs are missing, report only the gaps needed for the next step.
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Header and cookie baseline
      prompt: |
        When authorized, activate and run web-http-baseline for scoped header and cookie collection. Summarize session-relevant observations only; avoid pasting full headers unless needed as evidence.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__exec_cli
        - local__get_investigation_memory
    - name: Auth auditor synthesis
      prompt: |
        Activate and run web-auth-session-auditor with the collected cookies, JWTs, CSRF/OAuth notes, and role observations. Update memory with durable confirmed findings and gaps, and separate confirmed issues from untested areas.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory

Initial Prompt:
Assess authentication and session-management posture for the scoped web app.

Response Template:
## {{ recipe_title }}

{{ response }}
