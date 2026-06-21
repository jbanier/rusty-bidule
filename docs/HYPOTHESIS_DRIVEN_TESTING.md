

 # Hypothesis-Driven Web Application Testing

**Architecture**: Iterative intelligence-driven exploitation replacing linear scan pipelines

---

## Philosophy

Traditional web app pentesting follows a linear pipeline:
```
Recon → Scan → Exploit → Report
```

This approach treats findings as **endpoints** - each scanner result is reported independently.

**Problem**: Critical attack chains are missed because findings aren't combined.

---

## The Hypothesis Loop

Hypothesis-driven testing treats findings as **ingredients**:

```
┌─────────────────────────────────────────────────┐
│  Mapper: Build Asset Graph                      │
│  - Endpoints, roles, parameters                 │
│  - Relationships (what links to what)           │
│  - Technologies, patterns                       │
└──────────────────┬──────────────────────────────┘
                   ▼
     ┌─────────────────────────────┐
     │   Asset Graph (Memory)      │
     │   - Entities                │
     │   - Relationships           │
     │   - Findings                │
     └──────────────┬──────────────┘
                    ▼
     ┌──────────────────────────────────────────┐
     │  Stratège: Generate Hypotheses           │
     │  - Read graph + relationships            │
     │  - Identify chains                       │
     │  - Prioritize by impact                  │
     └──────────────┬───────────────────────────┘
                    ▼
     ┌──────────────────────────────────────────┐
     │  Select Priority 1 Hypothesis            │
     └──────────────┬───────────────────────────┘
                    ▼
     ┌──────────────────────────────────────────┐
     │  Exploiteur: Test Hypothesis             │
     │  - Targeted test (not blind scan)        │
     │  - Collect proof                         │
     │  - Return structured result              │
     └──────────────┬───────────────────────────┘
                    ▼
          ┌────────────────────┐
          │  Confirmed?        │
          └────┬──────────┬────┘
               │          │
          YES  │          │  NO
               ▼          ▼
     ┌─────────────┐  ┌──────────────┐
     │ Add Finding │  │ Mark Blocked │
     │ to Graph    │  │ Try Next H   │
     └──────┬──────┘  └──────┬───────┘
            │                │
            └────────┬───────┘
                     ▼
          ┌────────────────────────┐
          │  Update Asset Graph    │
          │  - New findings        │
          │  - New relationships   │
          └───────────┬────────────┘
                      ▼
          ┌────────────────────────────┐
          │  RE-GENERATE Hypotheses    │
          │  (Graph changed, new       │
          │   chains now possible)     │
          └───────────┬────────────────┘
                      │
                      └──────┐
                             ▼
            ┌───────────────────────────────┐
            │  Loop Until Stop Condition:   │
            │  - No critical H remain       │
            │  - Budget exhausted           │
            │  - Scope boundary reached     │
            └───────────┬───────────────────┘
                        ▼
         ┌──────────────────────────────────┐
         │  Chain Critic: Final Review      │
         │  - Missed chains?                │
         │  - Severity escalations?         │
         │  - Coverage gaps?                │
         └───────────┬──────────────────────┘
                     ▼
         ┌──────────────────────────────────┐
         │  Final Report with Attack Chains │
         └──────────────────────────────────┘
```

**Key difference**: Stratège **regenerates after each finding**, enabling chain discovery.

---

## Components

### 1. Mapper (`web-asset-mapper`)

**Role**: Build relational asset graph

**Output**:
```json
{
  "endpoints": [...],
  "roles": [...],
  "parameters": [...],
  "technologies": {...},
  "relationships": [
    {
      "type": "foreign_key",
      "from": "/api/orders/{id}",
      "to": "/api/users/{user_id}",
      "field": "order.user_id"
    }
  ]
}
```

**Key insight**: Captures **relationships**, not just flat lists.

### 2. Stratège (`web-hypothesis-chaining`)

**Role**: Generate prioritized attack hypotheses

**Input**: Asset graph + findings

**Output**: Ranked hypothesis list
```json
{
  "id": "H001",
  "priority": 1,
  "type": "IDOR",
  "target": "/api/orders/{id}",
  "hypothesis": "User can access admin orders",
  "rationale": [
    "Sequential IDs observed",
    "No authz check confirmed",
    "Admin sees extra fields"
  ],
  "test_approach": {...},
  "impact_if_confirmed": "critical"
}
```

**Critical behaviors**:
- Reads **relationships** to identify chains
- Runs **after each finding** (not once at start)
- Generates **targeted** hypotheses, not generic checklists

### 3. Exploiteur (`web-hypothesis-tester`)

**Role**: Execute targeted tests

**Input**: Single hypothesis

**Output**: Structured result with proof
```json
{
  "hypothesis_id": "H001",
  "status": "confirmed",
  "finding_id": "F001",
  "severity": "critical",
  "evidence": {...},
  "graph_updates": {...},
  "new_hypotheses_triggered": [...]
}
```

**Key principle**: Surgical strikes, not shotgun scanning.

### 4. Chain Critic (`web-chain-critic`)

**Role**: Final adversarial review

**Questions**:
1. What findings connect?
2. What chains were missed?
3. What business-critical paths untested?
4. What escalates isolated findings?

**Output**: List of missed chains to re-test before final report.

---

## Example: Finding a Critical Chain

### Linear Pipeline (Misses Chain)

```
1. Run nuclei → Finding: IDOR on /api/orders
   Report: "Medium severity IDOR"

2. Run nuclei → Finding: Mass assignment on /api/users
   Report: "Medium severity mass assignment"

3. Generate report → 2 medium findings

❌ MISSED: IDOR + mass assignment = privilege escalation
```

