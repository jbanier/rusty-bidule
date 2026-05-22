---
name: web-app-active-baseline
title: Web App Active Baseline
description: Build a bounded active baseline with crawl inventory, endpoint map, parameter discovery, WAF observations, TLS checks, and safe scanner planning.
keywords: web, active, crawl, baseline, scanner
---

Instructions:
Confirm validated scope and active testing authorization before running any active tool. Respect rate limits and blackout windows.

Use:
- `web-crawler-inventory` for bounded same-scope crawling,
- `web-discovery-recon` for tool availability and command planning,
- `web-scanner-safe` for conservative nuclei/ZAP baseline plans.

Do not run destructive templates, brute force, denial-of-service checks, or WAF evasion. Scanner findings are leads until manually confirmed.

Config:
  local_tools:
    - local__time
    - local__activate_skill
    - local__run_skill
    - local__get_investigation_memory
    - local__update_investigation_memory
    - local__write_file
    - local__exec_cli
    - local__get_job
    - local__list_jobs
  max_agent_iterations: 8
  continuation_increment: 5

Workflow:
  type: supervised_steps
  steps:
    - name: Confirm active authorization
      prompt: |
        Read investigation memory and confirm active testing authorization, allowed hosts, rate limits, blackout windows, and exclusions. If active authorization is missing, stop with the exact missing approvals.
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Crawl inventory
      prompt: |
        Activate and run web-crawler-inventory for bounded same-scope crawling. Persist any useful endpoint or parameter inventory as evidence or memory, and summarize only discovered surfaces and crawl gaps. Keep output under 60 lines; do not paste raw JSON, headers, command output, or logs.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory
        - local__write_file
    - name: Safe scanner plan
      prompt: |
        Activate web-discovery-recon and web-scanner-safe as needed to produce conservative scanner and command plans. Do not run destructive templates or evasion. If active scanning is explicitly authorized and the operator asks to run a safe scanner command, use local__exec_cli with execution_mode managed_job, wait_for_result true, and the scoped rate limits instead of raising the foreground timeout. Summarize leads, unsafe checks excluded, and next validation work. Keep output under 60 lines; do not paste raw JSON, full command lists, command output, or logs.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__exec_cli
        - local__get_job
        - local__list_jobs
        - local__get_investigation_memory
        - local__update_investigation_memory

Initial Prompt:
Build a bounded active baseline for the authorized web application targets.

Response Template:
## {{ recipe_title }}

{{ response }}
