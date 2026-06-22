# Recipe → Skill Migration Example

**Before**: Recipe with declarative workflow  
**After**: Skill-driven dynamic planning

---

## Example: Web Application Hypothesis Testing

### Before: Recipe-Based Approach

**File**: `recipes/web-app-hypothesis-loop/RECIPE.md` (311 lines)

```yaml
---
name: web-app-hypothesis-loop
title: Web App Hypothesis-Driven Testing Loop
description: Iterative attack hypothesis generation, testing, and graph enrichment
keywords: hypothesis, loop, chaining, iterative
---

Instructions:
[Long description of what the recipe does...]

Config:
  local_tools:
    - local__activate_skill
    - local__run_skill
    - local__exec_cli
    - local__get_investigation_memory
    - local__update_investigation_memory
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
        
        ### 2. Select Hypothesis to Test
        Pick **Priority 1** hypothesis from list.
        
        ### 3. Execute Test (Exploiteur)
        Activate `web-hypothesis-tester` with selected hypothesis.
        
        ### 4. Update Graph
        Enrich asset graph with test result.
        
        ### 5. Trigger Re-Generation
        **Critical**: After EVERY significant finding update, re-run Stratège.
        
        ## Stop Conditions (Exit Loop When):
        1. No high/critical hypotheses remain
        2. Budget limit reached
        3. Scope boundary hit
      
      stop_condition: "No high/critical priority hypotheses remain OR budget exhausted"

    - name: Chain Critic Review
      prompt: |
        Activate `web-chain-critic` for final adversarial review.
        
        Questions:
        1. What findings connect?
        2. What chains were missed?
        3. What business-critical paths untested?
        4. What escalates isolated findings?
        5. What would senior pentester test next?
      
      stop_condition: "Critical chains tested, severity escalations applied"

    - name: Final Report Generation
      prompt: |
        Generate comprehensive penetration testing report.
        
        Include:
        - Executive Summary
        - Asset Graph Visualization
        - Findings with CWE/OWASP mappings
        - Attack Chains
        - Business Impact
        - Coverage
        - Remediation
        - Retest Plan
      
      stop_condition: "Final report generated"
```

**User interaction**:
```
User: /recipe use web-app-hypothesis-loop
Agent: Loaded web-app-hypothesis-loop recipe
User: Start testing https://api.example.com
Agent: [Executes Phase 1, then Phase 2, then Phase 3, then Phase 4]
```

**Problems**:
1. **Rigid**: Must execute all 4 phases in order
2. **Verbose**: 311 lines for something that should be simple
3. **Inflexible**: Can't adapt if GraphQL discovered mid-test
4. **Config scattered**: Tool access, iterations, etc. in recipe
5. **User friction**: Must remember recipe name

---

### After: Skill-Driven Approach

**Files**:
- `skills/web-asset-mapper/SKILL.md` (already exists)
- `skills/web-hypothesis-chaining/SKILL.md` (already exists)
- `skills/web-hypothesis-tester/SKILL.md` (already exists)
- `skills/web-chain-critic/SKILL.md` (already exists)
- `skills/task-planner/SKILL.md` (meta-skill for orchestration)

**Total**: Just the existing skills (no new files needed!)

**User interaction**:
```
User: Run hypothesis-driven testing on https://api.example.com

Agent (activates task-planner skill internally):
  Analyzing request...
  - User wants: Hypothesis-driven testing
  - Target: api.example.com
  - Available skills: web-asset-mapper, web-hypothesis-chaining, ...
  
  Creating execution plan:
  
  ## Execution Plan
  
  - [ ] TODO-1: Map assets (web-asset-mapper)
  - [ ] TODO-2: Generate hypotheses (web-hypothesis-chaining)
  - [ ] TODO-3: Test hypothesis loop (web-hypothesis-tester)
  - [ ] TODO-4: Analyze chains (web-chain-critic)
  - [ ] TODO-5: Generate report
  
  Starting execution...
  
  ✅ TODO-1 complete: Mapped 47 endpoints, 8 relationships
  ✅ TODO-2 complete: Generated 12 hypotheses
  ⏳ TODO-3 iteration 1: Testing H001...
  ✅ H001 confirmed → F001 (critical IDOR)
  ⏳ TODO-3 iteration 2: Testing H002 (chains with F001)...
  ✅ H002 confirmed → F002 (write IDOR)
  ...
  ✅ TODO-4 complete: Found 3 critical chains
  ✅ TODO-5 complete: Report ready
  
  Done! Found 8 vulnerabilities → 3 critical chains.
```

**Benefits**:
1. **Natural**: User just describes what they want
2. **Concise**: No 311-line YAML file
3. **Flexible**: Can adapt mid-execution
4. **Simple**: Just activate skills, LLM figures out the rest

---

## Detailed Comparison

### Phase 1: Initial Mapping

#### Recipe Approach

