# Skill-Driven Architecture

**Architecture shift**: From declarative recipes to LLM-driven planning with skills

**Date**: 2026-06-22

---

## The Shift

### Before: Recipe-Based

```yaml
# recipes/web-app-hypothesis-loop/RECIPE.md
Workflow:
  type: iterative_research
  phases:
    - name: Initial Mapping
      prompt: "Activate web-asset-mapper..."
      stop_condition: "Asset graph initialized"
    - name: Hypothesis Loop
      prompt: "Generate hypotheses..."
      stop_condition: "No critical hypotheses remain"
```

**Problems**:
- Rigid structure (must follow phases)
- Declarative workflow (YAML-defined)
- Hard to adapt mid-execution
- Recipe-specific config scattered

### After: Skill-Driven

```
User: "Run hypothesis-driven testing on api.example.com"

LLM (using task-planner skill):
1. Analyzes request + available skills
2. Creates dynamic plan with todos
3. Spawns subagents as needed
4. Adapts based on results
5. Marks todos complete/blocked
```

**Benefits**:
- Flexible reasoning (LLM decides)
- Dynamic adaptation (re-plan on new info)
- Simpler config (just skills)
- Natural language interface

---

## Architecture Components

### 1. Skills (Only)

**Definition**: Self-contained capability with:
- Clear purpose
- Usage instructions
- Tool definitions (optional)
- Example invocations

**Location**: `skills/*/SKILL.md`

**Format** (agentskills.io compliant):
```markdown
---
name: skill-name
description: One-line description
metadata:
  keywords: tag1, tag2, tag3
---

# Skill Name

[Instructions for LLM]

## Tools (optional)

Tools:
  - name: Tool Name
    slug: tool-slug
    description: What it does
    script: scripts/tool.py
```

**No workflows, no phases, no stop conditions** - just instructions.

### 2. Task Planner (Meta-Skill)

**Purpose**: Orchestrates other skills

**Location**: `skills/task-planner/SKILL.md`

**Capabilities**:
- Breaks down user requests
- Creates todos
- Activates skills
- Spawns subagents
- Evaluates results
- Re-plans dynamically

**Key insight**: The planner IS a skill, so it can be:
- Activated like any skill
- Improved via prompt engineering
- Swapped for alternative planning approaches

### 3. Todos (Built-in Tool)

**Purpose**: Track execution state

**Operations**:
- `create_todo(title, description, skill, depends_on, priority)`
- `mark_complete(todo_id, result)`
- `mark_blocked(todo_id, reason)`
- `list_todos(status="all|pending|complete|blocked")`

**State**: Persisted in `investigation_memory.plan.todos`

### 4. Investigation Memory (Shared State)

**Purpose**: Single source of truth

**Structure**:
```json
{
  "asset_graph": {...},      // From web-asset-mapper
  "hypotheses": [...],        // From web-hypothesis-chaining
  "findings": [...],          // From web-hypothesis-tester
  "chains": [...],            // From web-chain-critic
  "plan": {
    "todos": [...],
    "completed": [],
    "in_progress": [],
    "blocked": []
  },
  "scope": {...},
  "metadata": {...}
}
```

All skills read/write to this shared memory.

---

## Execution Flow

### Traditional Recipe Flow

```
┌──────────────────────────────────────┐
│ Recipe Loader                        │
│ - Parse YAML                         │
│ - Load workflow definition           │
└────────────┬─────────────────────────┘
             ▼
┌──────────────────────────────────────┐
│ Phase 1: Initial Mapping             │
│ - Execute prompt                     │
│ - Wait for stop_condition            │
└────────────┬─────────────────────────┘
             ▼
┌──────────────────────────────────────┐
│ Phase 2: Hypothesis Loop             │
│ - Execute prompt                     │
│ - Wait for stop_condition            │
└────────────┬─────────────────────────┘
             ▼
┌──────────────────────────────────────┐
│ Phase 3: Chain Critic                │
│ - Execute prompt                     │
│ - Wait for stop_condition            │
└──────────────────────────────────────┘
```

**Fixed sequence, no adaptation**

### New Skill-Driven Flow

```
┌──────────────────────────────────────┐
│ User Request                         │
│ "Run hypothesis testing on X"        │
└────────────┬─────────────────────────┘
             ▼
┌──────────────────────────────────────┐
│ LLM Reasoning (with task-planner)    │
│ - What skills are available?         │
│ - What's the best approach?          │
│ - What order makes sense?            │
│ - What can run in parallel?          │
└────────────┬─────────────────────────┘
             ▼
┌──────────────────────────────────────┐
│ Create Todos                         │
│ TODO-1: Map assets (web-asset-mapper)│
│ TODO-2: Generate hypotheses          │
│ TODO-3: Test hypothesis (loop)       │
│ TODO-4: Analyze chains               │
└────────────┬─────────────────────────┘
             ▼
┌──────────────────────────────────────┐
│ Execute Todos                        │
│ - Spawn subagents as needed          │
│ - Track progress                     │
│ - Update investigation_memory        │
└────────────┬─────────────────────────┘
             ▼
┌──────────────────────────────────────┐
│ Dynamic Re-Planning                  │
│ - New info? Create new todos         │
│ - Failure? Adapt approach            │
│ - Success? Mark complete             │
└────────────┬─────────────────────────┘
             ▼
┌──────────────────────────────────────┐
│ Completion                           │
│ - All todos done OR                  │
│ - Goal achieved OR                   │
│ - User satisfied                     │
└──────────────────────────────────────┘
```

