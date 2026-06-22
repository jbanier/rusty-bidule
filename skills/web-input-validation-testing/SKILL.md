---
name: web-input-validation-testing
description: Plan and execute scoped input validation testing for SQLi, NoSQLi, XSS, command injection, and other injection vulnerabilities
metadata:
  keywords: input-validation, sqli, xss, injection, ssrf, xxe, ssti
---

# Web Input Validation Testing

Plan and execute comprehensive input validation testing while respecting scope and authorization boundaries.

## Purpose

Test web application inputs for injection vulnerabilities:
- SQL injection (SQLi)
- NoSQL injection (NoSQLi)
- Cross-Site Scripting (XSS, DOM XSS)
- Command injection
- Path traversal
- Server-Side Template Injection (SSTI)
- XML External Entity (XXE)
- Server-Side Request Forgery (SSRF)
- Prototype pollution
- Deserialization attacks

## Usage Pattern

**When to use**:
- User requests input validation testing
- Pentesting web application forms/APIs
- Testing for injection vulnerabilities

**Example invocation**:
```
User: "Test https://api.example.com for injection vulnerabilities"
User: "Check all input fields for SQLi and XSS"
```

## Testing Approach

### Phase 1: Authorization Confirmation

**Critical first step**: Verify scope and authorization

1. Read scope from investigation_memory
2. Confirm authorization for:
   - Active testing (sending payloads)
   - Out-of-band callbacks (e.g., Burp Collaborator)
   - Destructive payloads (if any)
   - Data extraction attempts
   - Rate limit boundaries

3. If unclear, activate `web-scope-guard` skill to validate

**Stop if not authorized** - do not proceed with active testing!

### Phase 2: Parameter Discovery

Activate `web-input-probe` or `web-parameter-discovery` to:

1. Enumerate all input points:
   - GET/POST parameters
   - Headers
   - Cookies  
   - JSON/XML body fields
   - File uploads

2. Identify input contexts:
   - Database queries
   - OS commands
   - Template rendering
   - XML parsing
   - URL fetching

3. Build parameter/context checklist

### Phase 3: Injection Testing

For each parameter and context, test with **benign payloads first**:

#### SQL Injection

**Benign detection**:
```sql
' OR '1'='1   # Boolean-based
' AND SLEEP(5)--   # Time-based
```

**Validation**: Check for:
- Error messages revealing DB type
- Boolean logic changes (different responses)
- Time delays (5-second response)

**Do not extract data unless explicitly authorized**

#### NoSQL Injection

**Benign detection** (MongoDB example):
```javascript
{"username": {"$ne": null}}
{"username": {"$regex": ".*"}}
```

**Validation**: Check for authentication bypass or data leakage

#### Cross-Site Scripting (XSS)

**Benign payload**:
```html
<script>alert('XSS')</script>
<img src=x onerror=alert('XSS')>
```

**For DOM XSS**: Check JavaScript source/sink analysis

**Validation**: 
- Reflected in response without encoding?
- Executes in browser context?
- CSP blocks it? (Check with `web-crypto-posture`)

#### Command Injection

**Benign detection**:
```bash
; sleep 5
| whoami
`id`
```

**Validation**: Time delays or OS command output

#### Path Traversal

**Benign payloads**:
```
../../../etc/passwd
....//....//etc/passwd
..%2F..%2F..%2Fetc/passwd
```

**Validation**: File content disclosure

#### SSTI (Server-Side Template Injection)

**Detection payloads** (template engine-specific):
```
{{7*7}}  # Jinja2, Twig
<%= 7*7 %>  # ERB
${7*7}  # FreeMarker
```

**Validation**: Check if `49` appears in response

#### XXE (XML External Entity)

**Benign detection**:
```xml
<?xml version="1.0"?>
<!DOCTYPE foo [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>
<foo>&xxe;</foo>
```

**Do not use OOB XXE unless authorized**

#### SSRF (Server-Side Request Forgery)

**Benign detection**:
```
http://localhost:8080/admin
http://169.254.169.254/latest/meta-data/
```

**Validation**: Response indicates internal resource access

**Do not use OOB callbacks unless authorized**

### Phase 4: Evidence Collection

For each confirmed vulnerability:

1. **Minimal proof**: Show detection, not exploitation
2. **Request/response**: Capture full HTTP exchange
3. **Impact statement**: What attacker could do
4. **Remediation**: How to fix (parameterized queries, output encoding, etc.)

Store in `investigation_memory.findings`

### Phase 5: Gap Summary

Document:
- Parameters tested
- Parameters blocked (rate limiting, WAF)
- Parameters requiring authorization (OOB, destructive)
- Coverage gaps (untested contexts)

