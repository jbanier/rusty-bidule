---
name: web-hypothesis-driven-testing
description: Iterative attack hypothesis generation, testing, and graph enrichment - intelligence-driven security testing that discovers attack chains
metadata:
  keywords: hypothesis, iterative, chaining, graph, strategy, idor, authorization
---

# Web Hypothesis-Driven Testing

Conduct comprehensive penetration testing using an iterative hypothesis loop that discovers attack chains by treating findings as ingredients, not endpoints.

## Purpose

Replace linear scan-and-report workflows with intelligent, adaptive testing that:
- Discovers relationships between vulnerabilities
- Identifies multi-hop attack chains
- Adapts testing based on discoveries
- Prioritizes high-impact exploitation paths

**Use when**: Conducting thorough web application security assessments where attack chain discovery is critical.

## Core Principles

1. **Findings are ingredients, not endpoints** - Each result triggers new hypotheses
2. **Relationships matter** - Graph structure enables chain discovery  
3. **Test then update** - Never continue with stale hypothesis list
4. **Business logic over scanner noise** - Targeted tests beat enumeration
5. **Critical self-review** - Pause before report to find missed chains

## Testing Approach

### Step 1: Build Asset Graph

Activate `web-asset-mapper` to discover and map:
- **Endpoints**: Paths, methods, parameters
- **Roles**: Permissions, privilege boundaries
- **Technologies**: Framework, database, auth method
- **Relationships**: Object references, shared ID namespaces, foreign keys

**Critical**: Focus on *relationships* - what links to what, which parameters appear across endpoints, which roles see different fields.

Store in `investigation_memory.asset_graph`.

**Example**:
```
Discovered:
- /api/orders/{id} → contains user_id field
- /api/users/{id} → user profile endpoint
- Relationship: orders.user_id references users.id
→ Hypothesis: Multi-hop IDOR possible (orders → users)
```

### Step 2: Generate Hypotheses (Stratège)

