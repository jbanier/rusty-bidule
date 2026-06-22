---
name: web-auth-session-testing
description: Assess authentication, password reset, MFA, cookies, JWT/OAuth, CSRF, account lockout, and session lifecycle posture.
metadata:
  keywords: web, authentication, session, jwt, csrf, oauth
---

# Web App Auth And Session Review

Confirm scope, credentials, and role boundaries. Do not brute force or password spray.

Use `web-auth-session-auditor` to analyze collected cookie headers, JWTs, CSRF token names, OAuth notes, and role/session observations. Use `web-http-baseline` for scoped header and cookie collection when authorized.

Report confirmed findings separately from test gaps such as missing credentials or MFA not testable.

## Migration Note

This skill was migrated from the `web-app-auth-session` recipe.

The original recipe used a declarative workflow. This skill provides guidance for LLM-driven execution.

For detailed methodology, see the original recipe or related skills.

## Related Skills

(Document related skills here during manual review)

---

**Status**: Migrated stub - needs manual review and enhancement
