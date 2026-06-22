---
name: web-active-baseline-testing
description: Build a bounded active baseline with crawl inventory, endpoint map, parameter discovery, WAF observations, TLS checks, and safe scanner planning.
metadata:
  keywords: web, active, crawl, baseline, scanner
---

# Web App Active Baseline

Confirm validated scope and active testing authorization before running any active tool. Respect rate limits and blackout windows.

Use:
- `web-crawler-inventory` for bounded same-scope crawling,
- `web-directory-enum` for directory and content discovery with ffuf or feroxbuster,
- `web-discovery-recon` for tool availability and command planning,
- `web-scanner-safe` for conservative nuclei/ZAP baseline plans.

Do not run destructive templates, brute force, denial-of-service checks, or WAF evasion. Scanner findings are leads until manually confirmed.

## Migration Note

This skill was migrated from the `web-app-active-baseline` recipe.

The original recipe used a declarative workflow. This skill provides guidance for LLM-driven execution.

For detailed methodology, see the original recipe or related skills.

## Related Skills

(Document related skills here during manual review)

---

**Status**: Migrated stub - needs manual review and enhancement
