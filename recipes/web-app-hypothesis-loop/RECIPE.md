---
name: web-app-hypothesis-loop
title: Web App Hypothesis-Driven Testing Loop
description: Iterative attack hypothesis generation, testing, and graph enrichment - replaces linear pipeline with intelligence-driven exploration
keywords: hypothesis, loop, chaining, iterative, graph, strategy
---

Instructions:
This recipe implements hypothesis-driven penetration testing via a continuous loop:

**Mapper** → builds/enriches asset graph with relationships
**Stratège** → reads graph, generates prioritized attack hypotheses
**Exploiteur** → tests hypotheses, collects proof
**Graph Update** → enriches graph with findings
**Loop** → Stratège regenerates based on new graph state

The loop continues until:
- No high/critical priority hypotheses remain
- Budget exhausted (turns/time limit)
- Scope boundary reached

Before finalizing report, **Chain Critic** performs adversarial review to catch missed chains.

## Core Principles

1. **Findings are ingredients, not endpoints** - each result triggers new hypotheses
2. **Relationships matter** - graph structure enables chain discovery
3. **Test then update** - never continue with stale hypothesis list
4. **Business logic over scanner noise** - targeted tests beat enumeration
5. **Critical self-review** - pause before report to find missed chains

## Skills Used

- `web-asset-mapper`: Build relational asset graph
- `web-hypothesis-chaining`: Generate attack hypotheses from graph
- `web-hypothesis-tester`: Execute targeted tests with proof
- `web-chain-critic`: Final review for missed chains
- `web-vulnerability-taxonomy`: Map findings to OWASP/CWE
- `web-access-control-matrix`: Analyze role-based authorization
- `web-api-graphql`: GraphQL-specific testing when discovered

Config:
  local_tools:
    - local__activate_skill
    - local__run_skill
    - local__exec_cli
    - local__get_investigation_memory
    - local__update_investigation_memory
    - local__search_conversation_memories
  max_agent_iterations: 25
  continuation_increment: 10

