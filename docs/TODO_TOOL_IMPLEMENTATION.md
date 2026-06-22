# Todo Tool Implementation Guide

**For**: Rusty-bidule implementation  
**Purpose**: Enable skill-driven planning with task tracking

---

## Overview

The todo tool enables LLM-driven dynamic planning by:
1. Persisting execution plans across turns
2. Tracking progress (pending/in_progress/complete/blocked)
3. Supporting dependencies and priorities
4. Enabling dynamic re-planning

---

## Data Model

### Todo Structure

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: String,              // "TODO-1", "TODO-2", etc.
    pub title: String,           // "Map API assets"
    pub description: String,     // Detailed description
    pub skill: Option<String>,   // Skill to activate (e.g., "web-asset-mapper")
    pub depends_on: Vec<String>, // Dependencies: ["TODO-1"]
    pub priority: Priority,      // Critical, High, Medium, Low
    pub status: TodoStatus,      // Pending, InProgress, Complete, Blocked
    pub result: Option<Value>,   // Result when completed
    pub error: Option<String>,   // Error message when blocked
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Priority {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Complete,
    Blocked,
}
```

### Storage Location

Store in `investigation_memory` under `plan` key:

```json
{
  "investigation_memory": {
    "asset_graph": {...},
    "hypotheses": [...],
    "findings": [...],
    "plan": {
      "todos": [
        {
          "id": "TODO-1",
          "title": "Map API assets",
          "description": "Activate web-asset-mapper, build graph",
          "skill": "web-asset-mapper",
          "depends_on": [],
          "priority": "High",
          "status": "Complete",
          "result": {"endpoints": 47, "roles": 3},
          "created_at": "2026-06-22T10:00:00Z",
          "updated_at": "2026-06-22T10:05:00Z"
        },
        {
          "id": "TODO-2",
          "title": "Generate hypotheses",
          "description": "Use web-hypothesis-chaining with asset graph",
          "skill": "web-hypothesis-chaining",
          "depends_on": ["TODO-1"],
          "priority": "High",
          "status": "InProgress",
          "created_at": "2026-06-22T10:05:00Z",
          "updated_at": "2026-06-22T10:05:30Z"
        }
      ],
      "metadata": {
        "total_todos": 5,
        "completed": 1,
        "in_progress": 1,
        "blocked": 0,
        "pending": 3
      }
    }
  }
}
```

---

## Tool Operations

### 1. create_todo

**Purpose**: Create a new todo

**Input**:
```json
{
  "title": "Map API assets",
  "description": "Activate web-asset-mapper and build asset graph with relationships",
  "skill": "web-asset-mapper",
  "depends_on": [],
  "priority": "high"
}
```

**Output**:
```json
{
  "todo_id": "TODO-1",
  "status": "pending"
}
```

**Implementation**:
```rust
pub fn create_todo(
    title: String,
    description: String,
    skill: Option<String>,
    depends_on: Vec<String>,
    priority: Priority,
) -> Result<TodoId> {
    // 1. Load investigation_memory
    let mut memory = get_investigation_memory()?;
    
    // 2. Generate ID
    let existing_todos = memory["plan"]["todos"].as_array().unwrap_or(&vec![]);
    let next_id = format!("TODO-{}", existing_todos.len() + 1);
    
    // 3. Create todo
    let todo = Todo {
        id: next_id.clone(),
        title,
        description,
        skill,
        depends_on,
        priority,
        status: TodoStatus::Pending,
        result: None,
        error: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    // 4. Append to todos
    let mut todos = existing_todos.clone();
    todos.push(serde_json::to_value(todo)?);
    memory["plan"]["todos"] = Value::Array(todos);
    
    // 5. Update metadata
    update_plan_metadata(&mut memory);
    
    // 6. Save
    update_investigation_memory(memory)?;
    
    Ok(next_id)
}
```

### 2. mark_complete

**Purpose**: Mark todo as complete with result

**Input**:
```json
{
  "todo_id": "TODO-1",
  "result": {
    "endpoints": 47,
    "roles": 3,
    "relationships": 8
  }
}
```

**Output**:
```json
{
  "success": true,
  "updated_todo": {...}
}
```

**Implementation**:
```rust
pub fn mark_complete(todo_id: &str, result: Value) -> Result<Todo> {
    // 1. Load memory
    let mut memory = get_investigation_memory()?;
    
    // 2. Find todo
    let todos = memory["plan"]["todos"].as_array_mut()
        .ok_or_else(|| Error::NotFound)?;
    
    let todo = todos.iter_mut()
        .find(|t| t["id"] == todo_id)
        .ok_or_else(|| Error::TodoNotFound(todo_id.to_string()))?;
    
    // 3. Update status
    todo["status"] = Value::String("Complete".to_string());
    todo["result"] = result;
    todo["updated_at"] = Value::String(Utc::now().to_rfc3339());
    
    // 4. Update metadata
    update_plan_metadata(&mut memory);
    
    // 5. Save
    update_investigation_memory(memory)?;
    
    // 6. Return updated todo
    Ok(serde_json::from_value(todo.clone())?)
}
```

### 3. mark_blocked

**Purpose**: Mark todo as blocked with reason

**Input**:
```json
{
  "todo_id": "TODO-3",
  "reason": "Scope check failed - target out of authorized scope"
}
```

**Output**:
```json
{
  "success": true,
  "updated_todo": {...}
}
```

**Implementation**:
```rust
pub fn mark_blocked(todo_id: &str, reason: String) -> Result<Todo> {
    // Similar to mark_complete, but set status=Blocked and error=reason
    let mut memory = get_investigation_memory()?;
    
    let todos = memory["plan"]["todos"].as_array_mut()
        .ok_or_else(|| Error::NotFound)?;
    
    let todo = todos.iter_mut()
        .find(|t| t["id"] == todo_id)
        .ok_or_else(|| Error::TodoNotFound(todo_id.to_string()))?;
    
    todo["status"] = Value::String("Blocked".to_string());
    todo["error"] = Value::String(reason);
    todo["updated_at"] = Value::String(Utc::now().to_rfc3339());
    
    update_plan_metadata(&mut memory);
    update_investigation_memory(memory)?;
    
    Ok(serde_json::from_value(todo.clone())?)
}
```

### 4. list_todos

**Purpose**: List todos with optional filtering

**Input**:
```json
{
  "status": "pending"  // Optional: "all", "pending", "in_progress", "complete", "blocked"
}
```

**Output**:
```json
{
  "todos": [
    {
      "id": "TODO-2",
      "title": "Generate hypotheses",
      "status": "Pending",
      ...
    },
    {
      "id": "TODO-3",
      "title": "Test hypothesis loop",
      "status": "Pending",
      ...
    }
  ],
  "count": 2
}
```

**Implementation**:
```rust
pub fn list_todos(status: Option<TodoStatus>) -> Result<Vec<Todo>> {
    // 1. Load memory
    let memory = get_investigation_memory()?;
    
    // 2. Get todos
    let todos: Vec<Todo> = memory["plan"]["todos"]
        .as_array()
        .ok_or_else(|| Error::NotFound)?
        .iter()
        .map(|t| serde_json::from_value(t.clone()))
        .collect::<Result<Vec<Todo>, _>>()?;
    
    // 3. Filter by status if provided
    let filtered = if let Some(status) = status {
        todos.into_iter()
            .filter(|t| t.status == status)
            .collect()
    } else {
        todos
    };
    
    Ok(filtered)
}
```

### 5. update_todo_status (helper)

**Purpose**: Update in-progress status

**Input**:
```json
{
  "todo_id": "TODO-2",
  "status": "in_progress"
}
```

**Usage**: When starting work on a todo

---

## Integration with Skills

### Skill Activation Pattern

When a todo has a `skill` field:

```rust
// In LLM conversation flow
fn execute_todo(todo: &Todo) -> Result<Value> {
    // 1. Mark in progress
    update_todo_status(&todo.id, TodoStatus::InProgress)?;
    
    // 2. Activate skill
    if let Some(skill_name) = &todo.skill {
        activate_skill(skill_name)?;
    }
    
    // 3. Execute (LLM follows skill instructions)
    let result = llm_execute_with_skill_context(todo)?;
    
    // 4. Mark complete or blocked
    match result {
        Ok(r) => mark_complete(&todo.id, r)?,
        Err(e) => mark_blocked(&todo.id, e.to_string())?,
    }
    
    Ok(result?)
}
```

### Dependency Resolution

```rust
fn get_ready_todos() -> Result<Vec<Todo>> {
    let todos = list_todos(Some(TodoStatus::Pending))?;
    
    let ready = todos.into_iter()
        .filter(|todo| {
            // All dependencies must be complete
            todo.depends_on.iter().all(|dep_id| {
                is_todo_complete(dep_id).unwrap_or(false)
            })
        })
        .collect();
    
    Ok(ready)
}

fn is_todo_complete(todo_id: &str) -> Result<bool> {
    let todos = list_todos(None)?;
    Ok(todos.iter()
        .find(|t| t.id == todo_id)
        .map(|t| t.status == TodoStatus::Complete)
        .unwrap_or(false))
}
```

---

## LLM Interaction Pattern

### Creating a Plan

```
User: "Run hypothesis testing on https://api.example.com"

LLM: 
  Activating task-planner skill...
  
  Creating execution plan:
  
  [create_todo]
  {
    "title": "Map API assets",
    "description": "Use web-asset-mapper to discover endpoints, roles, relationships",
    "skill": "web-asset-mapper",
    "depends_on": [],
    "priority": "high"
  }
  → TODO-1 created
  
  [create_todo]
  {
    "title": "Generate hypotheses",
    "description": "Use web-hypothesis-chaining with asset graph",
    "skill": "web-hypothesis-chaining",
    "depends_on": ["TODO-1"],
    "priority": "high"
  }
  → TODO-2 created
  
  [create_todo]
  {
    "title": "Test hypothesis loop",
    "description": "Iteratively test hypotheses with web-hypothesis-tester",
    "skill": "web-hypothesis-tester",
    "depends_on": ["TODO-2"],
    "priority": "high"
  }
  → TODO-3 created
  
  Plan created. Starting execution...
```

### Executing Todos

```
LLM:
  [list_todos status="pending"]
  → Returns: TODO-1, TODO-2, TODO-3
  
  Checking dependencies...
  TODO-1 has no dependencies, can start.
  
  [update_todo_status id="TODO-1" status="in_progress"]
  
  Activating web-asset-mapper skill...
  [Executes skill instructions]
  
  Asset mapping complete:
  - 47 endpoints discovered
  - 3 roles identified
  - 8 relationships extracted
  
  [mark_complete id="TODO-1" result={"endpoints": 47, "roles": 3, "relationships": 8}]
  ✅ TODO-1 complete
  
  [list_todos status="pending"]
  → Returns: TODO-2, TODO-3
  
  Checking TODO-2 dependencies...
  TODO-1 is complete, can proceed.
  
  [update_todo_status id="TODO-2" status="in_progress"]
  Activating web-hypothesis-chaining skill...
  ...
```

### Handling Failures

```
LLM:
  Testing TODO-3...
  
  [update_todo_status id="TODO-3" status="in_progress"]
  Activating web-hypothesis-tester...
  
  Scope check: https://api.example.com
  Error: Target is out of authorized scope!
  
  [mark_blocked id="TODO-3" reason="Scope check failed - target not in authorized scope"]
  ❌ TODO-3 blocked
  
  Checking if plan can continue...
  TODO-4 depends on TODO-3 → Also blocked
  
  Critical path blocked. Escalating to user:
  "Cannot proceed - target out of scope. Please verify authorization."
```

### Dynamic Re-Planning

```
LLM:
  Executing TODO-3 (hypothesis testing)...
  
  Testing H001... ✅ Confirmed F001
  
  [Discovers /graphql endpoint during test]
  
  New information: GraphQL endpoint discovered!
  This is a new attack surface worth testing.
  
  Re-planning:
  
  [create_todo]
  {
    "title": "GraphQL introspection and testing",
    "description": "Use web-api-graphql to introspect schema and test authz",
    "skill": "web-api-graphql",
    "depends_on": ["TODO-1"],  // Needs asset graph, not TODO-3
    "priority": "high"
  }
  → TODO-6 created (inserted into plan)
  
  TODO-6 can run in parallel with TODO-3!
  
  Spawning subagent for TODO-6...
```

---

## System Prompt Integration

Add to rusty-bidule system prompt:

```markdown
## Task Planning and Execution

When user requests complex work, use the task-planner skill to:

1. **Create todos** for discrete sub-tasks
   - Use create_todo tool
   - Set skill, dependencies, priority
   - Break down into manageable chunks

2. **Execute todos** in dependency order
   - Use list_todos to check what's ready
   - Activate skill for each todo
   - Mark complete or blocked

3. **Track progress** across turns
   - Todos persist in investigation_memory
   - Check status with list_todos
   - Resume where left off

4. **Adapt dynamically**
   - Create new todos based on discoveries
   - Re-prioritize if needed
   - Handle failures gracefully

Example:
User: "Run hypothesis testing on X"
You: 
  1. Activate task-planner
  2. Create todos (map → hypotheses → test → analyze)
  3. Execute in order
  4. Adapt based on results

Available tools:
- create_todo(title, description, skill, depends_on, priority)
- mark_complete(todo_id, result)
- mark_blocked(todo_id, reason)
- list_todos(status)
- update_todo_status(todo_id, status)
```

---

## Testing

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_todo() {
        let id = create_todo(
            "Test task".to_string(),
            "Description".to_string(),
            Some("web-asset-mapper".to_string()),
            vec![],
            Priority::High,
        ).unwrap();
        
        assert_eq!(id, "TODO-1");
    }

    #[test]
    fn test_mark_complete() {
        let id = create_todo(...).unwrap();
        let result = json!({"success": true});
        
        mark_complete(&id, result).unwrap();
        
        let todo = get_todo(&id).unwrap();
        assert_eq!(todo.status, TodoStatus::Complete);
    }

    #[test]
    fn test_dependencies() {
        let id1 = create_todo(..., vec![], ...).unwrap();
        let id2 = create_todo(..., vec![id1.clone()], ...).unwrap();
        
        let ready = get_ready_todos().unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, id1);
        
        mark_complete(&id1, json!({})).unwrap();
        
        let ready = get_ready_todos().unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, id2);
    }
}
```

### Integration Test

```rust
#[test]
fn test_hypothesis_testing_plan() {
    // Simulate user request
    let request = "Run hypothesis testing on https://api.example.com";
    
    // LLM creates todos
    let t1 = create_todo("Map assets", ..., Some("web-asset-mapper"), vec![], ...).unwrap();
    let t2 = create_todo("Generate hypotheses", ..., Some("web-hypothesis-chaining"), vec![t1.clone()], ...).unwrap();
    let t3 = create_todo("Test loop", ..., Some("web-hypothesis-tester"), vec![t2.clone()], ...).unwrap();
    
    // Execute
    assert_eq!(get_ready_todos().unwrap().len(), 1); // Only t1 ready
    
    mark_complete(&t1, json!({"endpoints": 47})).unwrap();
    assert_eq!(get_ready_todos().unwrap().len(), 1); // Now t2 ready
    
    mark_complete(&t2, json!({"hypotheses": 12})).unwrap();
    assert_eq!(get_ready_todos().unwrap().len(), 1); // Now t3 ready
    
    mark_complete(&t3, json!({"findings": 8})).unwrap();
    assert_eq!(get_ready_todos().unwrap().len(), 0); // All done
}
```

---

## Performance Considerations

### Efficiency

- Keep todo list reasonable size (< 100 todos)
- Index by ID for fast lookups
- Cache in memory, persist to investigation_memory on changes

### Scalability

- For large plans (> 100 todos), consider hierarchical todos
- Support todo groups/phases
- Implement todo pruning (archive completed/blocked)

---

## Next Steps

1. Implement todo.rs module
2. Add tool definitions to rusty-bidule
3. Update system prompt
4. Test with example: "Run hypothesis testing on X"
5. Validate dynamic planning works
6. Delete recipe loader code

---

**This enables the skill-driven architecture!**
