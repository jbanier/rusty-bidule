#!/bin/bash
#
# Demo: Hypothesis-Driven Testing Loop
#
# Demonstrates the full flow:
# 1. Mapper builds asset graph
# 2. Stratège generates hypotheses
# 3. Exploiteur tests hypothesis
# 4. Chain Critic analyzes results
#

set -e

DEMO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT_DIR="${DEMO_DIR}/demo_output"

echo "========================================="
echo "  Hypothesis-Driven Testing Loop Demo"
echo "========================================="
echo

# Create output directory
mkdir -p "$OUTPUT_DIR"

# Target for demo (example.com - won't actually test, just demonstrate structure)
TARGET="https://api.example.com"

echo "[1/4] Mapper: Building Asset Graph"
echo "------------------------------------"
python3 "${DEMO_DIR}/skills/web-asset-mapper/scripts/build_asset_graph.py" \
    "$TARGET" \
    --passive-only \
    --output "${OUTPUT_DIR}/asset_graph.json"

echo
echo "[+] Asset graph saved to: ${OUTPUT_DIR}/asset_graph.json"
echo

# Check if graph was created
if [ ! -f "${OUTPUT_DIR}/asset_graph.json" ]; then
    echo "[!] Error: Asset graph not created"
    exit 1
fi

echo "[2/4] Stratège: Generating Hypotheses"
echo "--------------------------------------"
python3 "${DEMO_DIR}/skills/web-hypothesis-chaining/scripts/generate_hypotheses.py" \
    --graph "${OUTPUT_DIR}/asset_graph.json" \
    --output "${OUTPUT_DIR}/hypotheses.json" \
    --priority 2  # Show priority 1 and 2

echo
echo "[+] Hypotheses saved to: ${OUTPUT_DIR}/hypotheses.json"
echo

# Display first hypothesis
echo "First hypothesis generated:"
jq '.hypotheses[0] | {id, priority, type, hypothesis, impact_if_confirmed}' "${OUTPUT_DIR}/hypotheses.json" 2>/dev/null || echo "(jq not installed, skipping preview)"
echo

# Create a sample hypothesis file for testing
# (In real flow, this would be the top priority hypothesis from generate_hypotheses.py)
cat > "${OUTPUT_DIR}/test_hypothesis.json" << 'EOF'
{
  "id": "H001",
  "priority": 1,
  "type": "IDOR",
  "target": "https://api.example.com/api/orders/{id}",
  "hypothesis": "User can access other users' orders via predictable ID",
  "rationale": [
    "IDs are sequential (1000-9999)",
    "No authorization check confirmed"
  ],
  "prerequisites": ["user_account"],
  "test_approach": {
    "tool": "curl",
    "steps": [
      "Create order as user_A, capture ID",
      "Access as user_B with user_A's ID",
      "Expected: 403, Actual: 200 = IDOR confirmed"
    ]
  },
  "impact_if_confirmed": "high"
}
EOF

echo "[3/4] Exploiteur: Testing Hypothesis"
echo "-------------------------------------"
echo "(Note: Using example.com - won't actually make requests)"
python3 "${DEMO_DIR}/skills/web-hypothesis-tester/scripts/test_hypothesis.py" \
    "${OUTPUT_DIR}/test_hypothesis.json" \
    --scope "https://api.example.com" \
    --output "${OUTPUT_DIR}/test_result.json" \
    --rate-limit 0.1

echo
echo "[+] Test result saved to: ${OUTPUT_DIR}/test_result.json"
echo

# Create sample findings for chain analysis
cat > "${OUTPUT_DIR}/findings.json" << 'EOF'
[
  {
    "id": "F001",
    "type": "IDOR",
    "severity": "medium",
    "target": "/api/orders/{id}",
    "title": "IDOR on orders endpoint",
    "description": "Users can access other users' orders"
  },
  {
    "id": "F002",
    "type": "IDOR",
    "severity": "medium",
    "target": "/api/users/{id}",
    "title": "IDOR on users endpoint",
    "description": "Users can access other users' profiles"
  },
  {
    "id": "F003",
    "type": "MassAssignment",
    "severity": "medium",
    "target": "/api/users/{id}",
    "title": "Mass assignment on user update",
    "description": "Can set is_admin field via PATCH"
  }
]
EOF

echo "[4/4] Chain Critic: Analyzing for Missed Chains"
echo "------------------------------------------------"
python3 "${DEMO_DIR}/skills/web-chain-critic/scripts/analyze_chains.py" \
    "${OUTPUT_DIR}/asset_graph.json" \
    "${OUTPUT_DIR}/findings.json" \
    --output "${OUTPUT_DIR}/critic_report.json"

echo
echo "[+] Critic report saved to: ${OUTPUT_DIR}/critic_report.json"
echo

# Display summary
echo
echo "========================================="
echo "  Demo Complete!"
echo "========================================="
echo
echo "Output files:"
echo "  1. Asset graph:      ${OUTPUT_DIR}/asset_graph.json"
echo "  2. Hypotheses:       ${OUTPUT_DIR}/hypotheses.json"
echo "  3. Test result:      ${OUTPUT_DIR}/test_result.json"
echo "  4. Critic report:    ${OUTPUT_DIR}/critic_report.json"
echo

# Show critic summary if jq is available
if command -v jq &> /dev/null; then
    echo "Chain Critic Summary:"
    jq '.summary' "${OUTPUT_DIR}/critic_report.json"
    echo
    echo "Recommended Next Action:"
    jq '.recommended_next_action' "${OUTPUT_DIR}/critic_report.json"
    echo
    echo "Chains Identified:"
    jq '.chains_identified[] | {chain_id, chained_severity, attack_path}' "${OUTPUT_DIR}/critic_report.json"
fi

echo
echo "View full results:"
echo "  cat ${OUTPUT_DIR}/asset_graph.json | jq ."
echo "  cat ${OUTPUT_DIR}/hypotheses.json | jq ."
echo "  cat ${OUTPUT_DIR}/critic_report.json | jq ."
