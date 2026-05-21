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

Initial Prompt:
Assess CMS or WordPress posture for the scoped web application.

Response Template:
## {{ recipe_title }}

{{ response }}

