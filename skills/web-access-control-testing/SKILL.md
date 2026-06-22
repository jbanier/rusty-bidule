---
name: web-access-control-testing
description: Assess IDOR/BOLA, vertical and horizontal authorization, forced browsing, method confusion, and object reference controls.
metadata:
  keywords: web, access-control, idor, bola, authorization
---

# Web App Access Control Review

Use validated scope and only authorized test accounts. Compare equivalent requests across anonymous, user, and privileged roles.

Use `web-access-control-matrix` after collecting observations with role, method, path, object ID, expected access, and observed status. Confirm object ownership and intended authorization before calling an issue a finding.

## Migration Note

This skill was migrated from the `web-app-access-control` recipe.

The original recipe used a declarative workflow. This skill provides guidance for LLM-driven execution.

For detailed methodology, see the original recipe or related skills.

## Related Skills

(Document related skills here during manual review)

---

**Status**: Migrated stub - needs manual review and enhancement
