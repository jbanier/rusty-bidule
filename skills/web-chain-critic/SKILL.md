---
name: web-chain-critic
description: Final review pass to identify missed chains, combined impacts, and untested critical paths before report generation
metadata:
  keywords: critic, review, chains, combinations, synthesis, final-check
---

# Web Chain Critic

**The final sanity check before closing an engagement.** This skill performs adversarial self-review to catch chains the hypothesis loop might have missed.

## Philosophy: What Would a Senior Pentester Ask?

After completing hypothesis testing, a senior pentester would pause and ask:

1. **"What did we miss?"**
   - Findings tested in isolation but not combined
   - Obvious next steps that weren't followed
   - High-value targets that didn't get hypotheses generated

2. **"What makes each finding critical?"**
   - Currently marked medium/low
   - But could escalate when chained
   - Business context adds impact

3. **"What's the worst-case scenario?"**
   - Full attack path from anonymous to admin
   - Data exfiltration to compliance breach
   - Financial impact of chained findings

## When to Use

**Trigger**: Before generating final report

**Input**: Complete asset graph + all findings

**Output**: List of missed chains, escalated findings, and critical paths to re-test

## Critical Questions

### Question 1: Isolated Findings That Connect

Review all findings and ask:

**"Which findings share a common weakness?"**

Example:
```
F001: IDOR on /api/orders/{id}
F005: IDOR on /api/users/{id}
F012: IDOR on /api/payments/{id}

❌ Reported separately: 3 medium-severity findings

✅ Chained insight: Systemic authorization failure
   → Test if orders.user_id → users → payments creates full data access chain
   → Escalate to CRITICAL: Complete database enumeration via object reference chain
```

**"Which findings appear on the same critical business flow?"**

Example:
```
F003: XSS in order notes field (medium)
F008: No CSP header (low)
F011: Admin panel displays user order notes (info)

❌ Reported separately: 1 medium + 1 low + 1 info

✅ Chained: Stored XSS → Admin reads it → Session hijack → Full compromise
   → Escalate to CRITICAL: Privilege escalation via stored XSS
```

### Question 2: Findings + Business Context

**"What's the business-critical operation we haven't fully tested?"**

Common high-value targets:
- Payment flows (create/modify/refund)
- User registration / password reset
- Data export / bulk operations
- Admin functions (user management, config changes)
- OAuth flows / SSO integration

Example:
```
Asset graph shows: /api/payments/refund endpoint exists
Findings so far: F001 (IDOR on orders)
Tested: Read access to orders

❌ Missed: Write operations on payments
   → Hypothesis gap: "Can user trigger refund on any order via F001 IDOR?"

✅ New test: POST /api/payments/refund with other user's order_id
   → Result: CRITICAL - Arbitrary refund via IDOR chain
```

### Question 3: Privilege Escalation Paths

**"Can we map a path from anonymous → user → admin?"**

Review:
- Authentication bypasses
- Registration / invite flows
- Role-based field differences
- Admin-only endpoints discovered

Example:
```
F002: Self-registration enabled (info)
F007: Mass assignment on /api/users (medium)
F015: Admin role check is client-side only (medium)

❌ Reported separately: Mixed severity

✅ Chain test:
   1. Register account (F002)
   2. Set is_admin=true via mass assignment (F007)
   3. Access /api/admin/* (bypasses client-side check F015)
   → Result: CRITICAL - Anonymous to admin in 3 steps
```

### Question 4: Data Exfiltration Chains

**"What's the full extent of data accessible if all findings are chained?"**

Map:
- What data each finding exposes
- What findings enable lateral movement
- Combined scope of accessible data

Example:
```
F001: IDOR on orders → see all order data
F005: IDOR on users → see all user PII
F009: orders.user_id links to users
F014: Sequential IDs (range 1-9999)

❌ Reported impact: "Can view individual records"

✅ Chained impact calculation:
   - 9999 orders * average 2 users/order = ~20k user records
   - PII includes: email, phone, address, payment method
   - GDPR/PCI scope: 20k records = mandatory breach notification
   → Escalate to CRITICAL: Mass PII exfiltration + compliance violation
```

