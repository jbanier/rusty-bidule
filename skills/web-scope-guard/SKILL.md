---
name: web-scope-guard
description: Validates authorized web assessment scope, target URLs, allowed hosts, active-test flags, rate limits, exclusions, and reporting constraints before any web posture testing.
metadata:
  keywords: web, pentest, scope, authorization, allowlist, guardrails
---

# Web Scope Guard

Use this skill before any active web assessment recipe or skill. It turns the user's authorization details into a normalized scope object and fails closed when targets do not match allowed hosts.

Constraints:

- Do not perform active testing until the user has provided authorization, target URLs, allowed hosts or target hosts, and any excluded test classes.
- Keep destructive testing, brute force, denial-of-service, WAF evasion, and OOB callbacks disabled unless the user explicitly authorizes them in writing.
- Store the returned `investigation_memory_patch` with `local__update_investigation_memory` so later recipes can reuse the same scope.

Tools:
  - name: Validate Scope
    slug: validate-scope
    description: Normalize target URLs, allowed hosts, active/destructive/OOB flags, rate limits, excluded tests, and reporting requirements.
    script: scripts/scope_guard.py

