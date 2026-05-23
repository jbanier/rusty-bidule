---
name: web-finding-validator
description: Applies strict validation gates to web assessment findings before they can be reported as confirmed vulnerabilities.
metadata:
  keywords: web, validation, findings, evidence, false positives
---

# Web Finding Validator

Use this skill before final reporting or when scanner/tool output looks interesting. The validator is intentionally skeptical: findings stay as `lead`, `needs-work`, or `rejected` until evidence satisfies every gate.

Validation gates:

- Reproducible request or steps.
- HTTP request and response evidence.
- Demonstrated impact.
- In-scope affected endpoint.
- Real vulnerability, not only an informational observation.
- Client reproducibility.
- Credential and secret redaction.

Tools:
  - name: Validate Finding
    slug: validate-finding
    description: Validate a finding JSON object against scope and evidence gates, returning a recommended status and gate results.
    script: scripts/finding_validator.py
    safety_profile: passive
    methodology:
      - OWASP WSTG
      - OWASP API Security Top 10 2023

