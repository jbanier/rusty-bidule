---
name: web-evidence-report
description: Converts web assessment observations and findings JSON into a concise posture report with evidence references, severity rationale, remediation, and retest checklist.
metadata:
  keywords: web, report, findings, evidence, remediation, retest
---

# Web Evidence Report

Use this skill at the end of a web assessment to normalize observations into report-ready findings. Cite stored tool artifacts and distinguish confirmed findings from leads.

Tools:
  - name: Build Web Report
    slug: build-web-report
    description: Render a markdown posture report from findings JSON, scope summary, evidence notes, and retest requirements.
    script: scripts/evidence_report.py
    filesystem: read_write

