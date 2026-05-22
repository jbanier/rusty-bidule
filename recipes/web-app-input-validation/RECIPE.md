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
  max_agent_iterations: 8
  continuation_increment: 5

Workflow:
  type: supervised_steps
  steps:
    - name: Confirm input-testing authorization
      prompt: |
        Read scope and authorization from memory. Confirm active testing, OOB callback, destructive payload, data extraction, and rate-limit boundaries. If unclear, activate web-scope-guard to validate only the ambiguous boundaries.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Parameter and context probe plan
      prompt: |
        Activate and run web-input-probe to build a parameter/context checklist for SQLi, NoSQLi, XSS, DOM XSS, command injection, path traversal, SSTI, XXE, SSRF, prototype pollution, and deserialization. Keep checks benign and scoped.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__exec_cli
        - local__get_investigation_memory
    - name: Evidence and gaps summary
      prompt: |
        Summarize scoped checks performed or planned, confirmed findings, blocked checks, and required approvals. Update investigation memory with durable parameter inventory, findings, and unresolved gaps.
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory

Initial Prompt:
Plan and execute a scoped input-validation review for the authorized web app.

Response Template:
## {{ recipe_title }}

{{ response }}