```yaml
- name: Initial Mapping
  prompt: |
    Activate `web-asset-mapper` and build the initial asset graph.
    
    Discover:
    - Endpoints (paths, methods, parameters)
    - Roles (observed permissions, privilege boundaries)
    - Technologies (framework, database, auth method)
    - Relationships (object references, shared ID namespaces)
    
    Store graph in investigation_memory.asset_graph.
    
    Focus on relationships: what links to what, which parameters 
    appear across endpoints, which roles see different fields.
  
  local_tools:
    - local__activate_skill
    - local__run_skill
    - local__exec_cli
    - local__get_investigation_memory
    - local__update_investigation_memory
  
  stop_condition: "Asset graph initialized with endpoints, roles, and key relationships mapped"
```

**Issues**:
- Hardcoded tool list
- Manual stop condition checking
- Phase must complete before next starts

#### Skill-Driven Approach

```
LLM reasoning:
  "User wants hypothesis testing. First need to map the target.
   I'll use web-asset-mapper skill."

Creates TODO-1:
  - Title: "Map API assets"
  - Skill: web-asset-mapper
  - Description: "Build asset graph with relationships"
  
Executes:
  activate_skill("web-asset-mapper")
  result = run_skill("web-asset-mapper", target="https://api.example.com")
  
Evaluates:
  if result.asset_graph.endpoints.length > 0:
    mark_complete(TODO-1, result)
  else:
    mark_blocked(TODO-1, "No endpoints discovered")
```

**Benefits**:
- LLM decides when complete (no hardcoded condition)
- Tool access inherited from skill definition
- Can run async if next phase doesn't depend on it

---

### Phase 2: Hypothesis Loop

#### Recipe Approach

```yaml
- name: Hypothesis Loop
  prompt: |
    **This is the core iterative loop. Repeat until stop condition met.**
    
    ## Loop Iteration:
    
    ### 1. Generate Hypotheses (Stratège)
    Activate `web-hypothesis-chaining` and read current asset graph.
    
    ### 2. Select Hypothesis to Test
    Pick **Priority 1** hypothesis from list.
    
    ### 3. Execute Test (Exploiteur)
    Activate `web-hypothesis-tester` with selected hypothesis.
    
    ### 4. Update Graph
    Enrich asset graph with test result.
    
    ### 5. Trigger Re-Generation
    **Critical**: After EVERY significant finding update, re-run Stratège.
    
    ## Stop Conditions:
    1. No high/critical hypotheses remain
    2. Budget limit reached
    3. Scope boundary hit
  
  stop_condition: "No high/critical priority hypotheses remain OR budget exhausted"
```

**Issues**:
- Tries to encode loop logic in text prompt
- Stop condition is string-matched (brittle)
- No parallelization possible

#### Skill-Driven Approach

```
LLM reasoning:
  "Need to test hypotheses iteratively until no critical ones remain."

Creates TODO-3 (loop):
  - Title: "Test hypothesis loop"
  - Skill: web-hypothesis-tester
  - Type: iterative
  - Stop: "No priority 1-2 hypotheses left OR budget < 10%"

Executes loop:
  iteration = 1
  while True:
    hypotheses = activate_skill("web-hypothesis-chaining", graph=current_graph)
    
    critical_h = filter(h for h in hypotheses if h.priority <= 2)
    
    if not critical_h:
      log("No critical hypotheses remain")
      break
    
    if budget_remaining() < 0.1:
      log("Budget exhausted")
      break
    
    top_h = critical_h[0]
    result = activate_skill("web-hypothesis-tester", hypothesis=top_h)
    
    if result.status == "confirmed":
      findings.append(result.finding)
      update_graph(result.graph_updates)
      # Loop continues with updated graph
    
    iteration += 1
  
  mark_complete(TODO-3, iterations=iteration)
```

**Benefits**:
- Real loop logic (not text prompt trying to describe a loop)
- Dynamic stop conditions (LLM evaluates)
- Can spawn subagents for parallel hypothesis testing

---

### Phase 3: Chain Critic

#### Recipe Approach

```yaml
- name: Chain Critic Review
  prompt: |
    Activate `web-chain-critic` for final adversarial review.
    
    Questions:
    1. What findings connect?
    2. What chains were missed?
    3. What business-critical paths untested?
    4. What escalates isolated findings?
  
  stop_condition: "Critical chains tested, severity escalations applied"
```

**Issues**:
- Fixed timing (always after loop)
- Can't run earlier if findings warrant it

#### Skill-Driven Approach

```
LLM reasoning:
  "Loop found 8 findings. Time to check for chains."

Creates TODO-4:
  - Title: "Analyze chains"
  - Skill: web-chain-critic
  - Depends on: TODO-3 (needs findings)

Executes:
  chains = activate_skill("web-chain-critic", 
                         graph=asset_graph,
                         findings=findings)
  
  if chains.new_hypotheses:
    # Dynamic re-plan!
    create_todo("Test newly discovered chains", 
                skill="web-hypothesis-tester",
                priority="high")
  
  mark_complete(TODO-4)
```

**Benefits**:
- Can trigger mid-loop if findings accumulate
- Can create new todos based on critic findings
- Flexible timing

