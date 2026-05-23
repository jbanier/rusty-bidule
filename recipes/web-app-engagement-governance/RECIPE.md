---
name: web-app-engagement-governance
title: Web App Engagement Governance
description: Maintain normalized engagement state, WSTG/API coverage, skipped checks, unresolved approvals, and validation-ready findings.
keywords: web, engagement, coverage, governance, validation
safety_profile: passive
requires_active_authorization: false
requires_oob_authorization: false
requires_destructive_authorization: false
methodology:
  - OWASP WSTG
  - OWASP API Security Top 10 2023
---

Instructions:
Use this recipe to organize an authorized web assessment before or between active testing phases. Preserve scope gaps and skipped checks. Do not infer coverage from silence.

Use:
- `web-engagement-state` for normalized scope, endpoint inventory, rate limits, and unresolved approvals,
- `web-coverage-status` for WSTG/API coverage and gaps,
- `web-finding-validator` for validation status when findings are ready for review.

Config:
  local_tools:
    - local__activate_skill
    - local__run_skill
    - local__get_investigation_memory
    - local__update_investigation_memory
  max_agent_iterations: 7
  continuation_increment: 4

Workflow:
  type: supervised_steps
  steps:
    - name: Normalize engagement state
      prompt: |
        Read investigation memory and operator-provided scope notes. Activate and run web-engagement-state to normalize target scope, endpoint inventory, rate limits, skipped checks, and unresolved approvals. Store useful investigation_memory_patch output.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Summarize coverage
      prompt: |
        Activate and run web-coverage-status with available coverage, findings, and skipped-check notes. Summarize tested categories, gaps, skipped checks, and approvals needed without claiming untested areas are clean.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
    - name: Validate ready findings
      prompt: |
        If candidate findings have enough evidence, activate and run web-finding-validator for each ready finding. Keep scanner or payload output as lead status unless every validation gate passes.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory

Initial Prompt:
Normalize web assessment engagement state, coverage, skipped checks, and validation status.

Response Template:
## {{ recipe_title }}

{{ response }}

