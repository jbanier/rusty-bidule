# Hypothesis-Driven Web Testing Tools

Python implementation of the hypothesis-driven testing loop components.

## Architecture

```
┌─────────────────────────────────────────────────┐
│  1. Mapper: build_asset_graph.py                │
│     Discovers endpoints, roles, relationships   │
└──────────────────┬──────────────────────────────┘
                   ▼
           asset_graph.json
                   ▼
┌─────────────────────────────────────────────────┐
│  2. Stratège: generate_hypotheses.py            │
│     Generates prioritized attack hypotheses     │
└──────────────────┬──────────────────────────────┘
                   ▼
           hypotheses.json
                   ▼
┌─────────────────────────────────────────────────┐
│  3. Exploiteur: test_hypothesis.py              │
│     Tests hypothesis, collects proof            │
└──────────────────┬──────────────────────────────┘
                   ▼
           findings.json
                   ▼
┌─────────────────────────────────────────────────┐
│  4. Critic: analyze_chains.py                   │
│     Identifies missed chains, escalations       │
└─────────────────────────────────────────────────┘
                   ▼
           critic_report.json
```

## Quick Start

Run the demo:

```bash
./scripts/demo_hypothesis_loop.sh
```

This demonstrates the full flow with example data.

## Tools

### 1. build_asset_graph.py (Mapper)

**Purpose**: Build relational asset graph from web application

**Usage**:
```bash
python3 skills/web-asset-mapper/scripts/build_asset_graph.py \
    https://api.example.com \
    --passive-only \
    --output asset_graph.json
```

**Options**:
- `--passive-only`: Passive discovery only (no active probing)
- `--scope URL [URL...]`: Additional URLs in scope
- `--output FILE`: Output file (default: stdout)

**Output**: JSON asset graph with:
- Endpoints (paths, methods, parameters)
- Roles (permissions, boundaries)
- Technologies (framework, database, headers)
- Relationships (object references, shared namespaces)

### 2. generate_hypotheses.py (Stratège)

**Purpose**: Generate prioritized attack hypotheses from asset graph

**Usage**:
```bash
python3 skills/web-hypothesis-chaining/scripts/generate_hypotheses.py \
    --graph asset_graph.json \
    --findings findings.json \
    --output hypotheses.json \
    --priority 2
```

**Options**:
- `--graph FILE`: Asset graph JSON
- `--findings FILE`: Existing findings JSON (optional)
- `--output FILE`: Output file (default: stdout)
- `--priority N`: Filter by max priority (1=highest, 3=lowest)

**Output**: JSON hypothesis list with:
- Hypothesis ID, type, target
- Rationale (why it might work)
- Test approach (concrete steps)
- Impact if confirmed
- Chains with existing findings

**Hypothesis Types**:
- IDOR (horizontal/vertical)
- Authorization bypass
- Injection (SQL, NoSQL)
- Business logic flaws
- Privilege escalation
- Chained exploits

### 3. test_hypothesis.py (Exploiteur)

**Purpose**: Execute targeted test for specific hypothesis

**Usage**:
```bash
python3 skills/web-hypothesis-tester/scripts/test_hypothesis.py \
    hypothesis.json \
    --scope https://api.example.com \
    --output result.json \
    --rate-limit 0.5
```

**Options**:
- `--scope URL [URL...]`: Authorized scope URLs (required)
- `--output FILE`: Output file (default: stdout)
- `--rate-limit SECONDS`: Delay between requests (default: 0.5)

**Input**: Single hypothesis JSON

**Output**: Test result with:
- Status (confirmed, blocked, unclear, error)
- Severity if confirmed
- Evidence (request/response pairs)
- Proof statement
- Graph updates
- New hypotheses triggered

**Test Methods**:
- IDOR testing (cross-user, cross-tenant, vertical)
- Authorization bypass
- NoSQL injection
- SQL injection
- XSS
- Authentication bypass
- Business logic abuse
- Privilege escalation
- Chain exploits

### 4. analyze_chains.py (Chain Critic)

**Purpose**: Final adversarial review for missed chains

**Usage**:
```bash
python3 skills/web-chain-critic/scripts/analyze_chains.py \
    asset_graph.json \
    findings.json \
    --output critic_report.json
```

**Options**:
- `--output FILE`: Output file (default: stdout)

**Input**:
- Asset graph JSON
- Findings JSON

**Output**: Critic report with:
- Chains identified (combined findings)
- Coverage gaps (untested high-priority areas)
- Severity escalations (business context)
- Recommended next action

**Chain Detection**:
1. **Type patterns**: Multiple IDORs → systemic failure
2. **Flow chains**: XSS + weak CSP → admin hijack
3. **Object references**: IDOR traversal across endpoints
4. **Privilege paths**: IDOR + mass assignment → admin
5. **Data exfiltration**: Sequential IDs + IDOR → mass enumeration

## Example Workflow

### Step 1: Build Asset Graph

```bash
python3 skills/web-asset-mapper/scripts/build_asset_graph.py \
    https://api.example.com \
    --output graph.json
```

