---
name: web-app-passive-recon
title: Web App Passive Recon
description: Collect low-impact web posture evidence for scoped targets, including headers, cookies, TLS, DNS, technologies, exposed files, and public attack surface.
keywords: web, recon, passive, headers, tls, cookies
---

Instructions:
Read investigation memory first and confirm validated scope. If scope is missing, switch to `web-app-scope-intake`.

Use:
- `web-http-baseline` for security headers, cookies, redirects, CORS, and cache hints,
- `web-discovery-recon` for a scoped recon command plan and installed tool inventory,
- existing `nmap` only when active testing is authorized and the target host is in scope.

Treat missing headers, permissive CORS, weak cookies, outdated TLS, and exposed metadata as posture observations unless impact is confirmed.

Config:
  local_tools:
    - local__time
    - local__activate_skill
    - local__run_skill
    - local__get_investigation_memory
    - local__update_investigation_memory
    - local__exec_cli

Initial Prompt:
Run passive and low-impact reconnaissance for the scoped web application targets.

Response Template:
## {{ recipe_title }}

{{ response }}

