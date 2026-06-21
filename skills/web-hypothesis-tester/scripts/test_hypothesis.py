#!/usr/bin/env python3
"""
Test specific attack hypotheses with targeted techniques.

The Exploiteur component - executes surgical tests against hypotheses,
collects proof, returns structured results.
"""

import json
import sys
import subprocess
import time
import re
from typing import Dict, Any, Optional, List
from dataclasses import dataclass, asdict, field
from urllib.parse import urljoin, urlparse
import argparse


@dataclass
class TestEvidence:
    """Evidence from a test execution"""
    request_num: int
    method: str
    url: str
    headers: Dict[str, str] = field(default_factory=dict)
    body: Optional[str] = None
    response_code: int = 0
    response_headers: Dict[str, str] = field(default_factory=dict)
    response_body: Optional[str] = None
    timing_ms: Optional[int] = None


@dataclass
class TestResult:
    """Result of hypothesis test"""
    hypothesis_id: str
    status: str = "unclear"  # confirmed, blocked, unclear, error
    severity: Optional[str] = None  # critical, high, medium, low
    finding_id: Optional[str] = None
    title: Optional[str] = None
    evidence: List[TestEvidence] = field(default_factory=list)
    proof: Optional[str] = None
    graph_updates: Dict[str, Any] = field(default_factory=dict)
    new_hypotheses_triggered: List[str] = field(default_factory=list)
    error_message: Optional[str] = None


