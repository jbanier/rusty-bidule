---
name: web-passive-recon
description: Collect low-impact web posture evidence for scoped targets, including headers, cookies, TLS, DNS, technologies, exposed files, and public attack surface.
metadata:
  keywords: web, recon, passive, headers, tls, cookies
---

# Web App Passive Recon

Read investigation memory first and confirm validated scope. If scope is missing, switch to `web-app-scope-intake`.

Use:
- `web-http-baseline` for security headers, cookies, redirects, CORS, and cache hints,
- `web-discovery-recon` for a scoped recon command plan and installed tool inventory,
- existing `nmap` only when active testing is authorized and the target host is in scope.

Treat missing headers, permissive CORS, weak cookies, outdated TLS, and exposed metadata as posture observations unless impact is confirmed.

## Migration Note

This skill was migrated from the `web-app-passive-recon` recipe.

The original recipe used a declarative workflow. This skill provides guidance for LLM-driven execution.

For detailed methodology, see the original recipe or related skills.

## Related Skills

(Document related skills here during manual review)

---

**Status**: Migrated stub - needs manual review and enhancement