### Hypothesis Loop (Discovers Chain)

```
Iteration 1:
  Mapper: /api/orders/{id} has sequential IDs
  Stratège: H001 "Test IDOR on orders"
  Exploiteur: ✅ Confirmed F001 (IDOR)
  Graph Update: F001 added

Iteration 2:
  Stratège: Re-generates based on F001
            H002 "F001 IDOR exposes user_id field → test /api/users/{user_id}"
  Exploiteur: ✅ Confirmed F002 (Chained IDOR to users)
  Graph Update: F001 + F002 chain

Iteration 3:
  Stratège: Re-generates based on F002
            H003 "F002 gives user access → test PATCH /api/users for mass assignment"
  Exploiteur: ✅ Confirmed F003 (Mass assignment allows is_admin=true)
  Graph Update: F001 + F002 + F003 = CRITICAL CHAIN

Chain Critic:
  ✅ Attack path: Anonymous → IDOR orders → IDOR users → Set is_admin → Full compromise
  Severity: CRITICAL (was 2 mediums in linear approach)
```

---

## Recipe: `web-app-hypothesis-loop`

**Location**: `recipes/web-app-hypothesis-loop/RECIPE.md`

**Phases**:
1. **Initial Mapping**: Build asset graph
2. **Hypothesis Loop**: Iterate until stop condition
3. **Chain Critic Review**: Final adversarial check
4. **Final Report**: Chains + impacts + remediation

**Stop conditions**:
- No high/critical hypotheses remain
- Budget exhausted
- Scope boundary reached
- Diminishing returns (5 consecutive blocks)

---

## Skills

| Skill | Role | Key Tool |
|-------|------|----------|
| `web-asset-mapper` | Mapper | `build_asset_graph.py` |
| `web-hypothesis-chaining` | Stratège | `generate_hypotheses.py` |
| `web-hypothesis-tester` | Exploiteur | `test_hypothesis.py` |
| `web-chain-critic` | Critic | `analyze_chains.py` |

**Supporting skills** (activated when relevant):
- `web-vulnerability-taxonomy`: Coverage tracking
- `web-access-control-matrix`: Role authorization analysis
- `web-api-graphql`: GraphQL-specific testing

---

## Key Principles

### 1. Findings Are Ingredients

❌ **Linear thinking**: "Found IDOR → report it → move on"

✅ **Chain thinking**: "Found IDOR → what does this enable? → test write access → test object references → test privilege escalation"

### 2. Test Then Update

❌ **Stale list**:
```python
hypotheses = generate_once()
for h in hypotheses:
    test(h)  # Graph changes but list doesn't
```

✅ **Live regeneration**:
```python
while not stop_condition:
    hypotheses = generate_from_current_graph()
    test(hypotheses[0])  # Priority 1
    update_graph(result)
    # Loop regenerates with new graph state
```

### 3. Relationships Matter

**Flat list** (weak):
```
- /api/orders
- /api/users
- JWT auth
```

**Relational graph** (powerful):
```
/api/orders/{id}
  └─ response.user_id → /api/users/{user_id}
                         └─ response.payment_id → /api/payments/{id}
```

Graph reveals: **3-hop IDOR chain possible**

### 4. Business Logic Over Noise

❌ **Scanner approach**: Run 5000 nuclei templates, get 200 leads

✅ **Hypothesis approach**: Generate 12 targeted hypotheses based on business flow analysis, confirm 8 critical chains

### 5. Critical Self-Review

Before finalizing report, **Chain Critic asks**:
- "What would a senior pentester test next?"
- "What combinations haven't been tried?"
- "What makes each medium finding critical when chained?"

---

## When to Use

**Use hypothesis loop when**:
- Complex business logic (multi-role, multi-tenant)
- API-heavy application (GraphQL, REST, microservices)
- Chain detection critical (IDOR → mass assignment → privilege escalation)
- Budget allows iterative depth (not quick surface scan)

**Use linear pipeline when**:
- Simple application (CRUD, minimal roles)
- Compliance checkbox (OWASP Top 10 coverage required)
- Tight time budget (scan & report in hours)

---

## Comparison: Linear vs Hypothesis

| Aspect | Linear Pipeline | Hypothesis Loop |
|--------|----------------|-----------------|
| **Execution** | Fixed steps, run once | Iterative, adapts to discoveries |
| **Findings** | Independent reports | Chains emphasized |
| **Tools** | Full scans (nuclei, sqlmap) | Targeted tests (curl, custom) |
| **Coverage** | Breadth-first | Depth-first on high-value |
| **Report** | List of findings | Attack paths + chains |
| **Stop condition** | Script completes | No critical hypotheses remain |
| **Best for** | Compliance, quick scans | Critical chains, deep testing |

---

## Future Enhancements

1. **ML-based hypothesis ranking**: Learn from past engagement patterns
2. **Automated proof validation**: Re-test findings to eliminate false positives
3. **Visual attack graphs**: Generate Graphviz diagrams of chains
4. **Budget optimization**: Allocate turns based on hypothesis ROI
5. **Multi-app correlation**: Detect patterns across engagements

---

## References

- OWASP WSTG (Web Security Testing Guide)
- OWASP ASVS (Application Security Verification Standard)
- PTES (Penetration Testing Execution Standard)
- "The Web Application Hacker's Handbook" (Stuttard & Pinto)

---

**Status**: Architecture defined, initial skills created, recipe implemented

**Next steps**: Implement Python tool scripts, test on sample application, iterate based on findings
