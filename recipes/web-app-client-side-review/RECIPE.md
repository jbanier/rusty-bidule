---
name: web-app-client-side-review
title: Web App Client-Side Review
description: Review JavaScript-heavy client-side attack surface with passive route discovery, CSP/SRI posture, DOM risk indicators, browser evidence, and shadow API candidates.
keywords: web, javascript, client, csp, sri, dom, shadow-api
safety_profile: passive
requires_active_authorization: false
methodology:
  - OWASP WSTG-CLNT
  - OWASP WSTG-INFO
---

Instructions:
Use this recipe after browser inspection, route extraction, or artifact collection. Keep extracted URLs as leads until validated with scoped HTTP evidence. Do not run one-liners, fuzzing, brute force, or high-concurrency discovery from this recipe.

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
    - name: Gather client artifacts
      prompt: |
        Read investigation memory and available artifact references for browser evidence, HTML, JavaScript, sitemap XML, OpenAPI or Swagger JSON, CSP headers, and route extractor output. Confirm that any active fetches were already authorized before using them as evidence.
      local_tools:
        - local__read_file
        - local__get_investigation_memory
    - name: Normalize browser and route evidence
      prompt: |
        Activate web-browser-evidence and web-js-route-extractor when their inputs are available. Treat routes, parameters, source maps, forms, and browser requests as inventory leads, not confirmed vulnerabilities.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Run client-side audit
      prompt: |
        Activate and run web-client-side-audit with supplied HTML, JavaScript, browser evidence, route inventory, sitemap/OpenAPI artifacts, and headers. Summarize client_routes, api_candidates, external_origins, csp_origins, dom_risk_indicators, SRI posture, and shadow_api_candidates. Keep confirmed issues separate from passive discovery candidates.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Evidence and validation gaps
      prompt: |
        Update memory with durable route inventory, third-party origins, and validation gaps. Recommend manual validation only for scoped targets and do not provide exploit payloads.
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory

Initial Prompt:
Review client-side web application artifacts for passive route discovery, browser supply-chain posture, and safe validation gaps.

Response Template:
## {{ recipe_title }}

{{ response }}