### Step 2: Generate Initial Hypotheses

```bash
python3 skills/web-hypothesis-chaining/scripts/generate_hypotheses.py \
    --graph graph.json \
    --output hypotheses.json \
    --priority 1  # Only critical/high priority
```

### Step 3: Test Top Hypothesis

```bash
# Extract first hypothesis
jq '.hypotheses[0]' hypotheses.json > h001.json

# Test it
python3 skills/web-hypothesis-tester/scripts/test_hypothesis.py \
    h001.json \
    --scope https://api.example.com \
    --output result_h001.json
```

### Step 4: Update Graph with Finding

```bash
# If confirmed, add to findings list
jq '.status' result_h001.json  # Check if "confirmed"

# Add finding to findings.json
echo '[' > findings.json
jq '. | {id: .finding_id, type: .type, severity: .severity, target: .target}' result_h001.json >> findings.json
echo ']' >> findings.json
```

### Step 5: Regenerate Hypotheses

```bash
# Critical: Re-run Stratège with new finding
python3 skills/web-hypothesis-chaining/scripts/generate_hypotheses.py \
    --graph graph.json \
    --findings findings.json \
    --output hypotheses_v2.json
```

**Important**: New hypotheses now include chains with F001!

### Step 6: Repeat Until Stop Condition

Continue loop until:
- No critical/high priority hypotheses remain
- Budget exhausted
- Scope boundary reached

### Step 7: Final Chain Analysis

```bash
python3 skills/web-chain-critic/scripts/analyze_chains.py \
    graph.json \
    findings.json \
    --output critic.json

# View chains discovered
jq '.chains_identified[] | {attack_path, chained_severity}' critic.json
```

## Demo Output Example

From `./scripts/demo_hypothesis_loop.sh`:

**Input**: 3 medium findings
- F001: IDOR on /api/orders
- F002: IDOR on /api/users
- F003: Mass assignment on /api/users

**Chain Critic Output**:
```json
{
  "chains_identified": [
    {
      "chain_id": "CHAIN-001",
      "finding_ids": ["F001", "F002"],
      "current_severity": ["medium", "medium"],
      "chained_severity": "critical",
      "attack_path": "Multiple IDOR findings → Systemic authorization failure",
      "impact": "Complete database enumeration via object reference traversal"
    },
    {
      "chain_id": "CHAIN-PRIV-ESC",
      "finding_ids": ["F001", "F003"],
      "current_severity": ["medium", "medium"],
      "chained_severity": "critical",
      "attack_path": "IDOR → Access victim user object → Mass assignment → Set is_admin=true",
      "impact": "Complete privilege escalation from user to admin"
    }
  ]
}
```

**Result**: 2 critical chains discovered from 3 medium findings

## Integration with Rusty Bidule

These tools are designed to be called from rusty-bidule skills:

1. Agent activates `web-hypothesis-chaining` skill
2. Skill calls `generate_hypotheses.py` via `local__run_skill`
3. Output is parsed and used to select next test
4. Loop continues within rusty-bidule agent workflow

See `recipes/web-app-hypothesis-loop/RECIPE.md` for integration.

## Development

### Requirements

- Python 3.8+
- `curl` for HTTP requests
- `jq` for JSON parsing (optional, for demo)

### Testing

Run the demo to validate all tools:

```bash
./scripts/demo_hypothesis_loop.sh
```

Check output in `demo_output/`:
- `asset_graph.json`
- `hypotheses.json`
- `test_result.json`
- `critic_report.json`

### Adding New Hypothesis Generators

Edit `generate_hypotheses.py`, add method to `HypothesisGenerator`:

```python
def generate_custom_hypotheses(self):
    """Custom hypothesis generator"""
    for endpoint in self.graph.get("endpoints", []):
        # Your logic here
        self.add_hypothesis(
            priority=1,
            type="Custom",
            target=endpoint["path"],
            hypothesis="What to test",
            rationale=["Why it might work"],
            test_approach={"tool": "curl", "steps": [...]},
            impact_if_confirmed="critical"
        )
```

Call it from `generate_all()`.

### Adding New Test Methods

Edit `test_hypothesis.py`, add method to `HypothesisTester`:

```python
def test_custom(self, hypothesis: Dict[str, Any]) -> TestResult:
    """Test custom vulnerability"""
    result = TestResult(hypothesis_id=hypothesis["id"])
    
    # Execute test
    evidence = self.make_request("GET", hypothesis["target"])
    result.evidence.append(evidence)
    
    # Analyze
    if condition_met:
        result.status = "confirmed"
        result.severity = "critical"
        # ... populate result
    
    return result
```

Add to `test_methods` dict in `test_hypothesis()`.

## See Also

- **Architecture**: `docs/HYPOTHESIS_DRIVEN_TESTING.md`
- **Skills**: `skills/web-*/SKILL.md`
- **Recipe**: `recipes/web-app-hypothesis-loop/RECIPE.md`
