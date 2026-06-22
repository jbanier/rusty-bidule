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
  max_agent_iterations: 7
  continuation_increment: 4

Workflow:
  type: supervised_steps
  steps:
    - name: Load validated scope
      prompt: |
        Read investigation memory and confirm validated passive or low-impact scope. If scope is missing or active authorization is unclear, summarize the gap and direct the operator to web-app-scope-intake.
      local_tools:
        - local__get_investigation_memory
    - name: HTTP baseline
      prompt: |
        Activate and run web-http-baseline for scoped targets where authorization is clear. Summarize only header, cookie, redirect, CORS, cache, TLS, and exposure observations with evidence references. Keep output under 60 lines; do not paste raw JSON, headers, command output, or logs.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
    - name: Recon tool inventory
      prompt: |
        Activate and run web-discovery-recon for scoped discovery planning, but summarize only target scope, tool availability, and evidence references. Update investigation memory with durable target and tool-inventory facts. Keep output under 60 lines; do not paste raw JSON, full command lists, command output, or logs.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Bounded recon command plan
      prompt: |
        Activate and run web-discovery-recon as needed to produce a bounded recon command plan for validated scope. Use local CLI only for explicitly allowed low-impact commands. Summarize command phases, safety constraints, and gaps; do not expand every command argument unless it is the next action. Keep output under 60 lines and update investigation memory.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__exec_cli
        - local__get_investigation_memory
        - local__update_investigation_memory

Initial Prompt:
Run passive and low-impact reconnaissance for the scoped web application targets.

Response Template:
## {{ recipe_title }}

{{ response }}