### Question 5: Untested High-Priority Hypotheses

**"What did we run out of time to test that could be critical?"**

Review:
- Hypotheses marked "deferred" due to budget
- Endpoints discovered but not tested
- Role combinations not validated

Example:
```
Asset graph shows:
- /api/webhooks/configure endpoint (discovered, not tested)
- Admin role exists (confirmed)
- Webhook URL parameter (observed)

Hypothesis generated but not tested (budget limit):
- H087: "Test if webhook URL accepts internal addresses (SSRF)"

❌ Skipped due to low priority in initial ranking

✅ Critic re-evaluates:
   - Webhooks often fetch URLs server-side
   - Admin panel might have loose validation
   - SSRF on admin context = high impact
   → Escalate priority, test before finalizing report
```

## Methodology

### Step 1: Build Finding Matrix

```
| Finding ID | Type | Severity | Target | Prerequisites | Impact |
|------------|------|----------|--------|---------------|--------|
| F001 | IDOR | Medium | /api/orders/{id} | User account | Read orders |
| F005 | IDOR | Medium | /api/users/{id} | User account | Read users |
| F007 | Mass Assignment | Medium | /api/users | User account | Modify user fields |
```

### Step 2: Identify Patterns

- **Pattern 1**: Same type (3 IDOR findings)
- **Pattern 2**: Same target family (/api/*)
- **Pattern 3**: Same prerequisite (all need user account)

### Step 3: Test Combinations

For each pattern, generate chain hypothesis:

```
Pattern: 3 IDOR findings

Chain test:
- F001 gives access to order → order.user_id field
- F005 gives access to user → user.payment_method_id field  
- Test: Can we traverse full graph via object references?

Result:
- Confirmed: /api/orders/1 → user_id=5 → /api/users/5 → payment_method_id=99 → /api/payment-methods/99
- Impact: Complete data graph traversal from single entry point
- New finding: F020 - Chained IDOR enabling full database enumeration
```

### Step 4: Business Impact Mapping

```
Technical finding: F001 (IDOR on orders)
   ↓
Business context: Orders contain payment card last 4 digits
   ↓
Compliance scope: PCI DSS violation if accessed by unauthorized user
   ↓
Escalated severity: Medium → High (compliance risk)
```

### Step 5: Generate Re-Test List

```markdown
## Critical Gaps Found

### 1. Untested Chain: IDOR → Mass Assignment → Privilege Escalation
- F001 + F007 combination not tested
- High impact if confirmed
- **Recommendation**: Test before finalizing report

### 2. Business Logic Path: Payment Refund
- /api/payments/refund endpoint exists in graph
- Not tested with IDOR context
- **Recommendation**: Test if F001 enables arbitrary refunds

### 3. Isolated Finding Re-Evaluation: XSS + CSP
- F003 (XSS) currently medium
- F008 (no CSP) currently low
- **Recommendation**: Test stored XSS on admin panel for escalation path
```

## Tools

Tools:
  - name: Chain Analyzer
    slug: analyze-chains
    description: Review all findings, identify potential chains and combined impacts
    script: scripts/analyze_chains.py
    network: false

  - name: Coverage Gap Detector
    slug: coverage-gaps
    description: Compare asset graph against WSTG/OWASP coverage, identify untested high-priority areas
    script: scripts/coverage_gaps.py
    network: false

  - name: Impact Calculator
    slug: impact-calculator
    description: Calculate combined impact of chained findings (data volume, compliance scope, business risk)
    script: scripts/impact_calculator.py
    network: false

  - name: Senior Review Simulator
    slug: senior-review
    description: Apply heuristics from senior pentester review patterns to identify missed opportunities
    script: scripts/senior_review.py
    network: false

## Output Format

```markdown
# Chain Critic Review - [Timestamp]

## Executive Summary
- 15 findings reported
- 4 potential chains identified
- 2 high-priority gaps require re-testing
- 3 severity escalations recommended

---

## Potential Chains Identified

### Chain 1: IDOR Object Reference Traversal (CRITICAL)
**Findings**: F001, F005, F009
**Current severity**: 3x Medium
**Chained severity**: Critical

**Attack path**:
1. F001: Access any order via /api/orders/{id}
2. Extract user_id from order response
3. F005: Access user via /api/users/{user_id}
4. Extract payment_method_id from user response
5. Access payment method (assumed accessible based on pattern)

**Impact**: Complete database enumeration via object reference chain

**Recommendation**: ✅ Test full chain before report

---

### Chain 2: Stored XSS → Admin Session Hijack (CRITICAL)
**Findings**: F003, F008, F011
**Current severity**: Medium + Low + Info
**Chained severity**: Critical

**Attack path**:
1. F003: Inject XSS payload in order notes
2. F011: Admin views order in admin panel
3. F008: No CSP → XSS executes
4. Exfiltrate admin session token

**Impact**: Full admin compromise via stored XSS

**Recommendation**: ✅ Test XSS payload visibility in admin context

---

## Coverage Gaps (High Priority)

### Gap 1: Payment Operations Not Tested
**Asset**: /api/payments/refund endpoint
**Context**: F001 IDOR confirmed on orders
**Risk**: IDOR might enable arbitrary refunds
**Recommendation**: Test POST /api/payments/refund with victim order_id

### Gap 2: Webhook SSRF Not Tested
**Asset**: /api/webhooks/configure endpoint  
**Context**: Admin role, URL parameter observed
**Risk**: SSRF on admin context = internal network access
**Recommendation**: Test webhook URL with internal address

---

## Severity Escalations Recommended

### F001: IDOR on Orders (Medium → Critical)
**Reason**: Chains with F005 and F009 for full data enumeration
**New impact**: 20k+ PII records accessible, GDPR breach scope

### F003: XSS in Order Notes (Medium → Critical)
**Reason**: Chains with F008 and F011 for admin session hijack
**New impact**: Full admin compromise

---

## Findings Requiring Re-Test

1. **F001 + F007**: Test if IDOR enables mass assignment on other users
2. **F003**: Confirm XSS payload executes in admin panel context
3. **Payment refund**: Test with F001 IDOR context

---

## Recommended Actions Before Report

1. ✅ Test 3 critical chains identified above
2. ✅ Test 2 high-priority coverage gaps
3. ✅ Update severity for 2 escalated findings
4. ⚠️ Consider extending engagement if budget allows (4 more hypotheses generated)

---

## Coverage Summary

**OWASP Top 10 Coverage**:
- A01 (Broken Access Control): ✅ Well tested (IDOR chains)
- A02 (Cryptographic Failures): ⚠️ Partial (TLS tested, encryption at rest not tested)
- A03 (Injection): ✅ Tested (SQLi, XSS, NoSQLi)
- A07 (Authentication): ⚠️ Partial (JWT tested, MFA/password reset not tested)

**High-Value Business Flows**:
- ✅ Order creation
- ✅ User registration
- ⚠️ Payment operations (partial)
- ❌ Admin user management (not tested)
- ❌ Data export (not tested)
```

## Integration with Final Report

After critic review:

1. **Re-test critical chains** identified
2. **Update finding severity** based on chain analysis
3. **Add chain diagrams** to report (attack path visualization)
4. **Highlight missed coverage** in limitations section

Don't generate final report until critic review is complete!

## Anti-Patterns

❌ **Don't**: Run critic review during testing (too early)
✅ **Do**: Run after hypothesis loop exhausts, before report

❌ **Don't**: Accept critic output without re-testing
✅ **Do**: Treat critic output as new high-priority hypotheses

❌ **Don't**: Only look for positive chains
✅ **Do**: Also identify defense-in-depth (where chains are blocked)

---

**The critic catches what the loop missed. One critical chain beats ten isolated mediums.**
