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

Initial Prompt:
Assess business logic and race-condition posture for the scoped web application workflows.

Response Template:
## {{ recipe_title }}

{{ response }}
