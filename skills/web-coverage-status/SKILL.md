---
name: web-coverage-status
description: Summarizes web assessment coverage against OWASP WSTG and OWASP API Security Top 10, including tested, skipped, and unresolved checks.
metadata:
  keywords: web, coverage, wstg, api top 10, gaps
---

# Web Coverage Status

Use this skill to turn endpoint, finding, and skipped-check notes into a coverage dashboard. Coverage output is evidence-planning support, not a claim that untested areas are secure.

Tools:
  - name: Build Coverage Status
    slug: build-coverage-status
    description: Normalize coverage entries, findings, skipped checks, and methodology gaps into JSON.
    script: scripts/coverage_status.py
    safety_profile: passive
    methodology:
      - OWASP WSTG
      - OWASP API Security Top 10 2023

