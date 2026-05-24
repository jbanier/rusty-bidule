---
name: web-app-dependency-integrity
title: Web App Dependency Integrity Review
description: Inventory server and browser dependencies, third-party assets, SRI/pinning posture, and supplied SCA scanner output as scoped supply-chain leads.
keywords: web, dependencies, sca, supply-chain, sri, integrity
safety_profile: passive
requires_active_authorization: false
methodology:
  - OWASP Top 10 A06
  - OWASP Top 10 A08
  - OWASP SCVS
---

Instructions:
Use this recipe with local manifests, lockfiles, HTML/browser asset evidence, and supplied SCA output. It produces inventories and command plans only; do not run network SCA tools unless the operator explicitly authorizes them.

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
    - name: Inventory dependency evidence
      prompt: |
        Read available manifest paths, lockfiles, package manager files, supplied scanner artifacts, browser evidence, HTML assets, and client-side audit output. Keep file paths inside the workspace.
      local_tools:
        - local__read_file
        - local__get_investigation_memory
    - name: Run dependency SCA inventory
      prompt: |
        Activate and run web-dependency-sca with manifest paths, supplied SCA JSON, HTML/browser assets, and client-side audit output. Summarize package inventory, third-party assets, SRI coverage, pinning observations, scanner leads, and safe follow-up commands.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Normalize supplied scanner leads
      prompt: |
        If ZAP, Nuclei, OSV, npm audit, pip-audit, or Dependency-Check output is available, activate web-scanner-result-normalizer where applicable and keep all scanner-derived issues as leads until manually validated.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Report integrity gaps
      prompt: |
        Update memory with durable dependency and third-party asset inventories. Separate confirmed vulnerable components from missing-evidence gaps and proposed scanner commands.
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory

Initial Prompt:
Review dependency and browser supply-chain integrity evidence for the scoped web application.

Response Template:
## {{ recipe_title }}

{{ response }}

