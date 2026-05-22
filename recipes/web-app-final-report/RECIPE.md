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
  max_agent_iterations: 8
  continuation_increment: 5

Workflow:
  type: supervised_steps
  steps:
    - name: Gather findings and evidence
      prompt: |
        Read investigation memory, relevant conversation memories, and provided evidence files. Collect confirmed findings, leads, gaps, evidence ids, scope boundaries, and reporting requirements without expanding raw tool output.
      local_tools:
        - local__read_file
        - local__get_investigation_memory
        - local__search_conversation_memories
    - name: Generate report draft
      prompt: |
        Activate and run web-evidence-report with the confirmed findings and available evidence. Write a report artifact only if the operator requested a file path or durable report file.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__read_file
        - local__write_file
        - local__get_investigation_memory
    - name: Review confirmed findings and gaps
      prompt: |
        Review the draft for unsupported claims, scanner leads presented as confirmed findings, missing evidence, missing retest steps, and scope/reporting gaps. Return the concise final report summary and gap list.
      local_tools:
        - local__read_file
        - local__get_investigation_memory
        - local__search_conversation_memories

Initial Prompt:
Prepare the final web application posture assessment report from the current evidence and findings.

Response Template:
## {{ recipe_title }}

{{ response }}
