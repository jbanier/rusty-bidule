#!/usr/bin/env python3
"""
Generate prioritized attack hypotheses from asset graph.

The Stratège component - reads asset graph, identifies relationships,
generates targeted attack chains.
"""

import json
import sys
from typing import List, Dict, Any
from dataclasses import dataclass, asdict


@dataclass
class Hypothesis:
    """Structured attack hypothesis"""
    id: str
    priority: int  # 1 = highest
    type: str  # IDOR, SQLi, XSS, AuthBypass, Chain, etc.
    target: str  # Endpoint or flow
    hypothesis: str  # What to test
    rationale: List[str]  # Why it might work
    prerequisites: List[str]  # What's needed
    test_approach: Dict[str, Any]  # How to test
    impact_if_confirmed: str  # critical/high/medium/low
    chains_with: List[str] = None  # Finding IDs this builds on


class HypothesisGenerator:
    """Generates hypotheses from asset graph analysis"""

    def __init__(self, asset_graph: Dict[str, Any], findings: List[Dict] = None):
        self.graph = asset_graph
        self.findings = findings or []
        self.hypotheses = []
        self.next_id = 1

    def generate_all(self) -> List[Hypothesis]:
        """Run all hypothesis generators"""
        # Pattern-based generators
        self.generate_idor_hypotheses()
        self.generate_chain_hypotheses()
        self.generate_authz_hypotheses()
        self.generate_injection_hypotheses()
        self.generate_business_logic_hypotheses()

        # Relationship-based generators
        self.generate_object_reference_chains()
        self.generate_privilege_escalation_paths()

        # Coverage-based generators
        self.generate_untested_gaps()

        # Sort by priority
        self.hypotheses.sort(key=lambda h: h.priority)

        return self.hypotheses

    def generate_idor_hypotheses(self):
        """Generate IDOR hypotheses from parameter patterns"""
        endpoints = self.graph.get("endpoints", [])

        for endpoint in endpoints:
            params = endpoint.get("parameters", {})
            path_params = params.get("path", [])

            for param in path_params:
                # Check for predictable ID pattern
                if param.get("type") == "numeric" and param.get("pattern") in ["sequential", "timestamp-based"]:
                    # Check if authorization tested
                    roles_tested = endpoint.get("tested_roles", [])
                    untested_roles = endpoint.get("untested_roles", [])

                    if "authorization" not in endpoint or endpoint.get("authorization") != "confirmed":
                        # High priority: predictable ID + no authz confirmed
                        self.add_hypothesis(
                            priority=1,
                            type="IDOR",
                            target=endpoint["path"],
                            hypothesis=f"Horizontal IDOR - users can access other users' {param['name']}",
                            rationale=[
                                f"Parameter '{param['name']}' is {param['pattern']}",
                                f"No authorization check confirmed",
                                f"Tested roles: {', '.join(roles_tested) if roles_tested else 'none'}"
                            ],
                            prerequisites=["user_account"] if "user" in roles_tested else ["account"],
                            test_approach={
                                "tool": "curl",
                                "steps": [
                                    f"Create resource as user_A, capture {param['name']}",
                                    f"Access as user_B with user_A's {param['name']}",
                                    "Expected: 403/404, Actual: 200 = IDOR confirmed"
                                ]
                            },
                            impact_if_confirmed="high"
                        )

                    # Vertical IDOR if multiple roles observed
                    if len(roles_tested) > 1:
                        # Check for role-based field differences
                        response_fields = endpoint.get("response_fields", {})
                        if len(response_fields) > 1:
                            admin_fields = set(response_fields.get("admin_role", []))
                            user_fields = set(response_fields.get("user_role", []))
                            admin_only = admin_fields - user_fields

                            if admin_only:
                                self.add_hypothesis(
                                    priority=1,
                                    type="IDOR",
                                    target=endpoint["path"],
                                    hypothesis=f"Vertical IDOR - user can access admin resources",
                                    rationale=[
                                        f"Admin role sees extra fields: {', '.join(admin_only)}",
                                        f"ID is {param['pattern']} - predictable",
                                        "No vertical authorization check confirmed"
                                    ],
                                    prerequisites=["user_account", "admin_account"],
                                    test_approach={
                                        "tool": "curl",
                                        "steps": [
                                            "Create resource as admin, capture ID",
                                            "Access as user with admin's ID",
                                            f"Validate: Response contains admin-only fields {list(admin_only)}"
                                        ]
                                    },
                                    impact_if_confirmed="critical"
                                )

    def generate_chain_hypotheses(self):
        """Generate hypotheses that chain existing findings"""
        if not self.findings:
            return

        # Group findings by type
        idor_findings = [f for f in self.findings if f.get("type") == "IDOR"]

        # If multiple IDOR findings, test object reference chains
        if len(idor_findings) >= 2:
            for i, f1 in enumerate(idor_findings):
                for f2 in idor_findings[i+1:]:
                    # Check if one finding's response contains reference to other's resource
                    self.add_hypothesis(
                        priority=1,
                        type="Chain",
                        target=f"{f1.get('target')} → {f2.get('target')}",
                        hypothesis=f"Chained IDOR traversal via object references",
                        rationale=[
                            f"{f1.get('id')}: IDOR confirmed on {f1.get('target')}",
                            f"{f2.get('id')}: IDOR confirmed on {f2.get('target')}",
                            "Test if response contains cross-references enabling traversal"
                        ],
                        prerequisites=[f1.get("id"), f2.get("id")],
                        test_approach={
                            "tool": "curl + jq",
                            "steps": [
                                f"GET {f1.get('target')} → extract reference IDs from response",
                                f"Use extracted IDs to access {f2.get('target')}",
                                "Map full object graph traversal path"
                            ]
                        },
                        impact_if_confirmed="critical",
                        chains_with=[f1.get("id"), f2.get("id")]
                    )

        # IDOR + write access = modification capability
        idor_read = [f for f in self.findings if f.get("type") == "IDOR" and "read" in f.get("description", "").lower()]
        for finding in idor_read:
            target = finding.get("target")
            self.add_hypothesis(
                priority=1,
                type="Chain",
                target=target,
                hypothesis=f"IDOR write access - test modification capability",
                rationale=[
                    f"{finding.get('id')}: IDOR read confirmed",
                    "Write access (PUT/PATCH/DELETE) not yet tested",
                    "Could enable modification/deletion of other users' resources"
                ],
                prerequisites=[finding.get("id")],
                test_approach={
                    "tool": "curl",
                    "steps": [
                        f"Confirm read IDOR on {target}",
                        f"Test PUT/PATCH {target} with victim resource ID",
                        f"Test DELETE {target} with victim resource ID",
                        "Validate: 200 response = write IDOR confirmed"
                    ]
                },
                impact_if_confirmed="critical",
                chains_with=[finding.get("id")]
            )

    def generate_authz_hypotheses(self):
        """Generate authorization-related hypotheses"""
        roles = self.graph.get("roles", [])
        endpoints = self.graph.get("endpoints", [])

        for role in roles:
            if role.get("cross_tenant_tested") is False:
                # Cross-tenant isolation not validated
                self.add_hypothesis(
                    priority=1,
                    type="Authorization",
                    target=f"{role.get('name')} role",
                    hypothesis="Cross-tenant isolation bypass",
                    rationale=[
                        f"{role.get('name')} role exists",
                        "Cross-tenant boundary not tested",
                        "JWT might not contain tenant_id claim",
                        "Backend DB filtering might be app-level only"
                    ],
                    prerequisites=["two_tenant_accounts"],
                    test_approach={
                        "tool": "curl + jwt decode",
                        "steps": [
                            "Create resource as tenant_A",
                            "Access as tenant_B user",
                            "Expected: 403, Actual: 200 = cross-tenant bypass"
                        ]
                    },
                    impact_if_confirmed="critical"
                )

    def generate_injection_hypotheses(self):
        """Generate injection hypotheses based on tech stack"""
        tech = self.graph.get("technologies", {})
        db = tech.get("database", "")
        endpoints = self.graph.get("endpoints", [])

        # NoSQL injection if MongoDB detected
        if "mongo" in db.lower():
            for endpoint in endpoints:
                query_params = endpoint.get("parameters", {}).get("query", [])
                for param in query_params:
                    if param.get("tested_for_injection") is False:
                        self.add_hypothesis(
                            priority=2,
                            type="NoSQLi",
                            target=f"{endpoint['path']}?{param['name']}=",
                            hypothesis=f"NoSQL injection via {param['name']} parameter",
                            rationale=[
                                "Backend uses MongoDB",
                                f"Parameter '{param['name']}' not tested for injection",
                                "Query params often passed directly to DB queries"
                            ],
                            prerequisites=[],
                            test_approach={
                                "tool": "curl",
                                "steps": [
                                    f"Test boolean bypass: ?{param['name']}[$ne]=null",
                                    f"Test time-based: ?{param['name']}[$where]=sleep(5000)",
                                    "Validate: Response change or 5s delay = injection confirmed"
                                ]
                            },
                            impact_if_confirmed="high"
                        )

    def generate_business_logic_hypotheses(self):
        """Generate business logic abuse hypotheses"""
        endpoints = self.graph.get("endpoints", [])

        # Look for payment/financial endpoints
        payment_endpoints = [e for e in endpoints if any(x in e["path"].lower() for x in ["payment", "refund", "credit", "balance"])]

        for endpoint in payment_endpoints:
            # Check if rate limiting observed
            if endpoint.get("rate_limiting") == "none observed":
                self.add_hypothesis(
                    priority=1,
                    type="BusinessLogic",
                    target=endpoint["path"],
                    hypothesis="Race condition on financial operation",
                    rationale=[
                        f"Financial endpoint: {endpoint['path']}",
                        "No rate limiting observed",
                        "Concurrent requests might bypass balance checks"
                    ],
                    prerequisites=[],
                    test_approach={
                        "tool": "turbo-intruder",
                        "steps": [
                            "Send 2 simultaneous purchase requests",
                            "Expected: One succeeds, one blocked",
                            "Actual: Both succeed with single balance deduction = race condition"
                        ]
                    },
                    impact_if_confirmed="critical"
                )

    def generate_object_reference_chains(self):
        """Generate hypotheses based on object relationships"""
        relationships = self.graph.get("relationships", [])

        for rel in relationships:
            if rel.get("type") == "foreign_key":
                self.add_hypothesis(
                    priority=2,
                    type="Chain",
                    target=f"{rel['from']} → {rel['to']}",
                    hypothesis=f"Object reference traversal via {rel['field']}",
                    rationale=[
                        f"Relationship detected: {rel['from']} links to {rel['to']}",
                        f"Via field: {rel['field']}",
                        "Test if access to parent object enables child object access"
                    ],
                    prerequisites=[],
                    test_approach={
                        "tool": "curl + jq",
                        "steps": [
                            f"GET {rel['from']} → extract {rel['field']}",
                            f"Use extracted ID to access {rel['to']}",
                            "Validate: Unauthorized access via reference chain"
                        ]
                    },
                    impact_if_confirmed="high"
                )

    def generate_privilege_escalation_paths(self):
        """Generate privilege escalation hypotheses"""
        roles = self.graph.get("roles", [])

        # Look for user update endpoints
        endpoints = self.graph.get("endpoints", [])
        user_update_endpoints = [e for e in endpoints if "user" in e["path"].lower() and any(m in e.get("methods", []) for m in ["PUT", "PATCH"])]

        for endpoint in user_update_endpoints:
            if not endpoint.get("mass_assignment_tested"):
                self.add_hypothesis(
                    priority=1,
                    type="PrivilegeEscalation",
                    target=endpoint["path"],
                    hypothesis="Mass assignment enables privilege escalation",
                    rationale=[
                        f"User update endpoint: {endpoint['path']}",
                        "Mass assignment not tested",
                        "Might accept is_admin, role, or permission fields"
                    ],
                    prerequisites=["user_account"],
                    test_approach={
                        "tool": "curl",
                        "steps": [
                            f"PATCH {endpoint['path']} with {{\"is_admin\": true}}",
                            "Check if admin field accepted",
                            "Test admin panel access with modified token"
                        ]
                    },
                    impact_if_confirmed="critical"
                )

    def generate_untested_gaps(self):
        """Generate hypotheses for coverage gaps"""
        endpoints = self.graph.get("endpoints", [])

        for endpoint in endpoints:
            # Untested methods
            discovered_methods = endpoint.get("methods", [])
            tested_methods = endpoint.get("tested_methods", [])
            untested = set(discovered_methods) - set(tested_methods)

            for method in untested:
                self.add_hypothesis(
                    priority=3,  # Lower priority - coverage gap
                    type="Coverage",
                    target=f"{method} {endpoint['path']}",
                    hypothesis=f"Test {method} method on {endpoint['path']}",
                    rationale=[
                        f"Method {method} discovered but not tested",
                        "Might have different authorization logic",
                        "Could reveal verb tampering vulnerability"
                    ],
                    prerequisites=[],
                    test_approach={
                        "tool": "curl",
                        "steps": [
                            f"{method} {endpoint['path']}",
                            "Compare authorization with tested methods"
                        ]
                    },
                    impact_if_confirmed="medium"
                )

    def add_hypothesis(self, **kwargs):
        """Add hypothesis to list"""
        h_id = f"H{self.next_id:03d}"
        self.next_id += 1

        hypothesis = Hypothesis(
            id=h_id,
            priority=kwargs.get("priority", 3),
            type=kwargs.get("type", "Unknown"),
            target=kwargs.get("target", ""),
            hypothesis=kwargs.get("hypothesis", ""),
            rationale=kwargs.get("rationale", []),
            prerequisites=kwargs.get("prerequisites", []),
            test_approach=kwargs.get("test_approach", {}),
            impact_if_confirmed=kwargs.get("impact_if_confirmed", "medium"),
            chains_with=kwargs.get("chains_with", [])
        )

        self.hypotheses.append(hypothesis)


