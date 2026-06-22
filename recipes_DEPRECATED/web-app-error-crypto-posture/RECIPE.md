---
name: web-app-error-crypto-posture
title: Web App Error and Crypto Posture
description: Review error handling and cryptographic posture from scoped HTTP evidence, supplied headers, TLS observations, CSP origins, and related host candidates.
keywords: web, error-handling, crypto, tls, hsts, csp
safety_profile: passive
requires_active_authorization: false
methodology:
  - OWASP WSTG-ERRH
  - OWASP WSTG-CRYP
  - OWASP WSTG-CONF
---

Instructions:
Use this recipe to combine HTTP baseline evidence, error disclosure review, and passive TLS/header posture. Related hosts from certificate transparency or CSP are candidates only; confirm scope before any follow-up command is run.

Config:
  local_tools:
    - local__activate_skill
    - local__run_skill
    - local__read_file
    - local__get_investigation_memory
    - local__update_investigation_memory
  max_agent_iterations: 7
  continuation_increment: 4

Workflow:
  type: supervised_steps
  steps:
    - name: Confirm scope and evidence
      prompt: |
        Read scope, authorization state, target URLs, allowed hosts, existing HTTP baseline output, TLS observations, headers, route inventory, and error evidence from memory or files. Do not fetch new network evidence unless active authorization is explicit.
      local_tools:
        - local__read_file
        - local__get_investigation_memory
    - name: HTTP baseline context
      prompt: |
        Activate web-http-baseline if target URLs and authorization allow low-impact fetches, otherwise use supplied headers and prior evidence only. Capture header, cookie, redirect, CORS, cache, and body fingerprint observations as context.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Error handling review
      prompt: |
        Activate and run web-error-handling-review with supplied responses, observations, route inventory, and parameterized URLs. Report verbose errors, stack traces, debug leaks, framework disclosures, and safe validation priorities without exploit payloads.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Crypto posture review
      prompt: |
        Activate and run web-crypto-posture with headers, TLS/certificate observations, CSP origins, and related host candidates. Keep confirmed in-scope targets separate from CT/CSP-derived candidates and list TLS follow-up commands as plans only.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory

Initial Prompt:
Review scoped web error handling and cryptographic posture from supplied evidence.

Response Template:
## {{ recipe_title }}

{{ response }}

