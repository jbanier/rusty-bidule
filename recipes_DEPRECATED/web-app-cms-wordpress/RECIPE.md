---
name: web-app-cms-wordpress
title: Web App CMS And WordPress Review
description: Assess WordPress/CMS posture including version exposure, plugin/theme risk, user enumeration, XML-RPC, config leakage, and hardening.
keywords: web, cms, wordpress, wpscan
---

Instructions:
Confirm the target is in scope and active testing is authorized before CMS probing.

Use `web-http-baseline` for exposed files and headers, `web-discovery-recon` for command planning, and `wpscan` only when explicitly allowed by scope and rate limits. Do not brute force credentials.

Config:
  local_tools:
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
    - name: Baseline and exposed files
      prompt: |
        Read validated CMS/WordPress scope from memory. Activate and run web-http-baseline for headers, exposed files, redirects, cookies, and metadata on scoped targets. Summarize exposed-version and hardening observations only.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: CMS tooling plan
      prompt: |
        Activate and run web-discovery-recon for CMS and WordPress command planning and installed tool inventory. Use wpscan only when scope and rate limits explicitly allow it; do not brute force credentials. Keep output under 60 lines; do not paste raw JSON, full command lists, command output, or logs.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__exec_cli
        - local__get_investigation_memory
    - name: Hardening summary
      prompt: |
        Summarize confirmed CMS/WordPress findings, tooling constraints, exposed files, user-enumeration/XML-RPC/config-leakage gaps, and hardening recommendations. Update investigation memory with durable findings and gaps.
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory

Initial Prompt:
Assess CMS or WordPress posture for the scoped web application.

Response Template:
## {{ recipe_title }}

{{ response }}
