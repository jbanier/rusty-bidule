# Recipe Migration Plan

**Date**: 2026-06-22

## Status

Recipes that need migration to skills:

### ip-reputation

**Title**: IP reputation check
**Description**: Determine whether an IP belongs to internal or related infrastructure, then gather exposure and reputation context for the asset.

**Action**: Convert workflow to skill instructions

### morning-routine

**Title**: Morning Shift Handover
**Description**: Summarize overnight CSIRT activity across collaboration, mail, calendar, and durable case memory for European shift handover.

**Action**: Convert workflow to skill instructions

### web-app-access-control

**Title**: Web App Access Control Review
**Description**: Assess IDOR/BOLA, vertical and horizontal authorization, forced browsing, method confusion, and object reference controls.

**Action**: Convert workflow to skill instructions

### web-app-active-baseline

**Title**: Web App Active Baseline
**Description**: Build a bounded active baseline with crawl inventory, endpoint map, parameter discovery, WAF observations, TLS checks, and safe scanner planning.

**Action**: Convert workflow to skill instructions

### web-app-api-graphql-websocket

**Title**: Web App API, GraphQL, And WebSocket Review
**Description**: Assess API and realtime surfaces including OpenAPI, GraphQL, WebSocket auth, object authorization, batching, replay, and tampering.

**Action**: Convert workflow to skill instructions

### web-app-auth-session

**Title**: Web App Auth And Session Review
**Description**: Assess authentication, password reset, MFA, cookies, JWT/OAuth, CSRF, account lockout, and session lifecycle posture.

**Action**: Convert workflow to skill instructions

### web-app-business-logic-race

**Title**: Web App Business Logic And Race Review
**Description**: Assess workflow abuse, state manipulation, replay, idempotency, race conditions, CAPTCHA bypass, sequential validation, payment logic, and domain-specific abuse cases.

**Action**: Convert workflow to skill instructions

### web-app-client-side-review

**Title**: Web App Client-Side Review
**Description**: Review JavaScript-heavy client-side attack surface with passive route discovery, CSP/SRI posture, DOM risk indicators, browser evidence, and shadow API candidates.

**Action**: Convert workflow to skill instructions

### web-app-cms-wordpress

**Title**: Web App CMS And WordPress Review
**Description**: Assess WordPress/CMS posture including version exposure, plugin/theme risk, user enumeration, XML-RPC, config leakage, and hardening.

**Action**: Convert workflow to skill instructions

### web-app-dependency-integrity

**Title**: Web App Dependency Integrity Review
**Description**: Inventory server and browser dependencies, third-party assets, SRI/pinning posture, and supplied SCA scanner output as scoped supply-chain leads.

**Action**: Convert workflow to skill instructions

### web-app-engagement-governance

**Title**: Web App Engagement Governance
**Description**: Maintain normalized engagement state, WSTG/API coverage, skipped checks, unresolved approvals, and validation-ready findings.

**Action**: Convert workflow to skill instructions

### web-app-error-crypto-posture

**Title**: Web App Error and Crypto Posture
**Description**: Review error handling and cryptographic posture from scoped HTTP evidence, supplied headers, TLS observations, CSP origins, and related host candidates.

**Action**: Convert workflow to skill instructions

### web-app-files-cache-host

**Title**: Web App Files, Cache, And Host Header Review
**Description**: Assess upload/download handling, MIME/extension validation, path traversal, cache poisoning/deception, host-header attacks, CORS, and clickjacking posture.

**Action**: Convert workflow to skill instructions

### web-app-final-report

**Title**: Web App Final Report
**Description**: Normalize web assessment findings into a concise report with evidence references, severity rationale, remediation, and retest checklist.

**Action**: Convert workflow to skill instructions

### web-app-hypothesis-loop

**Title**: Web App Hypothesis-Driven Testing Loop
**Description**: Iterative attack hypothesis generation, testing, and graph enrichment - replaces linear pipeline with intelligence-driven exploration

**Action**: Convert workflow to skill instructions

### web-app-input-validation

**Title**: Web App Input Validation Review
**Description**: Plan and track scoped checks for SQLi, NoSQLi, XSS, DOM XSS, command injection, path traversal, SSTI, XXE, SSRF, prototype pollution, and deserialization.

**Action**: Convert workflow to skill instructions

### web-app-passive-recon

**Title**: Web App Passive Recon
**Description**: Collect low-impact web posture evidence for scoped targets, including headers, cookies, TLS, DNS, technologies, exposed files, and public attack surface.

**Action**: Convert workflow to skill instructions

### web-app-scanner-normalization

**Title**: Web App Scanner Result Normalization
**Description**: Normalize authorized ZAP baseline and Nuclei output as scoped leads, dedupe results, map methodology categories, and validate report-ready findings.

**Action**: Convert workflow to skill instructions

### web-app-scope-intake

**Title**: Web App Scope Intake
**Description**: Capture authorization, targets, allowed hosts, credentials, constraints, exclusions, and reporting needs for a web application posture assessment.

**Action**: Convert workflow to skill instructions


## Migration Strategy

For each recipe:

1. **Extract core logic**:
   - Remove `Config:` section (tool access now skill-specific)
   - Remove `Workflow:` structure (LLM plans dynamically)
   - Keep `Instructions:` as main skill content
   - Convert `steps` to sequential guidance (not rigid phases)

2. **Create skill file**:
   - `skills/<recipe-name>/SKILL.md`
   - Use agentskills.io format
   - Add proper frontmatter (name, description, metadata)

3. **Handle dependencies**:
   - If recipe activates other skills → document in "Related Skills"
   - If recipe uses specific tools → add to `Tools:` section

4. **Test**:
   - Verify LLM can follow skill instructions
   - Compare results with original recipe execution

## Next Steps

Run:
```bash
# For each recipe needing migration:
# 1. Manually convert recipe/RECIPE.md to skill/SKILL.md
# 2. Test the skill
# 3. Mark recipe as deprecated
```

After all migrations:
```bash
# Move recipes to deprecated
mv recipes/ recipes_deprecated/

# Update .gitignore
echo "recipes_deprecated/" >> .gitignore
```
