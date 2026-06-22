---
name: web-business-logic-race
description: Assess workflow abuse, state manipulation, replay, idempotency, race conditions, CAPTCHA bypass, sequential validation, payment logic, and domain-specific abuse cases.
metadata:
  keywords: web, business-logic, race, workflow, replay, captcha, payment, sequential
---

# Web App Business Logic And Race Review

Confirm active authorization, rate limits, test accounts, and business workflow boundaries. Avoid high-rate race testing unless explicitly authorized.

Document expected state transitions before testing. Record observations as workflow, role, request, expected result, observed result, and impact.

Use `web-input-probe` for planning and `web-access-control-matrix` where object or role access is involved.

Test CAPTCHA bypass via POST-to-GET conversion, parameter removal, and token reuse. Test sequential validation for password resets, OTP codes, and session tokens. For payment workflows, test negative amounts, currency manipulation, price/quantity tampering.

## Migration Note

This skill was migrated from the `web-app-business-logic-race` recipe.

The original recipe used a declarative workflow. This skill provides guidance for LLM-driven execution.

For detailed methodology, see the original recipe or related skills.

## Related Skills

(Document related skills here during manual review)

---

**Status**: Migrated stub - needs manual review and enhancement