## Safety Guidelines

### Do NOT

- ❌ Extract data unless authorized
- ❌ Use destructive payloads (DROP, DELETE) unless authorized
- ❌ Use OOB callbacks (Collaborator, webhook.site) unless authorized
- ❌ Trigger denial-of-service
- ❌ Bypass WAF/rate-limiting aggressively
- ❌ Test production systems without authorization

### DO

- ✅ Confirm scope and authorization first
- ✅ Use benign detection payloads
- ✅ Respect rate limits
- ✅ Stop if scope violation detected
- ✅ Document authorization gaps
- ✅ Keep proof minimal (detection, not exploitation)

## Related Skills

Activate these skills as needed:

- `web-scope-guard`: Validate scope boundaries
- `web-input-probe` or `web-parameter-discovery`: Enumerate parameters
- `web-vulnerability-taxonomy`: Map findings to OWASP/CWE
- `web-finding-validator`: Validate findings before reporting
- `web-crypto-posture`: Check CSP, HSTS (for XSS impact)

## Example Execution

```
User: "Test https://api.example.com/search for injection vulnerabilities"
LLM (activates this skill):
  1. **Authorization check**:
     Reading investigation_memory.scope...
     Target: api.example.com
     Authorization: Active testing ✅, OOB callbacks ❌
     
  2. **Parameter discovery**:
     Activating web-parameter-discovery...
     Found: ?q (query parameter), ?filter (query parameter)
     
  3. **Testing ?q parameter**:
     
     **SQLi test**:
     GET /search?q=test' OR '1'='1
     → Response: Error message reveals MySQL
     → ✅ Confirmed: SQL injection (error-based)
     
     **XSS test**:
     GET /search?q=<script>alert('XSS')</script>
     → Response: Script reflected without encoding
     → ✅ Confirmed: Reflected XSS
     
  4. **Testing ?filter parameter**:
     
     **NoSQL injection test**:
     GET /search?filter[$ne]=null
     → Response: 403 Forbidden (WAF blocked)
     → ❌ Blocked by WAF
     
  5. **Evidence collection**:
     F001: SQL injection on /search?q
       - Payload: test' OR '1'='1
       - Impact: Database enumeration possible
       - Remediation: Use parameterized queries
     
     F002: Reflected XSS on /search?q
       - Payload: <script>alert('XSS')</script>
       - Impact: Session hijacking possible (no CSP)
       - Remediation: Output encoding + CSP header
     
  6. **Gap summary**:
     - Tested: q, filter parameters
     - Blocked: NoSQL injection (WAF)
     - Not tested: POST parameters (require authentication)
     - Authorization gap: Cannot test authenticated endpoints (no credentials)

Done! 2 confirmed vulnerabilities, 1 blocked by WAF, 1 authorization gap.
```

## Output Format

```markdown
## Input Validation Testing Results

### Scope
- Target: https://api.example.com
- Authorization: Active testing ✅, OOB ❌, Destructive ❌

### Parameters Tested
| Parameter | Context | SQLi | XSS | Other | Result |
|-----------|---------|------|-----|-------|--------|
| ?q | Query | ✅ Vuln | ✅ Vuln | - | 2 findings |
| ?filter | Query | ❌ Blocked | - | NoSQL ❌ Blocked | WAF |
| POST /login | Body | ⏭️ Needs auth | - | - | Gap |

### Findings
1. **F001: SQL Injection on /search?q** (Critical)
   - Type: Error-based SQLi
   - Payload: `test' OR '1'='1`
   - Evidence: MySQL error message in response
   - Impact: Database enumeration, data extraction
   - Remediation: Parameterized queries

2. **F002: Reflected XSS on /search?q** (High)
   - Type: Reflected XSS (no CSP)
   - Payload: `<script>alert('XSS')</script>`
   - Evidence: Script reflected unencoded
   - Impact: Session hijacking, credential theft
   - Remediation: Output encoding + Content-Security-Policy header

### Coverage Gaps
- POST parameters require authentication (no test credentials)
- File upload endpoints not tested (out of scope)
- GraphQL mutations not tested (discovered but complex)

### Blocked Tests
- NoSQL injection blocked by WAF on ?filter parameter
- Command injection tests trigger rate limiting

### Recommendations
1. Fix F001 (SQLi) immediately - critical severity
2. Implement CSP to mitigate F002 impact
3. Provide test credentials for authenticated endpoint testing
4. Review WAF rules (blocking legitimate security tests)
```

---

**Use this skill for comprehensive, safe, scoped input validation testing.**