Activate `web-hypothesis-chaining` to generate prioritized attack hypotheses based on:
- Discovered relationships (object references, ID patterns)
- Role permission gaps (what's tested vs untested)
- Technology implications (framework CVEs, DB-specific attacks)
- Previous findings (what chains are now enabled?)

Optionally activate `web-vulnerability-taxonomy` for coverage tracking.

**Output**: Ranked hypothesis list (Priority 1 = highest impact/likelihood)

**Example**:
```
Generated hypotheses:
1. H001 (Priority 1): Vertical IDOR on /api/orders - user can access admin orders
2. H002 (Priority 1): Cross-tenant isolation via orders → users chain
3. H003 (Priority 2): Mass assignment on /api/users (lower priority)
```

### Step 3: Test Hypothesis Loop

**For each hypothesis** (starting with Priority 1):

#### 3a. Validate Prerequisites
- Is target in scope? (check investigation_memory.scope)
- Are prerequisites met? (test accounts, prior findings)
- Is test type authorized? (destructive, OOB, rate limits)

If blocked → mark as deferred, move to next hypothesis.

#### 3b. Execute Test
Activate `web-hypothesis-tester` with selected hypothesis.

Perform targeted test:
- Follow hypothesis test approach
- Collect structured proof (requests, responses, validation)
- Return result: confirmed / blocked / unclear

#### 3c. Record Finding
For confirmed vulnerabilities:
- Assign finding ID (F001, F002, etc.)
- Classify severity (critical/high/medium/low)
- Map to CWE/OWASP using `web-vulnerability-taxonomy`
- Store in investigation_memory.findings

#### 3d. Update Asset Graph
**Critical step**: Enrich graph with test result:
- Add finding to graph
- Update endpoint/role observations
- Add new relationships discovered
- Flag uncertainties resolved

Save updated graph to investigation_memory.

#### 3e. Re-Generate Hypotheses
**CRITICAL**: After EVERY significant finding, re-run Stratège.

New findings change the graph → new hypotheses become viable → priorities shift.

**Do NOT** continue with old hypothesis list!

**Example**:
```
Iteration 1:
  Test H001 → Confirmed F001 (IDOR on orders)
  Update graph with F001
  Re-generate hypotheses:
    - H004 (NEW, Priority 1): F001 enables write access test
    - H005 (NEW, Priority 1): Chain F001 with mass assignment
  
Iteration 2:
  Test H004 → Confirmed F002 (IDOR write access)
  Update graph with F002
  Re-generate hypotheses:
    - H006 (NEW, Priority 1): F001+F002 enable refund fraud
...
```

### Step 4: Activate Context-Specific Skills

When hypotheses reference specific attack types, activate relevant skills:

- **Authorization issues** → `web-access-control-matrix`
- **GraphQL endpoints** → `web-api-graphql`
- **Race conditions** → `web-business-logic-race`
- **Session/auth** → `web-auth-session-auditor`
- **Client-side** → `web-client-side-audit`

These skills run **when triggered by discoveries**, not at fixed positions.

### Step 5: Loop Until Stop Condition

Continue hypothesis loop until:
1. **No high/critical hypotheses remain** (only low-priority left)
2. **Budget exhausted** (time/iteration limit)
3. **Scope boundary reached** (next hypothesis out of scope)
4. **Diminishing returns** (last 5 tests all blocked/negative)

### Step 6: Chain Critic Review

**Before finalizing report**, activate `web-chain-critic` for adversarial self-review.

Critical questions:
1. What findings connect? (same type, same flow, shared weakness)
2. What chains were missed? (combinations not tested)
3. What business-critical paths untested? (payment, admin, export)
4. What escalates isolated findings? (medium → critical when chained)
5. What would senior pentester test next?

**Output**:
- Potential chains to re-test
- Severity escalations (findings that combine for higher impact)
- Coverage gaps (high-priority untested areas)
- Attack path diagrams

**Example**:
```
Critic identified:
- F001 (IDOR) + F003 (mass assignment) = Critical privilege escalation
  (individually medium, chained = critical)
- Coverage gap: Payment refund endpoint not tested with F001 context
- Recommended: Test F001 + refund = potential fraud
```

### Step 7: Generate Report

Produce comprehensive report with:
- **Executive summary**: Critical findings, business impact, attack chains
- **Asset graph**: Visualization of discovered relationships
- **Findings**: With CWE/OWASP mappings, proof, reproduction
- **Attack chains**: Diagrams showing multi-hop exploitation
- **Business impact**: Compliance scope (GDPR, PCI), data volume, financial risk
- **Coverage**: OWASP Top 10 checklist, WSTG sections tested
- **Remediation**: Prioritized by severity and business risk

Emphasize **chains** over isolated findings.

## Related Skills

**Core loop skills** (always used):
- `web-asset-mapper`: Build asset graph
- `web-hypothesis-chaining`: Generate hypotheses
- `web-hypothesis-tester`: Execute tests
- `web-chain-critic`: Final adversarial review

**Context-specific skills** (activate when triggered):
- `web-access-control-matrix`: Authorization analysis
- `web-api-graphql`: GraphQL testing
- `web-business-logic-race`: Race condition testing
- `web-auth-session-auditor`: Session security
- `web-client-side-audit`: Frontend vulnerabilities
- `web-vulnerability-taxonomy`: Coverage tracking

## Example Execution

```
User: "Conduct hypothesis-driven testing on https://api.example.com"

LLM (activates this skill):

[Step 1: Asset Mapping]
Activating web-asset-mapper...
✅ Discovered: 47 endpoints, 3 roles (user, admin, guest)
✅ Key relationships: orders → users, payments → orders
✅ Technologies: Express.js, MongoDB, JWT

[Step 2: Generate Hypotheses]
Activating web-hypothesis-chaining...
✅ Generated 12 hypotheses
Top 3:
  H001 (Priority 1): Vertical IDOR on /api/orders
  H002 (Priority 1): Cross-tenant via orders → users chain
  H003 (Priority 1): Mass assignment on /api/users

[Step 3: Test Loop - Iteration 1]
Testing H001 (Vertical IDOR)...
Activating web-hypothesis-tester...
  Created order as admin → ID 5678
  Accessed as user → 200 OK with admin data
✅ CONFIRMED: F001 (Critical IDOR)

Updating asset graph with F001...
Re-generating hypotheses...
  NEW: H004 (Priority 1) - F001 enables write access test

[Step 3: Test Loop - Iteration 2]
Testing H004 (IDOR write)...
  PUT /api/orders/5678 as user
✅ CONFIRMED: F002 (IDOR write access)

Re-generating hypotheses...
  NEW: H005 (Priority 1) - F001+F002 enable privilege escalation

[Step 3: Test Loop - Iteration 3]
Testing H005 (Privilege escalation)...
  F002 allows modifying admin order
  Testing mass assignment...
✅ CONFIRMED: F003 (Mass assignment to is_admin field)

[... loop continues ...]

[Step 3: Stop Condition Met]
No Priority 1 hypotheses remain. 8 findings confirmed.

[Step 6: Chain Critic]
Activating web-chain-critic...
✅ Identified 3 critical chains:
  1. F001+F002+F003 = Complete privilege escalation
  2. F004+F005 = Cross-tenant data access
  3. F006+F007+F008 = Payment fraud via IDOR chain

Severity escalations:
  - F002: Medium → Critical (when chained with F001)

[Step 7: Report]
Final results:
- 8 vulnerabilities found
- 3 critical attack chains identified
- OWASP A01 (Broken Access Control): 5 findings
- Business impact: Complete tenant isolation bypass + privilege escalation

Report stored in investigation_memory.final_report
```

## Key Differences from Linear Testing

| Linear Approach | Hypothesis-Driven |
|----------------|-------------------|
| Recon → Scan → Report | Map → Hypothesize → Test → Update → Loop |
| Findings independent | Findings trigger new hypotheses |
| Scanner-driven | Intelligence-driven |
| Breadth-first | Depth-first on high-impact |
| Fixed sequence | Adapts to discoveries |
| Isolated findings | Attack chains emphasized |

## Tips for Success

1. **Always re-generate after findings**: Stale hypothesis lists miss chains
2. **Focus on relationships**: Graph connections reveal exploitation paths
3. **Activate critic before report**: Catches 80% of missed chains
4. **Document uncertainties**: "Assumed X but not confirmed" → test it
5. **Respect stop conditions**: Don't waste time on low-priority when budget low

---

**This skill transforms linear scanning into intelligent exploitation discovery.**
