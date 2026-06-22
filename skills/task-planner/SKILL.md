---
name: task-planner
description: Breaks down complex tasks into executable sub-tasks with skill activations, creates todos, and manages execution
metadata:
  keywords: planning, orchestration, todos, subagents, execution
---

# Task Planner

Meta-skill that transforms user requests into executable plans using skills and sub-tasks.

## Purpose

When a user asks for complex work (e.g., "conduct hypothesis-driven pentest on api.example.com"), this skill:

1. **Analyzes** the request and available skills
2. **Decomposes** into concrete sub-tasks
3. **Creates todos** for tracking
4. **Activates skills** as needed
5. **Spawns subagents** for parallel work
6. **Evaluates** results and marks todos complete/blocked

## Core Principle

**No declarative workflows**. The LLM reasons about:
- What skills are available
- What the user wants
- What sequence makes sense
- What can run in parallel

Plans are **dynamic**, not pre-scripted.

## Usage Pattern

User says:
```
"Run hypothesis-driven testing on https://api.example.com"
```

Planner:
1. Reads available skills (web-asset-mapper, web-hypothesis-chaining, etc.)
2. Reasons: "Need to map assets → generate hypotheses → test → analyze chains"
3. Creates todos:
   - [ ] TODO-1: Map assets (skill: web-asset-mapper)
   - [ ] TODO-2: Generate hypotheses (skill: web-hypothesis-chaining)
   - [ ] TODO-3: Test top hypothesis (skill: web-hypothesis-tester)
   - [ ] TODO-4: Analyze chains (skill: web-chain-critic)
4. Executes sequentially or in parallel as appropriate
5. Marks todos complete/blocked based on results

## Planning Heuristics

### Skill Activation

When user request mentions:
- "hypothesis testing", "pentest", "security audit" → Activate `web-hypothesis-chaining`
- "map endpoints", "discover assets" → Activate `web-asset-mapper`
- "business logic", "race conditions" → Activate `web-business-logic-race`
- "IDOR", "authorization" → Activate `web-access-control-matrix`

### Parallelization

Run in parallel when:
- Tasks are independent (different endpoints, different test types)
- Shared state not required
- Budget allows concurrent work

Run sequentially when:
- Task B depends on Task A output (hypothesis test depends on graph)
- Shared resource (investigation_memory) needs coordination
- Critical path (must complete before proceeding)

### Stop Conditions

Mark complete when:
- All todos done
- User-defined goal achieved
- Budget exhausted
- Blocking error encountered

## Todo Tool Integration

The planner uses the `todo` tool pattern:

```python
# Create todo
create_todo(
    title="Map API assets",
    description="Activate web-asset-mapper, build asset graph, store in investigation_memory",
    skill="web-asset-mapper",
    depends_on=[],  # No dependencies
    priority="high"
)

# Execute (spawn subagent or run directly)
result = execute_todo(todo_id="TODO-1")

# Evaluate
if result.success:
    mark_complete(todo_id="TODO-1", result=result)
else:
    mark_blocked(todo_id="TODO-1", reason=result.error)
```

## Example: Hypothesis Testing Plan

**User request**:
```
"Conduct hypothesis-driven security testing on https://api.example.com, 
focusing on authorization and business logic"
```

**Planner reasoning**:
```
1. Need asset mapping first (foundation)
2. Then hypothesis generation (strategy)
3. Then iterative testing (execution)
4. Finally chain analysis (synthesis)

Focus areas: authorization + business logic
→ Activate web-access-control-matrix after mapping
→ Activate web-business-logic-race for race conditions
```

**Generated todos**:

