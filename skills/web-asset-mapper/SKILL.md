---
name: web-asset-mapper
description: Builds structured asset graph with relationships between endpoints, roles, parameters, and technologies - the "Mapper" component
metadata:
  keywords: mapper, graph, assets, relationships, recon, discovery
---

# Web Asset Mapper

The **Mapper** component of hypothesis-driven testing. Constructs a relational graph of discovered assets, not just flat lists. Captures **what connects to what**, enabling the Stratège to identify chain opportunities.

## Philosophy: Relationships > Lists

**Bad mapping** (flat list):
```
- /api/orders
- /api/users  
- /api/payments
- JWT authentication
- User role exists
```

**Good mapping** (relational graph):
```
/api/orders/{id}
  ├─ Auth: JWT required
  ├─ Roles: user, admin (tested), guest (untested)
  ├─ Parameters:
  │   └─ id: numeric, sequential pattern, range 1000-9999
  ├─ Relationships:
  │   ├─ Links to /api/users/{user_id} (order.user_id field)
  │   └─ Creates entry in /api/payments (order.payment_id)
  └─ Observations:
      ├─ User role can access any order ID
      └─ Admin gets extra 'internal_notes' field
```

This graph reveals:
- **IDOR hypothesis**: id is sequential + no authz seen
- **Privilege escalation**: admin field differences
- **Object reference chain**: order → user → payment (multi-step IDOR?)

## What to Map

### 1. Endpoints
```json
{
  "path": "/api/orders/{id}",
  "methods": ["GET", "POST", "PUT", "PATCH", "DELETE"],
  "auth_required": true,
  "auth_method": "JWT",
  "tested_roles": ["user", "admin"],
  "untested_roles": ["guest", "manager"],
  "parameters": {
    "path": [{"name": "id", "type": "numeric", "pattern": "sequential"}],
    "query": [{"name": "include", "type": "string", "values": ["user", "payment"]}],
    "body": [{"name": "status", "type": "enum", "values": ["pending", "shipped"]}]
  },
  "response_fields": {
    "user_role": ["id", "status", "user_id", "total"],
    "admin_role": ["id", "status", "user_id", "total", "internal_notes", "cost"]
  },
  "rate_limiting": "none observed",
  "content_type": "application/json",
  "related_endpoints": ["/api/users/{user_id}", "/api/payments/{payment_id}"]
}
```

### 2. Roles & Permissions
```json
{
  "name": "user",
  "authentication": "JWT",
  "permissions_observed": [
    "read own orders",
    "create orders",
    "update order status (own only?)" // uncertainty flag
  ],
  "permissions_blocked": [
    "delete orders (403 on DELETE /api/orders/123)"
  ],
  "cross_tenant_tested": false,
  "privilege_boundaries": {
    "vs_admin": "admin sees internal_notes field, user doesn't",
    "vs_guest": "not tested yet"
  }
}
```

### 3. Parameter Patterns
```json
{
  "name": "id",
  "type": "numeric",
  "pattern": "sequential", // or "uuid", "random", "timestamp-based"
  "observed_range": [1000, 9999],
  "sample_values": [1234, 1235, 1236],
  "predictability": "high",
  "appears_in": [
    "/api/orders/{id}",
    "/api/users/{id}",
    "/api/payments/{id}"
  ],
  "hypothesis": "Shared ID namespace across entities? Test cross-type IDOR"
}
```

### 4. Technologies & Fingerprints
```json
{
  "backend_framework": "Express.js 4.17.1",
  "evidence": "X-Powered-By: Express header",
  "database": "MongoDB (inferred from error messages)",
  "auth_method": "JWT (RS256 algorithm observed)",
  "cdn": "Cloudflare",
  "security_headers": {
    "csp": "none",
    "hsts": "present",
    "x-frame-options": "DENY"
  },
  "implications": [
    "Express <4.18 vulnerable to qs pollution (CVE-2022-24999)",
    "MongoDB → likely app-level tenant filtering (test cross-tenant)",
    "No CSP → XSS impact higher"
  ]
}
```

### 5. Relationships (The Key Part!)
```json
{
  "type": "foreign_key",
  "from": "/api/orders/{id}",
  "to": "/api/users/{user_id}",
  "field": "order.user_id",
  "hypothesis": "If orders have IDOR, can we traverse to users via user_id?"
},
{
  "type": "shared_namespace",
  "entities": ["/api/orders/{id}", "/api/invoices/{id}"],
  "evidence": "Both use sequential IDs starting at 1000",
  "hypothesis": "Test if order ID 1234 returns invoice data"
},
{
  "type": "role_escalation_path",
  "from_role": "user",
  "to_role": "admin",
  "via": "PATCH /api/users/{id} with is_admin=true",
  "status": "untested"
}
```

## Discovery Methods

### Passive Mapping (Safe Recon)
- Spider sitemap.xml, robots.txt
- Parse JavaScript bundles for API routes
- Extract GraphQL schema introspection
- Analyze HTML forms, fetch() calls
- Review Swagger/OpenAPI specs if accessible

Tools: `katana`, `hakrawler`, `getJS`, `graphql introspection`

### Active Mapping (Authorized Probing)
- Enumerate endpoints via wordlists
- Fuzz parameter discovery
- Test HTTP verb tampering
- Probe role differences
- Identify parameter types via error messages

Tools: `ffuf`, `arjun`, `param-miner`