Workflow:
  type: iterative_research
  phases:
    - name: Initial Mapping
      prompt: |
        Activate `web-asset-mapper` and build the initial asset graph.
        
        Discover:
        - Endpoints (paths, methods, parameters)
        - Roles (observed permissions, privilege boundaries)
        - Technologies (framework, database, auth method)
        - Relationships (object references, shared ID namespaces)
        
        Store graph in investigation_memory.asset_graph.
        
        Focus on relationships: what links to what, which parameters appear across endpoints, which roles see different fields.
      
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__exec_cli
        - local__get_investigation_memory
        - local__update_investigation_memory
      
      stop_condition: "Asset graph initialized with endpoints, roles, and key relationships mapped"

    - name: Hypothesis Loop
      prompt: |
        **This is the core iterative loop. Repeat until stop condition met.**
        
        ## Loop Iteration:
        
        ### 1. Generate Hypotheses (Stratège)
        Activate `web-hypothesis-chaining` and read current asset graph.
        
        Generate prioritized attack hypotheses based on:
        - Discovered relationships (object references, ID patterns)
        - Role permission gaps (what's tested vs untested)
        - Technology implications (framework CVEs, DB-specific attacks)
        - Previous findings (what chains are now enabled?)
        
        Activate `web-vulnerability-taxonomy` to ensure coverage.
        
        Output: Ranked list of hypotheses (Priority 1 = highest impact/likelihood)
        
        ### 2. Select Hypothesis to Test
        Pick **Priority 1** hypothesis from list.
        
        Check:
        - Is target in scope? (use investigation_memory.scope)
        - Are prerequisites met? (accounts, prior findings)
        - Is test type authorized? (destructive, OOB, rate limits)
        
        If blocked → mark as deferred, move to next hypothesis.
        
        ### 3. Execute Test (Exploiteur)
        Activate `web-hypothesis-tester` with selected hypothesis.
        
        Perform targeted test:
        - Follow hypothesis test approach
        - Collect structured proof (requests, responses, validation)
        - Return result: confirmed / blocked / unclear
        
        For confirmed findings:
        - Assign finding ID (F001, F002, etc.)
        - Classify severity (critical/high/medium/low)
        - Map to CWE/OWASP using taxonomy
        
        ### 4. Update Graph
        Enrich asset graph with test result:
        - Add finding to graph
        - Update endpoint/role observations
        - Add new relationships discovered
        - Flag uncertainties resolved
        
        Store updated graph in investigation_memory.
        
        ### 5. Trigger Re-Generation
        **Critical**: After EVERY significant finding update, re-run Stratège.
        
        New findings change the graph → new hypotheses become viable → priorities shift.
        
        Don't continue with old hypothesis list!
        
        ### 6. Activate Business Logic Skills When Triggered
        If hypotheses reference:
        - Role-based authorization → activate `web-access-control-matrix`
        - GraphQL endpoints → activate `web-api-graphql`
        - Race conditions → activate `web-business-logic-race`
        
        These skills run **when relevant findings appear**, not at fixed pipeline positions.
        
        ## Stop Conditions (Exit Loop When):
        
        1. **No high/critical hypotheses remain** (only low-priority left)
        2. **Budget limit reached** (max_agent_iterations approaching)
        3. **Scope boundary hit** (next hypothesis requires out-of-scope test)
        4. **Diminishing returns** (last 5 tests all blocked/negative)
        5. **User requests stop** (via /continue budget management)
        
        ## Loop Metrics to Track:
        - Hypotheses tested: X
        - Findings confirmed: Y
        - Chains discovered: Z
        - Coverage: OWASP Top 10 checklist
      
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__exec_cli
        - local__get_investigation_memory
        - local__update_investigation_memory
      
      stop_condition: "No high/critical priority hypotheses remain OR budget exhausted OR scope boundary reached"

    - name: Chain Critic Review
      prompt: |
        Activate `web-chain-critic` for final adversarial review.
        
        Questions to answer:
        1. **What findings connect?** (same type, same flow, shared weakness)
        2. **What chains were missed?** (combinations not tested)
        3. **What business-critical paths untested?** (payment, admin, export)
        4. **What escalates isolated findings?** (medium → critical when chained)
        5. **What would a senior pentester test next?**
        
        Output:
        - List of potential chains to re-test
        - Severity escalations recommended  
        - Coverage gaps (high-priority untested areas)
        - Attack path diagrams (anonymous → user → admin)
        
        If critic identifies critical gaps:
        - Generate new high-priority hypotheses
        - Return to Hypothesis Loop for targeted re-testing
        - Update findings with chained severity
        
        Don't proceed to final report until critic review is satisfied.
      
      local_tools:
        - local__activate_skill
        - local__run_skill
        - local__get_investigation_memory
        - local__update_investigation_memory
      
      stop_condition: "Critical chains tested, severity escalations applied, no high-priority gaps remain"

    - name: Final Report Generation
      prompt: |
        Generate comprehensive penetration testing report.
        
        Include:
        - **Executive Summary**: Critical findings, business impact, chained attack paths
        - **Asset Graph Visualization**: Endpoints, roles, relationships discovered
        - **Findings**: Structured list with CWE/OWASP mappings, proof, reproduction steps
        - **Attack Chains**: Diagrams showing how findings combine (e.g., IDOR + mass assignment = privilege escalation)
        - **Business Impact**: Compliance scope (GDPR, PCI), data volume accessible, financial risk
        - **Coverage**: OWASP Top 10 checklist, WSTG sections tested
        - **Remediation**: Prioritized by severity and business risk
        - **Retest Plan**: Critical fixes to validate
        
        **Chain emphasis**: Highlight combined impacts, not just isolated findings.
        
        Example:
        - ❌ "Finding F001: IDOR (medium severity)"
        - ✅ "Attack Chain: F001 (IDOR) + F005 (mass assignment) = Critical privilege escalation enabling full admin access"
        
        Store final report in investigation_memory.final_report.
      
      local_tools:
        - local__get_investigation_memory
        - local__update_investigation_memory
        - local__activate_skill
      
      stop_condition: "Final report generated with chains, impacts, and remediation priorities"

Initial Prompt:
Begin hypothesis-driven web application penetration testing. Start with asset mapping, then enter the hypothesis generation → test → update loop.

Response Template:
## {{ recipe_title }}

{{ response }}

---

## Example Loop Flow

**Iteration 1:**
```
Mapper: Discovered /api/orders/{id}, /api/users/{id}
        IDs are sequential (1000-9999)
        User and admin roles observed
        
Stratège: Hypothesis H001 (Priority 1)
          "User can access admin orders via /api/orders/{admin_id}"
          Rationale: Sequential IDs, no authz check observed
          
Exploiteur: Testing H001...
            Created order as admin → ID 5678
            Accessed as user → 200 OK with admin data
            ✅ CONFIRMED: F001 (Vertical IDOR)
            
Graph Update: /api/orders/{id}.authorization = "broken"
              findings.append(F001)
```

**Iteration 2:**
```
Stratège: Re-generated based on F001
          Hypothesis H002 (Priority 1)
          "F001 enables write access via PUT /api/orders/{admin_id}"
          Chain: IDOR read → test IDOR write
          
Exploiteur: Testing H002...
            PUT /api/orders/5678 {"status":"cancelled"}
            → 200 OK, order cancelled
            ✅ CONFIRMED: F002 (IDOR write access)
            
Graph Update: F001 + F002 = Critical chain
              Impact: User can modify any order
```

**Iteration 3:**
```
Stratège: Re-generated based on F001 + F002
          Hypothesis H003 (Priority 1)
          "Order modification enables refund fraud via /api/payments/refund"
          Chain: F002 (modify order) → trigger refund
          
Exploiteur: Testing H003...
            Modified victim order to "refund_requested"
            → Automatic refund triggered
            ✅ CONFIRMED: F003 (Refund fraud via IDOR chain)
            
Graph Update: F001 + F002 + F003 = CRITICAL CHAIN
              Impact: Financial fraud, arbitrary refunds
```

**Iteration 4:**
```
Stratège: Coverage check via taxonomy
          Gap: GraphQL endpoint discovered but not tested
          Hypothesis H004 (Priority 2)
          "GraphQL introspection might leak schema"
          
Exploiteur: Testing H004...
            Activated web-api-graphql skill
            → Introspection enabled, full schema leaked
            ✅ CONFIRMED: F004 (Info disclosure)
            
Graph Update: GraphQL mutations discovered
              New hypotheses: Test GraphQL-specific IDOR
```

**...Loop continues until stop condition...**

**Final Phase:**
```
Chain Critic: Reviewing 8 findings...
              
              ⚠️ Missed chain identified:
              F001 (IDOR) + F006 (mass assignment) not tested together
              
              Re-test: Can user set is_admin=true on victim account?
              → ✅ CONFIRMED: F009 (Privilege escalation)
              
              Severity escalation:
              F001: Medium → Critical (enables F009)
              
              Attack path: Anonymous → Register → IDOR → Mass Assignment → Admin
              
Final Report: 9 findings, 3 critical chains documented
              Total risk: Critical (full compromise possible)
```

---

## Key Differences from Linear Pipeline

| Linear Pipeline | Hypothesis Loop |
|----------------|-----------------|
| Recon → Scan → Report | Map → Hypothesize → Test → Update → Loop |
| Each step runs once | Stratège re-runs after each finding |
| Findings treated independently | Findings trigger new hypotheses |
| Tools run on full scope | Tests are targeted to hypothesis |
| No chain detection | Chain analyzer built-in |
| Fixed execution order | Priority-driven, adapts to discoveries |
| Stops when script ends | Stops when no critical hypotheses remain |

---

**This recipe transforms pentesting from checklist execution to intelligent exploitation.**
