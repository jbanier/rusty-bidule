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

Initial Prompt:
Assess authentication and session-management posture for the scoped web app.

Response Template:
## {{ recipe_title }}

{{ response }}