### Relationship Extraction
- Trace object references in responses
- Map ID patterns across endpoints
- Compare role-based response diffs
- Build state transition graphs (create → update → delete flows)

## Tools

Tools:
  - name: Build Asset Graph
    slug: build-asset-graph
    description: Discover endpoints, parameters, roles, and technologies - create initial graph structure
    script: scripts/build_asset_graph.py
    network: true

  - name: Extract Relationships
    slug: extract-relationships
    description: Analyze responses and patterns to identify connections between entities
    script: scripts/extract_relationships.py
    network: false

  - name: Enrich Graph
    slug: enrich-graph
    description: Add new discovery to existing graph, update relationships, flag conflicts
    script: scripts/enrich_graph.py
    network: false

  - name: Graph Visualization
    slug: graph-viz
    description: Generate visual graph representation (ASCII or Graphviz DOT format)
    script: scripts/graph_visualization.py
    network: false

## Integration with Investigation Memory

Store graph at:
```
investigation_memory.asset_graph
```

Update incrementally - don't rebuild from scratch each time:
```python
# Good: incremental enrichment
graph = get_investigation_memory()["asset_graph"]
graph["endpoints"].append(new_endpoint)
graph["relationships"].append(new_relationship)
update_investigation_memory({"asset_graph": graph})

# Bad: full rebuild each time (loses relationships)
graph = rebuild_graph_from_scratch()  # loses manual annotations
```

## Example: From Discovery to Graph

**Discovery input** (raw data):
```
GET /api/orders/1234 → 200 OK
GET /api/orders/1235 → 200 OK
GET /api/orders/9999 → 404 Not Found

Response (user role):
{
  "id": 1234,
  "user_id": 567,
  "status": "pending",
  "total": 99.99
}

Response (admin role):
{
  "id": 1234,
  "user_id": 567,
  "status": "pending",
  "total": 99.99,
  "internal_notes": "Rush order",
  "cost": 45.00
}
```

**Mapped graph output**:
```json
{
  "endpoints": [{
    "path": "/api/orders/{id}",
    "methods": ["GET"],
    "parameters": {
      "path": [{"name": "id", "type": "numeric", "pattern": "sequential", "range": [1000, 9999]}]
    },
    "role_diffs": {
      "admin_sees": ["internal_notes", "cost"],
      "user_sees": ["id", "user_id", "status", "total"]
    }
  }],
  "relationships": [{
    "type": "object_reference",
    "from": "/api/orders/{id}",
    "to": "/api/users/{user_id}",
    "field": "user_id",
    "value": 567
  }],
  "hypotheses_generated": [
    "IDOR: Sequential IDs + no authz check seen → test cross-user access",
    "Vertical privilege escalation: Admin fields leakage if user can elevate",
    "Chained IDOR: orders.user_id → test /api/users/567 access"
  ]
}
```

## Critical: Capture Uncertainties

Flag what's **assumed but not confirmed**:

```json
{
  "endpoint": "/api/orders/{id}",
  "observations": [
    "User can access order 1234 (own order)",
    "User can access order 1235 (also own order?)",
    "ASSUMPTION: 1235 might be another user's order - NOT CONFIRMED"
  ],
  "next_test": "Create order as user_A, capture ID, test access from user_B"
}
```

Don't claim IDOR until cross-user test confirms it!

## Output Format

```markdown
# Asset Graph Update - [Timestamp]

## New Discoveries
- 12 endpoints mapped (+5 from last run)
- 3 roles tested: user, admin, guest
- 47 parameters discovered
- 8 relationships identified

## Key Relationships Found
1. /api/orders → /api/users (via user_id field)
2. /api/payments → /api/orders (via order_id field)  
3. Shared ID namespace: orders/invoices/tickets (all sequential 1000-9999)

## Technology Stack
- Backend: Express.js 4.17.1 (CVE-2022-24999 applicable)
- Auth: JWT RS256
- Database: MongoDB (inferred)
- No rate limiting observed on /api/auth/*

## Hypothesis Seeds (for Stratège)
1. Test cross-user IDOR on orders (sequential IDs)
2. Test cross-tenant isolation (no tenant field in JWT)
3. Test JWT algorithm confusion (RS256 → none/HS256)
4. Test privilege escalation via user_id reference chain

## Coverage Gaps
- /api/reports/* endpoints not tested yet
- Manager role exists but no test account
- GraphQL endpoint discovered but schema not introspected

## Next Mapper Actions
- Introspect GraphQL schema
- Enumerate /api/reports/* with ffuf
- Request manager role test credentials
```

## Integration with Hypothesis Loop

```
Mapper → builds/enriches graph
   ↓
Stratège → reads graph, generates hypotheses
   ↓
Exploiteur → tests hypothesis, returns result
   ↓
Mapper → enriches graph with finding
   ↓
Stratège → re-generates based on new graph state
   ↓
[LOOP CONTINUES]
```

## Anti-Patterns

❌ **Don't**: Just run `ffuf` and dump results as flat list
✅ **Do**: Parse ffuf output, identify patterns, build relationships

❌ **Don't**: Treat each endpoint independently
✅ **Do**: Link endpoints via parameters, trace object references

❌ **Don't**: Overwrite graph on each update
✅ **Do**: Merge new data, preserve relationships, flag conflicts

---

**The Mapper feeds the Stratège. Rich graphs enable smart hypotheses.**