**Dynamic, adaptive, LLM-reasoned**

---

## Migration Guide

### Converting a Recipe to Skills

**Example**: `recipes/web-app-hypothesis-loop/RECIPE.md`

#### Step 1: Extract Core Logic into Skills

Recipe had:
- Phase 1: Initial Mapping
- Phase 2: Hypothesis Loop
- Phase 3: Chain Critic

These already have skills:
- ✅ `web-asset-mapper`
- ✅ `web-hypothesis-chaining`
- ✅ `web-hypothesis-tester`
- ✅ `web-chain-critic`

**No conversion needed** - skills already exist!

#### Step 2: Remove Recipe Config

Recipe config:
```yaml
Config:
  local_tools: [...]
  max_agent_iterations: 25
```

**New approach**: Config is global or skill-specific, not recipe-specific.

Tool access controlled by:
- `skills/*/SKILL.md` → `Tools:` section
- Global rusty-bidule config

#### Step 3: Delete Recipe File

```bash
rm recipes/web-app-hypothesis-loop/RECIPE.md
```

#### Step 4: User Interaction Changes

**Before**:
```
User: /recipe use web-app-hypothesis-loop
User: Start the pentest
```

**After**:
```
User: Run hypothesis-driven testing on https://api.example.com
```

LLM with `task-planner` reasons:
1. "User wants hypothesis testing"
2. Look at available skills: web-asset-mapper, web-hypothesis-chaining, etc.
3. Create execution plan
4. Execute

---

## Example: Hypothesis Testing Request

### User Input

```
Conduct hypothesis-driven penetration testing on https://api.example.com.
Focus on authorization issues. I have 2 hours.
```

### LLM Reasoning (via task-planner)

```
Analyzing request:
- Target: api.example.com
- Focus: Authorization
- Time: 2 hours
- Implied approach: Hypothesis-driven (iterative)

Available skills:
- web-asset-mapper: Maps endpoints, roles, relationships
- web-hypothesis-chaining: Generates attack hypotheses
- web-hypothesis-tester: Tests hypotheses
- web-access-control-matrix: Analyzes role-based access
- web-chain-critic: Finds missed chains

Execution plan:
1. Map assets (foundation)
2. Focus on authz-related hypotheses (user's request)
3. Test iteratively
4. Analyze chains
5. Respect 2-hour budget

Creating todos...
```

### Generated Plan

```markdown
## Execution Plan: Hypothesis-Driven Authorization Testing

**Target**: https://api.example.com  
**Focus**: Authorization issues  
**Budget**: 2 hours (~15 iterations)

### Todos

#### Discovery Phase
- [ ] TODO-1: Map API assets and relationships
  - Skill: web-asset-mapper
  - Priority: Critical (foundation for everything)
  - Estimated: 5 minutes

#### Strategy Phase
- [ ] TODO-2: Generate authorization-focused hypotheses
  - Skill: web-hypothesis-chaining
  - Depends on: TODO-1
  - Filter: IDOR, cross-tenant, role-based issues
  - Estimated: 3 minutes

- [ ] TODO-3: Analyze role-based access patterns
  - Skill: web-access-control-matrix
  - Depends on: TODO-1
  - Parallel: Can run with TODO-2
  - Estimated: 5 minutes

#### Execution Loop (Iterative)
- [ ] TODO-4: Test highest-priority hypothesis
  - Skill: web-hypothesis-tester
  - Depends on: TODO-2
  - Loop until: No critical hypotheses OR budget exhausted
  - Estimated: 1.5 hours

#### Synthesis
- [ ] TODO-5: Analyze chains and escalate findings
  - Skill: web-chain-critic
  - Depends on: TODO-4
  - Estimated: 10 minutes

---

Starting execution...
```

### Execution

