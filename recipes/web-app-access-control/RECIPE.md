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

Initial Prompt:
Assess access-control posture across the available roles and object references.

Response Template:
## {{ recipe_title }}

{{ response }}
