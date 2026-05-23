---
name: web-engagement-state
description: Maintains normalized authorized web assessment engagement state, including scope, endpoints, rate limits, skipped checks, and unresolved approvals.
metadata:
  keywords: web, engagement, scope, state, inventory, approvals
---

# Web Engagement State

Use this skill to convert scope and assessment notes into structured engagement state that can be stored in investigation memory or used by later recipes.

Constraints:

- Do not perform network requests.
- Preserve unresolved approvals instead of assuming authorization.
- Keep skipped checks explicit with a reason so final reports do not imply complete coverage.

Tools:
  - name: Build Engagement State
    slug: build-engagement-state
    description: Normalize scope, endpoint inventory, skipped checks, rate limits, and unresolved approvals into JSON.
    script: scripts/engagement_state.py
    safety_profile: passive
    methodology:
      - OWASP WSTG
      - OWASP API Security Top 10 2023

