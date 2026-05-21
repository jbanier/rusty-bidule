---
name: web-access-control-matrix
description: Compares endpoint/object access observations across anonymous, user, and privileged roles to identify IDOR/BOLA and authorization gaps.
metadata:
  keywords: web, access control, idor, bola, authorization, roles
---

# Web Access Control Matrix

Use this skill after collecting equivalent requests across roles. It analyzes observations; it does not generate unauthorized traffic by itself.

Tools:
  - name: Access Control Matrix
    slug: access-control-matrix
    description: Compare role/object/status observations and flag likely horizontal or vertical authorization inconsistencies.
    script: scripts/access_control_matrix.py

