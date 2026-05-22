---
name: web-app-business-logic-race
title: Web App Business Logic And Race Review
description: Assess workflow abuse, state manipulation, replay, idempotency, race conditions, and domain-specific abuse cases.
keywords: web, business-logic, race, workflow, replay
---

Instructions:
Confirm active authorization, rate limits, test accounts, and business workflow boundaries. Avoid high-rate race testing unless explicitly authorized.

Document expected state transitions before testing. Record observations as workflow, role, request, expected result, observed result, and impact.

Use `web-input-probe` for planning and `web-access-control-matrix` where object or role access is involved.

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
    - name: Map business workflows
      prompt: |
        Read scope, accounts, rate limits, and business workflow notes from memory. Map expected state transitions, roles, requests, expected results, observed results, and impact hypotheses. Ask only for missing workflow context if testing cannot proceed.
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Abuse and race planning
      prompt: |
        Activate and run web-input-probe for business-logic, replay, idempotency, state manipulation, and race-condition planning. Avoid high-rate race testing unless explicit authorization is present.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__exec_cli
        - local__get_investigation_memory
    - name: Role and impact summary
      prompt: |
        Use web-access-control-matrix if object or role access is involved. Summarize confirmed workflow issues, race-test limits, abuse cases to validate, and unresolved business assumptions. Update investigation memory.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__exec_cli
        - local__get_investigation_memory
        - local__update_investigation_memory

Initial Prompt:
Assess business logic and race-condition posture for the scoped web application workflows.

Response Template:
## {{ recipe_title }}

{{ response }}