class HypothesisTester:
    """Executes targeted hypothesis tests"""

    def __init__(self, scope: List[str], rate_limit_delay: float = 0.5):
        self.scope = scope
        self.rate_limit_delay = rate_limit_delay
        self.evidence_counter = 1

    def test_hypothesis(self, hypothesis: Dict[str, Any]) -> TestResult:
        """Execute test for a specific hypothesis"""
        h_id = hypothesis.get("id", "unknown")
        h_type = hypothesis.get("type", "unknown")

        print(f"[*] Testing hypothesis {h_id}: {hypothesis.get('hypothesis', '')}", file=sys.stderr)

        # Check scope
        if not self.in_scope(hypothesis.get("target", "")):
            return TestResult(
                hypothesis_id=h_id,
                status="blocked",
                error_message="Target out of scope"
            )

        # Route to appropriate test method based on type
        test_methods = {
            "IDOR": self.test_idor,
            "Authorization": self.test_authorization,
            "NoSQLi": self.test_nosql_injection,
            "SQLi": self.test_sql_injection,
            "XSS": self.test_xss,
            "AuthBypass": self.test_auth_bypass,
            "BusinessLogic": self.test_business_logic,
            "PrivilegeEscalation": self.test_privilege_escalation,
            "Chain": self.test_chain,
            "Coverage": self.test_coverage_gap
        }

        test_method = test_methods.get(h_type, self.test_generic)

        try:
            return test_method(hypothesis)
        except Exception as e:
            print(f"[!] Test error: {e}", file=sys.stderr)
            return TestResult(
                hypothesis_id=h_id,
                status="error",
                error_message=str(e)
            )

    def test_idor(self, hypothesis: Dict[str, Any]) -> TestResult:
        """Test for IDOR vulnerability"""
        h_id = hypothesis["id"]
        target = hypothesis["target"]
        test_approach = hypothesis.get("test_approach", {})
        steps = test_approach.get("steps", [])

        result = TestResult(hypothesis_id=h_id)

        # Simulate IDOR test
        # In real implementation, this would:
        # 1. Create resource as role A
        # 2. Access resource as role B
        # 3. Check if access was granted

        print(f"[*] IDOR test on {target}", file=sys.stderr)

        # Example: Test access to predictable ID
        # Step 1: Try to access a resource with ID
        test_url = target.replace("{id}", "1234")  # Example ID

        evidence = self.make_request(
            method="GET",
            url=test_url,
            description="Test access to resource ID 1234"
        )

        result.evidence.append(evidence)

        # Analyze response
        if evidence.response_code == 200:
            # Potential IDOR - got access to resource
            result.status = "confirmed"
            result.severity = "high"
            result.finding_id = self.generate_finding_id()
            result.title = f"IDOR on {target}"
            result.proof = f"Accessed resource with ID 1234, received 200 OK"

            # Graph updates
            result.graph_updates = {
                "endpoints": {
                    target: {
                        "authorization": "broken",
                        "idor_confirmed": True
                    }
                }
            }

            # New hypotheses triggered
            result.new_hypotheses_triggered = [
                f"Test write operations: PUT/PATCH {target}",
                f"Test mass enumeration of IDs",
                f"Test object reference chain from response"
            ]

        elif evidence.response_code in [403, 404]:
            result.status = "blocked"
            result.proof = f"Access denied with {evidence.response_code}"
        else:
            result.status = "unclear"
            result.proof = f"Unexpected response code: {evidence.response_code}"

        return result

    def test_authorization(self, hypothesis: Dict[str, Any]) -> TestResult:
        """Test for authorization bypass"""
        h_id = hypothesis["id"]
        result = TestResult(hypothesis_id=h_id)

        # Test cross-tenant isolation, role-based access, etc.
        print(f"[*] Authorization test", file=sys.stderr)

        # Placeholder - would test with different credentials
        result.status = "unclear"
        result.proof = "Authorization test requires multiple accounts (not implemented in this demo)"

        return result

    def test_nosql_injection(self, hypothesis: Dict[str, Any]) -> TestResult:
        """Test for NoSQL injection"""
        h_id = hypothesis["id"]
        target = hypothesis["target"]

        result = TestResult(hypothesis_id=h_id)

        print(f"[*] NoSQL injection test on {target}", file=sys.stderr)

        # Test 1: Boolean bypass
        payload_url = target.replace("?", "?username[$ne]=null&")

        evidence = self.make_request(
            method="GET",
            url=payload_url,
            description="NoSQL boolean bypass test"
        )

        result.evidence.append(evidence)

        # Check for injection indicators
        if evidence.response_code == 200:
            # Check if response differs from baseline
            # In real implementation, compare with baseline request

            result.status = "unclear"
            result.proof = "Response received, requires manual verification"

        return result

    def test_sql_injection(self, hypothesis: Dict[str, Any]) -> TestResult:
        """Test for SQL injection"""
        h_id = hypothesis["id"]
        result = TestResult(hypothesis_id=h_id)

        # Benign SQL injection tests
        # Use time-based or error-based detection

        result.status = "unclear"
        result.proof = "SQL injection test placeholder"

        return result

    def test_xss(self, hypothesis: Dict[str, Any]) -> TestResult:
        """Test for XSS"""
        h_id = hypothesis["id"]
        result = TestResult(hypothesis_id=h_id)

        # Test with benign XSS payload
        # Check if reflected/stored

        result.status = "unclear"
        result.proof = "XSS test placeholder"

        return result

    def test_auth_bypass(self, hypothesis: Dict[str, Any]) -> TestResult:
        """Test for authentication bypass"""
        h_id = hypothesis["id"]
        result = TestResult(hypothesis_id=h_id)

        # Test JWT algorithm confusion, none algorithm, etc.

        result.status = "unclear"
        result.proof = "Auth bypass test placeholder"

        return result

    def test_business_logic(self, hypothesis: Dict[str, Any]) -> TestResult:
        """Test for business logic flaws"""
        h_id = hypothesis["id"]
        result = TestResult(hypothesis_id=h_id)

        # Test race conditions, price manipulation, etc.

        result.status = "unclear"
        result.proof = "Business logic test placeholder"

        return result

    def test_privilege_escalation(self, hypothesis: Dict[str, Any]) -> TestResult:
        """Test for privilege escalation"""
        h_id = hypothesis["id"]
        target = hypothesis["target"]

        result = TestResult(hypothesis_id=h_id)

        print(f"[*] Privilege escalation test: mass assignment on {target}", file=sys.stderr)

        # Test mass assignment
        test_url = target.replace("{id}", "1")  # Own user ID

        evidence = self.make_request(
            method="PATCH",
            url=test_url,
            body='{"is_admin": true}',
            description="Test mass assignment of is_admin field"
        )

        result.evidence.append(evidence)

        if evidence.response_code == 200:
            # Check if field was accepted
            result.status = "unclear"
            result.proof = "PATCH accepted, requires verification if is_admin field was set"

            result.new_hypotheses_triggered = [
                "Verify admin privileges granted",
                "Test admin panel access"
            ]
        else:
            result.status = "blocked"
            result.proof = f"PATCH rejected with {evidence.response_code}"

        return result

    def test_chain(self, hypothesis: Dict[str, Any]) -> TestResult:
        """Test chained exploit"""
        h_id = hypothesis["id"]
        chains_with = hypothesis.get("chains_with", [])

        result = TestResult(hypothesis_id=h_id)

        print(f"[*] Chain test: combining {', '.join(chains_with)}", file=sys.stderr)

        # Chain tests require multiple steps
        # Execute each prerequisite finding, then test combination

        result.status = "unclear"
        result.proof = f"Chain test requires findings: {', '.join(chains_with)}"

        return result

    def test_coverage_gap(self, hypothesis: Dict[str, Any]) -> TestResult:
        """Test untested method/endpoint"""
        h_id = hypothesis["id"]
        target = hypothesis["target"]

        result = TestResult(hypothesis_id=h_id)

        # Extract method from target (e.g., "PUT /api/users/{id}")
        parts = target.split(" ", 1)
        method = parts[0] if len(parts) > 1 else "GET"
        url = parts[1] if len(parts) > 1 else target

        print(f"[*] Coverage test: {method} {url}", file=sys.stderr)

        evidence = self.make_request(
            method=method,
            url=url,
            description=f"Test {method} method"
        )

        result.evidence.append(evidence)

        if evidence.response_code in [200, 201, 204]:
            result.status = "confirmed"
            result.severity = "low"
            result.finding_id = self.generate_finding_id()
            result.title = f"{method} method accessible on {url}"
            result.proof = f"{method} request accepted with {evidence.response_code}"
        else:
            result.status = "blocked"
            result.proof = f"{method} blocked with {evidence.response_code}"

        return result

    def test_generic(self, hypothesis: Dict[str, Any]) -> TestResult:
        """Generic test handler"""
        h_id = hypothesis["id"]
        result = TestResult(hypothesis_id=h_id)

        result.status = "unclear"
        result.proof = f"No specific test handler for type: {hypothesis.get('type')}"

        return result

    def make_request(self, method: str, url: str, headers: Optional[Dict[str, str]] = None,
                    body: Optional[str] = None, description: str = "") -> TestEvidence:
        """Execute HTTP request and collect evidence"""

        # Rate limiting
        time.sleep(self.rate_limit_delay)

        headers = headers or {}
        evidence = TestEvidence(
            request_num=self.evidence_counter,
            method=method,
            url=url,
            headers=headers,
            body=body
        )
        self.evidence_counter += 1

        print(f"  [{evidence.request_num}] {method} {url} - {description}", file=sys.stderr)

        # Build curl command
        curl_cmd = ["curl", "-s", "-i", "-X", method]

        for key, value in headers.items():
            curl_cmd.extend(["-H", f"{key}: {value}"])

        if body:
            curl_cmd.extend(["-d", body])

        curl_cmd.append(url)

        try:
            start_time = time.time()
            result = subprocess.run(
                curl_cmd,
                capture_output=True,
                text=True,
                timeout=30
            )
            elapsed_ms = int((time.time() - start_time) * 1000)

            evidence.timing_ms = elapsed_ms

            # Parse response
            if result.returncode == 0:
                response = result.stdout

                # Split headers and body
                parts = response.split('\r\n\r\n', 1)
                header_section = parts[0] if len(parts) > 0 else ""
                body_section = parts[1] if len(parts) > 1 else ""

                # Parse status code
                status_line = header_section.split('\r\n')[0] if header_section else ""
                status_match = re.search(r'HTTP/[\d.]+ (\d+)', status_line)
                if status_match:
                    evidence.response_code = int(status_match.group(1))

                # Parse headers
                for line in header_section.split('\r\n')[1:]:
                    if ':' in line:
                        key, value = line.split(':', 1)
                        evidence.response_headers[key.strip().lower()] = value.strip()

                # Store body (truncate if too large)
                evidence.response_body = body_section[:1000] if body_section else None

                print(f"      → {evidence.response_code} ({elapsed_ms}ms)", file=sys.stderr)

        except subprocess.TimeoutExpired:
            evidence.response_code = 0
            evidence.response_body = "Request timeout"
            print(f"      → TIMEOUT", file=sys.stderr)
        except Exception as e:
            evidence.response_code = 0
            evidence.response_body = f"Error: {str(e)}"
            print(f"      → ERROR: {e}", file=sys.stderr)

        return evidence

    def in_scope(self, target: str) -> bool:
        """Check if target is in authorized scope"""
        # Simple scope check - in production, use more sophisticated logic
        for scope_item in self.scope:
            if target.startswith(scope_item):
                return True
        return False

    def generate_finding_id(self) -> str:
        """Generate unique finding ID"""
        # In production, coordinate with investigation_memory to avoid collisions
        import random
        return f"F{random.randint(100, 999)}"


def main():
    parser = argparse.ArgumentParser(description="Test attack hypothesis")
    parser.add_argument("hypothesis", help="Path to hypothesis JSON file")
    parser.add_argument("--scope", nargs="+", required=True, help="Authorized scope URLs")
    parser.add_argument("--output", "-o", help="Output file (default: stdout)")
    parser.add_argument("--rate-limit", type=float, default=0.5, help="Delay between requests (seconds)")

    args = parser.parse_args()

    # Load hypothesis
    with open(args.hypothesis) as f:
        hypothesis = json.load(f)

    # Execute test
    tester = HypothesisTester(scope=args.scope, rate_limit_delay=args.rate_limit)
    result = tester.test_hypothesis(hypothesis)

    # Output
    result_dict = asdict(result)
    json_output = json.dumps(result_dict, indent=2)

    if args.output:
        with open(args.output, 'w') as f:
            f.write(json_output)
        print(f"[+] Test result saved to {args.output}", file=sys.stderr)
    else:
        print(json_output)


if __name__ == "__main__":
    main()
