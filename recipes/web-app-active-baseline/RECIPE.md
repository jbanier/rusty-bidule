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

Initial Prompt:
Build a bounded active baseline for the authorized web application targets.

Response Template:
## {{ recipe_title }}

{{ response }}