```
[09:00] ✅ TODO-1 complete
  - Discovered: 47 endpoints, 3 roles (user, admin, guest)
  - Key relationships: orders → users, payments → orders
  - Technologies: Express.js, MongoDB, JWT

[09:05] ✅ TODO-2 complete
  - Generated 12 hypotheses
  - Top 3: All IDOR-related (as requested: authorization focus)
  - H001: Vertical IDOR on /api/orders (priority 1)

[09:05] ✅ TODO-3 complete (ran parallel with TODO-2)
  - Role matrix: Admin sees 'cost' field, user doesn't
  - No cross-tenant testing observed
  - Recommendation: Test cross-tenant isolation

[09:08] ⏳ TODO-4 iteration 1: Testing H001
[09:12] ✅ H001 confirmed → F001 (critical vertical IDOR)
  - User can access admin orders
  - Re-generating hypotheses with F001...

[09:15] ⏳ TODO-4 iteration 2: Testing H002 (chain with F001)
[09:18] ✅ H002 confirmed → F002 (IDOR write access)
  - User can modify any order
  - Re-generating...

[09:20] ⏳ TODO-4 iteration 3: Testing H003 (cross-tenant via F001)
[09:24] ✅ H003 confirmed → F003 (cross-tenant isolation bypass)

... [continues iteratively] ...

[10:30] ✅ TODO-4 complete (8 findings, budget 75% used)
  - Stop reason: No critical hypotheses remain
  - Findings: 8 confirmed (5 critical, 3 high)

[10:35] ⏳ TODO-5: Analyzing chains
[10:40] ✅ TODO-5 complete
  - Identified 3 critical chains
  - Chain 1: F001+F002+F003 → Complete tenant takeover
  - Severity escalations: 2 findings → critical when chained

---

✅ Plan complete (1h 40m used, 20m under budget)

Final report: 8 findings → 3 critical chains
```

### Output to User

```
✅ Hypothesis-driven testing complete!

Found 8 authorization vulnerabilities:
- 5 critical, 3 high severity

Discovered 3 attack chains:
1. **Critical**: Vertical IDOR + write access + cross-tenant = full takeover
2. **Critical**: Mass assignment + IDOR = privilege escalation
3. **High**: Sequential IDs + no rate limiting = mass enumeration

Top recommendation: Fix systemic authorization failure (root cause of 5 findings)

Full details in investigation_memory. Generate report?
```

---

## Benefits Over Recipes

### 1. Flexibility

**Recipe**: Must follow phases in order
**Skills**: LLM decides best approach

Example:
```
User: "Quick scan for SQLi only"

Recipe approach:
- Still runs full hypothesis loop (IDOR, XSS, everything)
- Wastes time on unwanted tests

Skill approach:
- LLM reasons: "User wants SQLi only"
- Creates plan: Map → Generate SQLi hypotheses → Test SQLi
- Skips irrelevant work
```

### 2. Natural Language Interface

**Recipe**: `/recipe use web-app-hypothesis-loop`
**Skills**: "Run hypothesis testing" (LLM understands intent)

### 3. Dynamic Adaptation

**Recipe**: Fixed workflow, can't change mid-execution
**Skills**: Can re-plan based on discoveries

Example:
```
Mid-execution: GraphQL endpoint discovered

Recipe: Not in workflow, ignored
Skills: LLM adds TODO: "Introspect GraphQL schema"
```

### 4. Parallelization

**Recipe**: Sequential phases
**Skills**: LLM identifies parallel-safe tasks

Example:
```
TODO-2: Generate hypotheses (fast)
TODO-3: Access control analysis (slow)

LLM: "These don't conflict, run in parallel"
→ Saves time
```

### 5. Simpler Codebase

**Recipe**:
- YAML parser
- Workflow engine
- Phase executor
- Stop condition checker

**Skills**:
- Just activate skill
- LLM does the rest

---

## Implementation in Rusty Bidule

### Remove Recipe Support

```rust
// OLD: Remove these
// recipes/
// recipe_loader.rs
// workflow_executor.rs
```

### Keep Skills Support

```rust
// KEEP: Already implemented
// skills/
// skill_loader.rs
// local__activate_skill
// local__run_skill
```

### Add Todo Tool

```rust
// NEW: Add todo management
pub fn create_todo(...) -> TodoId
pub fn mark_complete(todo_id: TodoId, result: Result)
pub fn mark_blocked(todo_id: TodoId, reason: String)
pub fn list_todos(status: TodoStatus) -> Vec<Todo>
```

Todos stored in `investigation_memory.plan.todos`

### Modify LLM Prompt

Add to system prompt:
```
When user requests complex work:
1. Activate the task-planner skill
2. Break down into todos
3. Execute todos using skills
4. Track progress
5. Adapt as needed

Available skills: [list from skills/]
```

---

## Migration Checklist

- [ ] Create `task-planner` skill ✅
- [ ] Add todo tool to rusty-bidule
- [ ] Update system prompt to use planning
- [ ] Convert 1-2 recipes to examples (show Before/After)
- [ ] Test with real user request
- [ ] Delete recipe loader code
- [ ] Delete recipe files
- [ ] Update documentation

---

## FAQ

**Q: What if I want a predefined workflow?**
A: Create a skill that describes the workflow. LLM follows it.

Example: `skills/standard-pentest-workflow/SKILL.md` with instructions:
"Always do: Recon → Vuln scan → Exploit → Report"

**Q: How do I control tool access?**
A: In skill's `Tools:` section or global config. Same as before.

**Q: What about max_agent_iterations?**
A: Global config setting, not per-recipe.

**Q: Can I still have structured phases?**
A: Yes, via skill instructions. Just not YAML-enforced.

---

**This architecture is more flexible, more powerful, and simpler to maintain.**
