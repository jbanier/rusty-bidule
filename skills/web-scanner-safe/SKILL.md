---
name: web-scanner-safe
description: Builds scoped safe scanner plans for nuclei and OWASP ZAP baseline scans with explicit template/rate limits and destructive checks disabled by default.
metadata:
  keywords: web, scanner, nuclei, zap, safe, baseline
---

# Web Scanner Safe

Use this skill for scanner planning and bounded safe scans. The default output is a command plan; do not run scanner commands unless the user has authorized active testing and the template set is non-destructive.

Constraints:

- Exclude brute force, DoS, destructive, intrusive, and exploit-heavy templates by default.
- Respect engagement rate limits and blackout windows.
- Treat scanner output as leads requiring manual confirmation.

Tools:
  - name: Safe Scanner Plan
    slug: scanner-safe
    description: Validate scope and generate safe nuclei/ZAP baseline command plans with conservative exclusions.
    script: scripts/scanner_safe.py
    network: true

