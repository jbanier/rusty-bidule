---
name: web-auth-session-auditor
description: Reviews authentication and session evidence, including cookie attributes, JWT claims, CSRF token presence, login/logout lifecycle, and role notes.
metadata:
  keywords: web, authentication, session, cookies, jwt, csrf, oauth
---

# Web Auth Session Auditor

Use this skill to analyze collected authentication/session evidence. It does not brute force, spray passwords, or attempt account takeover.

Tools:
  - name: Audit Session Evidence
    slug: auth-session-audit
    description: Analyze cookie headers, JWT structure, CSRF token names, OAuth notes, and role/session observations.
    script: scripts/auth_session_auditor.py

