---
name: web-http-baseline
description: Collects scoped HTTP baseline evidence for a target URL, including headers, cookies, redirects, methods, CORS/cache hints, and common security header posture.
metadata:
  keywords: web, http, headers, cookies, cors, cache, baseline
---

# Web HTTP Baseline

Use this skill for authorized low-impact HTTP evidence collection. It validates the target against the supplied scope before any request.

Constraints:

- Set `fetch=true` only when active network testing is authorized.
- Do not use this skill for fuzzing, brute force, WAF evasion, or high-rate requests.
- Summarize missing security headers, cookie attributes, redirects, caching, and CORS posture as observations, not confirmed vulnerabilities unless impact is demonstrated.

Tools:
  - name: HTTP Baseline
    slug: http-baseline
    description: Validate scope and optionally fetch a URL to inspect headers, cookies, body fingerprint, and security header posture.
    script: scripts/http_baseline.py
    network: true

