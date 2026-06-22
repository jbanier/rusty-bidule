---
name: web-app-ai-feature-review
title: Web App AI Feature Review
description: Assess LLM-enabled web features for prompt injection, tool abuse, retrieval exposure, system prompt leakage, and agentic boundary issues.
keywords: web, ai, llm, prompt-injection, agentic
safety_profile: active-safe
requires_active_authorization: true
requires_oob_authorization: false
methodology:
  - OWASP Top 10 for LLM Applications
  - OWASP WSTG-BUSL
---

Instructions:
Confirm the AI-enabled feature, tool boundaries, data sources, active authorization, and any OOB restrictions before testing. Do not attempt destructive tool actions or secret extraction beyond agreed evidence boundaries.

Config:
  local_tools:
    - local__activate_skill
    - local__run_skill
    - local__get_investigation_memory
    - local__update_investigation_memory
  max_agent_iterations: 7
  continuation_increment: 4

Workflow:
  type: supervised_steps
  steps:
    - name: Map AI feature boundaries
      prompt: |
        Read scope and AI feature notes from memory. Identify user-controlled inputs, retrieved content, model-visible tools, data sources, sensitive actions, and OOB restrictions. Stop if active authorization is unclear.
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Build AI feature checklist
      prompt: |
        Activate and run web-ai-feature-review with scoped features, tools, data sources, observations, and OOB authorization state. Summarize checks for prompt injection, indirect prompt injection, tool abuse, retrieval exposure, and system prompt leakage.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
    - name: Evidence and validation
      prompt: |
        Summarize confirmed issues, leads, evidence requirements, missing approvals, and remediation themes. Use web-finding-validator only when a candidate has concrete scoped evidence.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory

Initial Prompt:
Assess scoped AI-enabled web application features.

Response Template:
## {{ recipe_title }}

{{ response }}