---

## Dynamic Adaptation Example

### Scenario: GraphQL Discovered Mid-Test

**Recipe approach** (rigid):
```
Phase 2: Hypothesis Loop
  Testing H001 (REST IDOR)... ✅
  Testing H002 (REST authz)... ✅
  Testing H003 (REST injection)... ✅
  [Discovers /graphql endpoint]
  Testing H004 (REST param pollution)... ✅
  
Phase 3: Chain Critic
  [GraphQL never tested because not in recipe workflow]
```

**Skill-driven approach** (adaptive):
```
TODO-3 iteration 1: Testing H001... ✅ Found F001
TODO-3 iteration 2: Testing H002... ✅ Found F002
TODO-3 iteration 3: Testing H003...

  [During H003 test, discovers /graphql endpoint]
  
  LLM re-planning:
    "GraphQL endpoint found. This is a new attack surface.
     I should test it before continuing REST tests."
  
  Creates TODO-6 (inserted):
    - Title: "GraphQL introspection and testing"
    - Skill: web-api-graphql
    - Priority: High (new attack surface)
    - Insert before: TODO-4 (final analysis)
  
  Spawns subagent for TODO-6 in parallel with TODO-3

TODO-6: ⏳ GraphQL introspection... ✅ Schema leaked!
TODO-6: ⏳ Testing GraphQL authz... ✅ Found F008 (critical)
TODO-3 iteration 4: Testing H004... ✅
...
TODO-4: Analyze chains (includes GraphQL findings)
```

**Result**: GraphQL findings included because system adapted dynamically.

---

## Parallelization Example

### Scenario: Independent Tests

**Recipe approach** (sequential):
```
Phase 2: Loop iteration 1 (5 min)
Phase 2: Loop iteration 2 (5 min)
Phase 2: Loop iteration 3 (5 min)
Total: 15 minutes
```

**Skill-driven approach** (parallel):
```
LLM reasoning:
  "H001, H002, H003 test different endpoints and don't share state.
   These can run in parallel."

Spawns 3 subagents:
  Subagent A: Test H001 (5 min)
  Subagent B: Test H002 (5 min) [parallel]
  Subagent C: Test H003 (5 min) [parallel]

Total: 5 minutes (3x faster!)
```

---

## Code Simplification

### Recipe Loader (DELETE THIS)

```rust
// recipes/loader.rs (can delete entire file)
pub struct RecipeLoader {
    pub fn load_recipe(path: &Path) -> Recipe { ... }
    pub fn parse_workflow(yaml: &str) -> Workflow { ... }
    pub fn parse_phases(phases: &[Phase]) -> Vec<Phase> { ... }
}

pub struct WorkflowExecutor {
    pub fn execute_phase(phase: &Phase) -> PhaseResult { ... }
    pub fn check_stop_condition(condition: &str) -> bool { ... }
    pub fn manage_local_tools(tools: &[Tool]) -> ... { ... }
}

// ~500 lines of code for recipe management
```

### Skill-Driven (JUST THIS)

```rust
// skills/loader.rs (already exists!)
pub fn activate_skill(name: &str) -> Skill { ... }

// NEW: Add todo tool (~100 lines)
pub fn create_todo(...) -> TodoId
pub fn mark_complete(todo_id: TodoId, result: Result)
pub fn list_todos() -> Vec<Todo>

// ~100 lines total (5x simpler)
```

---

## Migration Steps

1. ✅ Create `task-planner` skill
2. ✅ Document architecture shift
3. [ ] Implement todo tool in rusty-bidule
4. [ ] Update system prompt to activate task-planner
5. [ ] Test with example: "Run hypothesis testing on X"
6. [ ] Delete recipe loader code
7. [ ] Move recipes/ to recipes_deprecated/
8. [ ] Update all docs

---

## User Experience Comparison

### Recipe-Based

```
User: I want to test https://api.example.com for authorization issues
Agent: You need to use a recipe. Try /recipe list
User: /recipe list
Agent: Available recipes:
  - web-app-hypothesis-loop
  - web-app-input-validation
  - web-app-business-logic-race
  ...
User: /recipe use web-app-hypothesis-loop
Agent: Recipe loaded. What's the target?
User: https://api.example.com
Agent: Starting Phase 1: Initial Mapping...
[Rigid execution of 4 phases]
```

### Skill-Driven

```
User: Test https://api.example.com for authorization issues

Agent: 
  I'll run hypothesis-driven testing focused on authorization.
  
  Creating execution plan...
  
  ✅ TODO-1: Map assets
  ✅ TODO-2: Generate authz hypotheses (filtering for IDOR, role-based issues)
  ⏳ TODO-3: Test hypothesis loop...
  
  Found critical vertical IDOR on /api/orders!
  Chaining with mass assignment test...
  
  ✅ Complete: 5 authorization findings → 2 critical chains
```

**Difference**: Natural conversation vs. command-line interface

---

**The skill-driven approach is simpler, more flexible, and more powerful.**
