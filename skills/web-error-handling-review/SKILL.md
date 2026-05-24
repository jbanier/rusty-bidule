---
name: web-error-handling-review
description: Reviews supplied responses, observations, routes, and parameterized URLs for verbose errors, stack traces, debug leaks, framework disclosures, and prioritized safe error-handling validation targets.
metadata:
  keywords: web, error-handling, debug, stack-trace, disclosure, routes
---

# Web Error Handling Review

Use this skill for passive error-handling review from supplied evidence. It classifies verbose errors and debug disclosures, prioritizes routes for safe validation, and describes expected evidence without generating exploit payloads.

Keep validation scoped and low impact. Do not fuzz parameters, force exceptions, or run denial-of-service style checks from this skill.

Tools:
  - name: Error Handling Review
    slug: error-handling-review
    description: Classify verbose error evidence and prioritize safe validation targets from supplied observations and route inventory.
    script: scripts/error_handling_review.py
    network: false
    filesystem: read_only
    safety_profile: passive
    requires_active_authorization: false
    methodology:
      - OWASP WSTG-ERRH
      - OWASP Top 10 A05
      - OWASP ASVS V7

