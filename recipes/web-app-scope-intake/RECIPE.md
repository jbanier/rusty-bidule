---
name: web-app-scope-intake
title: Web App Scope Intake
description: Capture authorization, targets, allowed hosts, credentials, constraints, exclusions, and reporting needs for a web application posture assessment.
keywords: web, pentest, scope, authorization, intake
---

Instructions:
Use this recipe before any active web assessment work.

Collect or confirm:
- target URLs and allowed hosts,
- authorization for active testing,
- explicitly excluded tests,
- credentials and roles available for testing,
- rate limits and blackout windows,
- OOB callback authorization,
- destructive/high-impact testing authorization,
- reporting format and finding severity expectations.

Run `web-scope-guard` with `tool_slug="validate-scope"` once enough scope details are available. Store the returned `investigation_memory_patch` with `local__update_investigation_memory`.

Do not run scanners, crawlers, fuzzers, or payload probes from this recipe.

Config:
  local_tools:
    - local__activate_skill
    - local__run_skill
    - local__update_investigation_memory
    - local__get_investigation_memory

Initial Prompt:
Set up an authorized web application posture assessment. I will provide target URLs, allowed hosts, credentials or roles, rate limits, exclusions, and reporting requirements.

Response Template:
## {{ recipe_title }}

{{ response }}

