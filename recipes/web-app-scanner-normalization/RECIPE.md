---
name: web-app-scanner-normalization
title: Web App Scanner Result Normalization
description: Normalize authorized ZAP baseline and Nuclei output as scoped leads, dedupe results, map methodology categories, and validate report-ready findings.
keywords: web, scanner, zap, nuclei, validation
safety_profile: passive
requires_active_authorization: false
methodology:
  - OWASP WSTG
  - OWASP API Security Top 10 2023
---

Instructions:
Use this recipe after authorized scanner output is available. Scanner output remains lead status until validated manually with scope, request/response evidence, impact, and reproducibility.

Config:
  local_tools:
    - local__activate_skill
    - local__run_skill
    - local__read_file
    - local__get_investigation_memory
    - local__update_investigation_memory
  max_agent_iterations: 7
  continuation_increment: 4

Workflow:
  type: supervised_steps
  steps:
    - name: Normalize scanner output
      prompt: |
        Read scope and scanner artifact references. Activate and run web-scanner-result-normalizer for ZAP baseline or Nuclei JSON/JSONL output. Summarize deduped scoped leads and out-of-scope exclusions without pasting raw scanner output.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__read_file
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Validate leads
      prompt: |
        For leads with enough evidence, activate and run web-finding-validator. Keep incomplete scanner results as leads or needs-work and document missing proof.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Coverage update
      prompt: |
        Activate and run web-coverage-status to update methodology coverage from normalized leads, validated findings, and skipped checks. Summarize gaps and next validation work.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory

Initial Prompt:
Normalize scanner output and separate leads from validated web findings.

Response Template:
## {{ recipe_title }}

{{ response }}

