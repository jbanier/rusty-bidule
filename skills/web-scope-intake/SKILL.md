---
name: web-scope-intake
description: Capture authorization, targets, allowed hosts, credentials, constraints, exclusions, and reporting needs for a web application posture assessment.
metadata:
  keywords: web, pentest, scope, authorization, intake
---

# Web App Scope Intake

Use this recipe before any active web assessment work.

Collect or confirm:
- target URLs and allowed hosts,
- authorization for active testing,
- explicitly excluded tests,
- credentials and roles available for testing,
- rate limits and blackout windows,
- OOB callback authorization,
- destructive/high-impact testing authorization,
- reporting format and finding severity expectations.

Run `web-scope-guard` with `tool_slug="validate-scope"` once enough scope details are available. Store the returned `investigation_memory_patch` with `local__update_investigation_memory`.

Do not run scanners, crawlers, fuzzers, or payload probes from this recipe.

## Migration Note

This skill was migrated from the `web-app-scope-intake` recipe.

The original recipe used a declarative workflow. This skill provides guidance for LLM-driven execution.

For detailed methodology, see the original recipe or related skills.

## Related Skills

(Document related skills here during manual review)

---

**Status**: Migrated stub - needs manual review and enhancement
