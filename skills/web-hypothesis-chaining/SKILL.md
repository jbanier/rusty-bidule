---
name: web-hypothesis-chaining
description: Generates prioritized attack hypotheses from asset graph, past findings, and vulnerability taxonomy - the "Stratège" component of hypothesis-driven testing
metadata:
  keywords: hypothesis, chaining, strategy, graph, exploitation, business-logic
---

# Web Hypothesis Chaining

This skill is the **Stratège** in the hypothesis-driven testing loop. It reads the asset graph from investigation_memory, analyzes relationships between discovered entities (endpoints, roles, parameters, technologies), and generates prioritized attack hypotheses that chain findings together.

## Core Principle

**Findings are ingredients, not endpoints.** Each discovered asset or weakness should trigger questions:
- What does this reveal about the system?
- What other findings could combine with this?
- What assumptions does this challenge?
- What high-impact chains does this enable?

## Usage Pattern

This skill is called **after each significant finding** in the hypothesis testing loop, NOT as a fixed pipeline step.

Trigger conditions:
1. New asset discovered (endpoint, role, parameter type)
2. Finding confirmed (IDOR, lack of rate limiting, predictable ID)
3. Technology fingerprinted (framework, database, auth method)
4. Behavioral pattern observed (role differences, timing variance)

## Input: Investigation Memory Graph

Expects structured asset graph in investigation_memory with:

```json
{
  "asset_graph": {
    "endpoints": [
      {
        "path": "/api/orders/{id}",
        "methods": ["GET", "POST"],
        "auth_required": true,
        "roles_tested": ["user", "admin"],
        "parameters": [
          {"name": "id", "type": "numeric", "pattern": "sequential"}
        ],
        "observations": [
          "User role can access any order ID",
          "No tenant validation observed"
        ]
      }
    ],
    "roles": [
      {
        "name": "user",
        "permissions": ["read_orders", "create_orders"],
        "cross_tenant_tested": false
      }
    ],
    "technologies": {
      "backend": "Express.js",
      "database": "MongoDB",
      "auth": "JWT"
    },
    "findings": [
      {
        "id": "F001",
        "type": "IDOR",
        "severity": "medium",
        "target": "/api/orders/{id}",
        "status": "confirmed"
      }
    ]
  }
}
```

## Output: Prioritized Hypotheses

Generates attack chains with:
- **Hypothesis**: What to test
- **Rationale**: Why it might work (based on graph relationships)
- **Prerequisites**: What findings enable this
- **Impact**: Potential severity if confirmed
- **Test approach**: Concrete steps (tools, payloads, validation)

Example output:

```markdown
## Priority 1: Cross-Tenant IDOR Chain

**Hypothesis**: User role B can access tenant A's orders via /api/orders/{id}

**Rationale**:
- F001 confirmed: users can access any order ID (no horizontal authz)
- Role permissions don't mention tenant scoping
- JWT token doesn't contain tenant_id claim (observed in token decode)
- Database is MongoDB (NoSQL → tenant filtering often app-level, not DB-level)

**Prerequisites**: F001 (IDOR confirmed), two tenant accounts

**Impact**: Critical - complete tenant isolation bypass

**Test Approach**:
1. Create order as tenant_A user → capture order_id_A
2. Login as tenant_B user → request /api/orders/{order_id_A}
3. Validation: Response should be 403/404, if 200 → confirmed cross-tenant IDOR
4. Tools: `curl`, Burp Repeater
5. Proof: Compare order response with tenant_A's actual data

---

## Priority 2: JWT Algorithm Confusion Attack

**Hypothesis**: JWT validation accepts 'none' algorithm or HS256 public key

**Rationale**:
- Auth uses JWT (observed)
- Backend is Express.js (common vulnerable jwt libraries: jsonwebtoken <8.0.0)
- No rate limiting on /api/auth/verify (finding F002)
- If successful, combine with F001 for unauthenticated tenant data access

**Prerequisites**: None (testable immediately)

**Impact**: Critical - authentication bypass → combine with IDOR for full breach

**Test Approach**:
1. Capture valid JWT token
2. Test 1: Decode, change alg to "none", remove signature, replay
3. Test 2: Decode, change alg to "HS256", sign with public key (if RSA keys leaked)
4. Tools: `jwt_tool`, `burp-jwt-extension`
5. Validation: 200 response on protected endpoint = bypass confirmed
```

## Critical Questions to Always Ask

After EVERY finding update, the Stratège must ask:

