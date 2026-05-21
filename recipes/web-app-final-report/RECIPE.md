---
name: web-app-final-report
title: Web App Final Report
description: Normalize web assessment findings into a concise report with evidence references, severity rationale, remediation, and retest checklist.
keywords: web, report, findings, remediation, retest
---

Instructions:
Use investigation memory, findings, and tool evidence to separate confirmed vulnerabilities from leads and gaps.

Use `web-evidence-report` to generate report-ready markdown. Every confirmed finding must include evidence, scope, reproduction boundaries, impact, remediation, and retest steps.

Do not overstate scanner leads as confirmed findings.

Config:
  local_tools:
    - local__activate_skill
    - local__run_skill
    - local__read_file
    - local__write_file
    - local__get_investigation_memory
    - local__search_conversation_memories

Initial Prompt:
Prepare the final web application posture assessment report from the current evidence and findings.

Response Template:
## {{ recipe_title }}

{{ response }}

