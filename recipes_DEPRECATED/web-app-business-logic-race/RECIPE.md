---
name: web-app-business-logic-race
title: Web App Business Logic And Race Review
description: Assess workflow abuse, state manipulation, replay, idempotency, race conditions, CAPTCHA bypass, sequential validation, payment logic, and domain-specific abuse cases.
keywords: web, business-logic, race, workflow, replay, captcha, payment, sequential
---

Instructions:
Confirm active authorization, rate limits, test accounts, and business workflow boundaries. Avoid high-rate race testing unless explicitly authorized.

Document expected state transitions before testing. Record observations as workflow, role, request, expected result, observed result, and impact.

Use `web-input-probe` for planning and `web-access-control-matrix` where object or role access is involved.

Test CAPTCHA bypass via POST-to-GET conversion, parameter removal, and token reuse. Test sequential validation for password resets, OTP codes, and session tokens. For payment workflows, test negative amounts, currency manipulation, price/quantity tampering.

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
        Read scope, accounts, rate limits, and business workflow notes from memory. Map expected state transitions, roles, requests, expected results, observed results, and impact hypotheses. Identify CAPTCHA-protected endpoints, payment flows, and authentication/reset mechanisms. Ask only for missing workflow context if testing cannot proceed.
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: CAPTCHA bypass testing
      prompt: |
        Test CAPTCHA-protected endpoints (login, registration, password reset, contact forms) for bypass techniques. Test: 1) POST-to-GET method conversion, 2) Parameter removal (removing captcha_response parameter), 3) Token reuse (replaying valid CAPTCHA token), 4) Rate limiting without CAPTCHA validation. Document bypass methods and impacted endpoints.
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Sequential validation testing
      prompt: |
        Test sequential or predictable token generation in: 1) Password reset tokens (test incremental/predictable values), 2) OTP/2FA codes (test sequential codes, timing patterns), 3) Session tokens (test incremental session IDs), 4) Order/transaction IDs (test prediction patterns). Use Burp Sequencer for entropy analysis when available. Document predictable patterns with evidence.
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Payment logic testing
      prompt: |
        Test payment and transaction manipulation: 1) Negative amounts (test -$50 for credit), 2) Currency manipulation (change USD to lower-value currency), 3) Quantity tampering (modify item quantity post-validation), 4) Price parameter manipulation (modify price in checkout), 5) Refund logic abuse (multiple refunds, negative refunds). Document financial impact and affected workflows.
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Abuse and race planning
      prompt: |
        Activate and run web-input-probe for business-logic, replay, idempotency, state manipulation, and race-condition planning. Avoid high-rate race testing unless explicit authorization is present. Consider race conditions in CAPTCHA validation, payment processing, and resource allocation.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__exec_cli
        - local__get_investigation_memory
    - name: Role and impact summary
      prompt: |
        Use web-access-control-matrix if object or role access is involved. Summarize confirmed workflow issues (CAPTCHA bypass, sequential validation flaws, payment manipulation, race conditions), race-test limits, abuse cases validated, and unresolved business assumptions. Update investigation memory with all findings.
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