def main():
    """CLI entry point"""
    import argparse

    parser = argparse.ArgumentParser(description="Generate attack hypotheses from asset graph")
    parser.add_argument("--graph", required=True, help="Path to asset graph JSON file")
    parser.add_argument("--findings", help="Path to findings JSON file")
    parser.add_argument("--output", help="Output file (default: stdout)")
    parser.add_argument("--priority", type=int, help="Filter by max priority (1-3)")

    args = parser.parse_args()

    # Load graph
    with open(args.graph) as f:
        graph = json.load(f)

    # Load findings if provided
    findings = []
    if args.findings:
        with open(args.findings) as f:
            findings = json.load(f)

    # Generate hypotheses
    generator = HypothesisGenerator(graph, findings)
    hypotheses = generator.generate_all()

    # Filter by priority if requested
    if args.priority:
        hypotheses = [h for h in hypotheses if h.priority <= args.priority]

    # Output
    output = {
        "generated_at": "2026-06-21",  # Could use datetime
        "total_hypotheses": len(hypotheses),
        "hypotheses": [asdict(h) for h in hypotheses]
    }

    if args.output:
        with open(args.output, 'w') as f:
            json.dump(output, f, indent=2)
    else:
        print(json.dumps(output, indent=2))


if __name__ == "__main__":
    main()
