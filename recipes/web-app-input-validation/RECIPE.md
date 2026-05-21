---
name: web-app-input-validation
title: Web App Input Validation Review
description: Plan and track scoped checks for SQLi, NoSQLi, XSS, DOM XSS, command injection, path traversal, SSTI, XXE, SSRF, prototype pollution, and deserialization.
keywords: web, input-validation, sqli, xss, ssrf, xxe
---

Instructions:
Confirm scope and active testing authorization. Do not use destructive payloads, data extraction, denial-of-service, or OOB callbacks unless explicitly authorized.

Use `web-input-probe` to generate a parameter/context checklist. Use `web-scope-guard` if OOB or destructive authorization is unclear. Keep proof steps minimal and evidence-focused.

Config:
  local_tools:
    - local__activate_skill
    - local__run_skill
    - local__exec_cli
    - local__get_investigation_memory
    - local__update_investigation_memory

Initial Prompt:
Plan and execute a scoped input-validation review for the authorized web app.

Response Template:
## {{ recipe_title }}

{{ response }}