```markdown
## Execution Plan

### Phase 1: Discovery
- [ ] TODO-1: Map API assets
  - Skill: web-asset-mapper
  - Target: https://api.example.com
  - Output: asset_graph in investigation_memory
  
### Phase 2: Strategy
- [ ] TODO-2: Generate authorization hypotheses
  - Skill: web-hypothesis-chaining
  - Input: asset_graph
  - Filter: authorization-related hypotheses
  - Output: prioritized hypothesis list

- [ ] TODO-3: Analyze role-based access patterns
  - Skill: web-access-control-matrix
  - Input: asset_graph
  - Parallel with TODO-2: Yes (can run concurrently)
  
### Phase 3: Execution Loop
- [ ] TODO-4: Test highest-priority hypothesis
  - Skill: web-hypothesis-tester
  - Depends on: TODO-2
  - Loop: Re-run after each finding

- [ ] TODO-5: Test business logic race conditions
  - Skill: web-business-logic-race
  - Depends on: TODO-1 (needs endpoints list)
  - Parallel with TODO-4: Yes
  
### Phase 4: Synthesis
- [ ] TODO-6: Analyze chains and missed opportunities
  - Skill: web-chain-critic
  - Depends on: TODO-4, TODO-5 (needs findings)
  - Output: final report with chains
```

**Execution**:
1. Spawn subagent for TODO-1 (Mapper)
2. When TODO-1 complete, spawn 2 parallel subagents for TODO-2 + TODO-3
3. When TODO-2 complete, start TODO-4 loop
4. Meanwhile, TODO-5 runs in parallel
5. When both loops exhaust, spawn TODO-6
6. Mark plan complete

## Dynamic Re-Planning

If a todo fails or new information emerges:

**Example**: TODO-4 discovers GraphQL endpoint

```
Original plan: Standard REST API testing
New information: GraphQL detected

→ Dynamic re-plan:
  - [ ] TODO-7: GraphQL introspection
    - Skill: web-api-graphql
    - Priority: High (new attack surface)
    - Insert before: TODO-6 (final analysis)
```

Planner adds new todo, adjusts dependencies, continues execution.

## Integration with Investigation Memory

All todos update shared state:

```json
{
  "investigation_memory": {
    "asset_graph": {...},  // From TODO-1
    "hypotheses": [...],    // From TODO-2
    "findings": [...],      // From TODO-4, TODO-5
    "chains": [...],        // From TODO-6
    "plan": {
      "todos": [...],
      "completed": ["TODO-1", "TODO-2"],
      "in_progress": ["TODO-4"],
      "blocked": []
    }
  }
}
```

## Error Handling

When a todo fails:

```
TODO-4 failed: Scope check failed - target out of authorized scope

Planner actions:
1. Mark TODO-4 blocked
2. Check if other todos can proceed
3. If critical path blocked, escalate to user
4. If non-critical, continue with other todos
5. Generate alternative plan if possible
```

## Tools

This is a meta-skill (no dedicated tools). It orchestrates:

- `local__activate_skill` - Load skill instructions
- Subagent spawning (via rusty-bidule Agent tool or equivalent)
- Todo creation/tracking (via rusty-bidule todo system)
- `local__get_investigation_memory` / `local__update_investigation_memory`

## Success Criteria

Plan is successful when:
1. All high-priority todos completed OR
2. User goal achieved (e.g., "found critical chains") OR
3. User explicitly satisfied with results

Plan is blocked when:
4. Critical todo blocked with no alternative
5. Scope violation prevents progress
6. Budget exhausted before minimum viable result

## Example Invocation

**User**:
```
Use hypothesis-driven testing to find authorization issues in the API
```

**Assistant (activates task-planner)**:
```
I'll create an execution plan for hypothesis-driven authorization testing:

[Creates todos as shown above]

Starting execution...

✅ TODO-1 complete: Mapped 47 endpoints, 3 roles, 8 relationships
✅ TODO-2 complete: Generated 12 hypotheses, top 3 are IDOR-related
⏳ TODO-4 in progress: Testing H001 (vertical IDOR on /api/orders)
✅ TODO-4 complete: Confirmed F001 (critical IDOR)
⏳ TODO-4 iteration 2: Testing H002 (chain with F001)
...
```

## Anti-Patterns

❌ **Don't pre-script everything**:
```
Always do: Step 1 → Step 2 → Step 3
```

✅ **Reason dynamically**:
```
User wants X. Skills available: A, B, C. 
Best approach: Use A first, then based on A's output, decide B or C.
```

❌ **Don't ignore failures**:
```
TODO-1 failed? Just continue anyway.
```

✅ **Adapt to failures**:
```
TODO-1 failed (network error). Alternative: Use cached data? Ask user? Try different approach?
```

---

**This skill enables flexible, LLM-driven orchestration without rigid workflow definitions.**