1. **What does this reveal about trust boundaries?**
   - If parameter X is user-controlled, what server-side assumptions might break?
   - If role A can do Y, what should role B NOT be able to do?

2. **What chains does this enable?**
   - If we can bypass X, what previously-blocked attack becomes viable?
   - Which findings share the same root cause? (e.g., all lack tenant filtering)

3. **What wasn't tested yet that this makes plausible?**
   - New endpoint patterns to probe
   - New role combinations to test
   - New parameter injection contexts

4. **What would a senior pentester test next?**
   - Business-critical flows (payment, data export, admin functions)
   - Time-based attacks (race conditions, session timing)
   - Abuse cases (bulk operations, edge limits)

## Integration with Taxonomy

Load `web-vulnerability-taxonomy` to:
- Map observed weaknesses to CWE/OWASP classes
- Identify related vulnerability patterns (if XSS exists, test for DOM XSS, CSP bypass)
- Check coverage gaps (tested for SQLi but not NoSQLi?)

## Integration with Business Logic Skills

Activate after generating hypotheses:
- `web-access-control-matrix`: If role-based hypotheses generated
- `web-api-graphql`: If GraphQL endpoints discovered
- `web-business-logic-race`: If stateful operations observed

## Anti-Pattern: Don't Generate Generic Checklists

**Bad hypothesis**: "Test for XSS on all input fields"
- No context, no prioritization, no chaining

**Good hypothesis**: "Test for stored XSS in order notes field + CSP bypass"
- Why: Order notes are displayed to admins (privilege escalation)
- Chain: If XSS works + CSP is weak, could exfiltrate admin session
- Priority: High impact, specific target

## Tools

Tools:
  - name: Generate Hypotheses
    slug: generate-hypotheses
    description: Read asset graph from investigation_memory, analyze relationships, generate prioritized attack chains
    script: scripts/generate_hypotheses.py
    network: false

  - name: Update Asset Graph
    slug: update-asset-graph
    description: Add newly discovered assets/findings to graph, preserve relationships
    script: scripts/update_asset_graph.py
    network: false

  - name: Chain Analyzer
    slug: chain-analyzer
    description: Given multiple findings, identify potential chains and combined impact
    script: scripts/chain_analyzer.py
    network: false

## Example Loop Integration

```
1. Mapper discovers: /api/users/{id} endpoint, user/admin roles
   → Update asset graph

2. Stratège generates hypothesis: "Admin can access /api/users/{any_id}, test if user role has same access"
   → Priority: High (IDOR + vertical privilege escalation)

3. Exploiteur tests hypothesis: User can access admin IDs
   → Confirmed: F001 (vertical IDOR)

4. Stratège re-generates: "F001 enables account takeover via PATCH /api/users/{admin_id}"
   → Chain: IDOR + write access = full compromise

5. Exploiteur tests: PATCH blocked (PUT accepted)
   → Confirmed: F002 (verb tampering bypasses write control)

6. Stratège re-generates: "F001+F002 = full admin takeover, test for mass assignment"
   → Chain impact: Critical

7. Exploiteur tests: Can set is_admin=true via PUT
   → Confirmed: F003 (mass assignment)

RESULT: Three findings chain into critical exploit path
```

## Stop Conditions

Generate hypotheses until:
1. **No high/critical priority hypotheses remain** (only low-impact tests left)
2. **Budget exhausted** (turn limit, time constraint)
3. **Scope boundary reached** (next hypothesis requires out-of-scope systems)
4. **Diminishing returns** (last 5 hypotheses all blocked/invalid)

## Self-Critique Trigger

Before final report, ask:
- "What combinations of findings haven't been explored?"
- "Are there isolated findings that could connect?"
- "What would make each medium-severity finding critical?"

## Output Format

Always structure as:

```markdown
# Hypothesis Generation - [Timestamp]

## Context
- Total assets mapped: X endpoints, Y roles, Z parameters
- Findings so far: N confirmed, M leads
- Last update: [what triggered this generation]

## New Hypotheses (Priority 1 → N)

[Hypothesis blocks as shown above]

## Findings Requiring Re-Test
- F001: Re-test with new role discovered
- F003: Chain with F007 for impact escalation

## Coverage Gaps
- No NoSQL injection tested yet (MongoDB backend)
- Cross-tenant isolation not validated on /api/reports/*

## Recommended Next Action
[Single highest-priority hypothesis to test]
```

---

**This skill transforms linear scanning into intelligent exploitation.**
