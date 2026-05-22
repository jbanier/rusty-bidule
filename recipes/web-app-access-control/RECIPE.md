---
name: web-app-access-control
title: Web App Access Control Review
description: Assess IDOR/BOLA, vertical and horizontal authorization, forced browsing, method confusion, and object reference controls.
keywords: web, access-control, idor, bola, authorization
---

Instructions:
Use validated scope and only authorized test accounts. Compare equivalent requests across anonymous, user, and privileged roles.

Use `web-access-control-matrix` after collecting observations with role, method, path, object ID, expected access, and observed status. Confirm object ownership and intended authorization before calling an issue a finding.

Config:
  local_tools:
    - local__activate_skill
    - local__run_skill
    - local__exec_cli
    - local__get_investigation_memory
    - local__update_investigation_memory
  max_agent_iterations: 7
  continuation_increment: 4

Workflow:
  type: supervised_steps
  steps:
    - name: Collect role and object observations
      prompt: |
        Read scope and role context from memory. Collect available observations by role, method, path, object id, expected access, observed status, and evidence source. If accounts or object ownership are missing, summarize the missing comparisons.
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory
    - name: Access-control matrix
      prompt: |
        Activate and run web-access-control-matrix with the collected role/object observations. Use local CLI only for explicitly authorized equivalent request comparisons. Do not infer findings without expected access and observed access evidence.
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__exec_cli
        - local__get_investigation_memory
    - name: Confirmed and inconclusive cases
      prompt: |
        Summarize confirmed access-control findings separately from inconclusive cases, role gaps, and follow-up checks. Update investigation memory with durable confirmed findings and unresolved comparisons.
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory

Initial Prompt:
Assess access-control posture across the available roles and object references.

Response Template:
## {{ recipe_title }}

{{ response }}
