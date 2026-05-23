---
name: web-scanner-result-normalizer
description: Imports ZAP baseline and Nuclei JSON/JSONL scanner output as deduplicated scoped leads that require manual validation before reporting.
metadata:
  keywords: web, scanner, zap, nuclei, normalizer, leads
---

# Web Scanner Result Normalizer

Use this skill after authorized scanner runs. Scanner findings are normalized as leads only. Do not present them as confirmed until `web-finding-validator` validates evidence and impact.

Tools:
  - name: Normalize Scanner Results
    slug: normalize-scanner-results
    description: Parse ZAP baseline or Nuclei JSON/JSONL results, dedupe scoped findings, and map them to methodology categories.
    script: scripts/scanner_result_normalizer.py
    filesystem: read_only
    safety_profile: passive
    methodology:
      - OWASP WSTG
      - OWASP API Security Top 10 2023

