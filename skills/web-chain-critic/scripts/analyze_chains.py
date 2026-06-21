#!/usr/bin/env python3
"""
Analyze findings for missed chains and combined impacts.

The Chain Critic component - adversarial final review before report generation.
"""

import json
import sys
from typing import List, Dict, Any, Set, Tuple
from dataclasses import dataclass, asdict, field
from collections import defaultdict
import argparse


@dataclass
class Chain:
    """Identified attack chain"""
    chain_id: str
    finding_ids: List[str]
    current_severity: List[str]  # Individual severities
    chained_severity: str  # Combined severity
    attack_path: str  # Human-readable attack path
    impact: str
    recommendation: str  # Re-test or escalate?
    confidence: str  # high, medium, low


@dataclass
class CoverageGap:
    """High-priority untested area"""
    gap_id: str
    asset: str
    context: str
    risk: str
    recommendation: str
    priority: str  # critical, high, medium


@dataclass
class SeverityEscalation:
    """Finding that should be escalated"""
    finding_id: str
    current_severity: str
    recommended_severity: str
    reason: str
    new_impact: str


@dataclass
class CriticReport:
    """Final chain critic output"""
    total_findings: int
    chains_identified: List[Chain] = field(default_factory=list)
    coverage_gaps: List[CoverageGap] = field(default_factory=list)
    severity_escalations: List[SeverityEscalation] = field(default_factory=list)
    findings_requiring_retest: List[str] = field(default_factory=list)
    recommended_next_action: str = ""
    summary: str = ""


class ChainCritic:
    """Identifies missed chains and escalation opportunities"""

    def __init__(self, asset_graph: Dict[str, Any], findings: List[Dict[str, Any]]):
        self.graph = asset_graph
        self.findings = findings
        self.findings_by_id = {f.get("id"): f for f in findings}
        self.findings_by_type = self.group_by_type()
        self.findings_by_target = self.group_by_target()

    def analyze(self) -> CriticReport:
        """Run complete chain analysis"""
        report = CriticReport(total_findings=len(self.findings))

        print("[*] Chain Critic: Analyzing findings for missed chains...", file=sys.stderr)

        # Question 1: Which findings connect?
        report.chains_identified.extend(self.identify_type_patterns())
        report.chains_identified.extend(self.identify_flow_chains())
        report.chains_identified.extend(self.identify_object_reference_chains())

        # Question 2: Business context escalations
        report.severity_escalations.extend(self.identify_business_context_escalations())

        # Question 3: Privilege escalation paths
        report.chains_identified.extend(self.identify_privilege_paths())

        # Question 4: Data exfiltration chains
        report.chains_identified.extend(self.identify_data_exfiltration_chains())

        # Question 5: Untested high-priority areas
        report.coverage_gaps.extend(self.identify_coverage_gaps())

        # Generate summary
        report.summary = self.generate_summary(report)
        report.recommended_next_action = self.recommend_next_action(report)

        print(f"[+] Analysis complete: {len(report.chains_identified)} chains, {len(report.coverage_gaps)} gaps", file=sys.stderr)

        return report

    def group_by_type(self) -> Dict[str, List[Dict]]:
        """Group findings by vulnerability type"""
        by_type = defaultdict(list)
        for finding in self.findings:
            finding_type = finding.get("type", "unknown")
            by_type[finding_type].append(finding)
        return dict(by_type)

    def group_by_target(self) -> Dict[str, List[Dict]]:
        """Group findings by target endpoint"""
        by_target = defaultdict(list)
        for finding in self.findings:
            target = finding.get("target", "")
            by_target[target].append(finding)
        return dict(by_target)

    def identify_type_patterns(self) -> List[Chain]:
        """Find findings of same type that might indicate systemic issue"""
        chains = []

        # Check for multiple IDORs
        if "IDOR" in self.findings_by_type:
            idor_findings = self.findings_by_type["IDOR"]
            if len(idor_findings) >= 2:
                chain = Chain(
                    chain_id="CHAIN-001",
                    finding_ids=[f["id"] for f in idor_findings],
                    current_severity=[f.get("severity", "medium") for f in idor_findings],
                    chained_severity="critical",
                    attack_path="Multiple IDOR findings → Systemic authorization failure",
                    impact="Complete database enumeration via object reference traversal",
                    recommendation="Test full object graph traversal across all IDOR endpoints",
                    confidence="high"
                )
                chains.append(chain)

        return chains

    def identify_flow_chains(self) -> List[Chain]:
        """Find findings on same business flow"""
        chains = []

        # Example: XSS + CSP bypass on admin flow
        xss_findings = self.findings_by_type.get("XSS", [])
        info_findings = [f for f in self.findings if f.get("severity") == "info"]

        for xss in xss_findings:
            # Check if there's a CSP weakness
            csp_weak = any("CSP" in f.get("title", "") or "CSP" in f.get("description", "")
                          for f in info_findings)

            if csp_weak:
                chain = Chain(
                    chain_id=f"CHAIN-XSS-{xss['id']}",
                    finding_ids=[xss["id"], "INFO-CSP"],
                    current_severity=["medium", "low"],
                    chained_severity="critical",
                    attack_path="Stored XSS + Weak CSP → Admin session hijack",
                    impact="If admin views XSS payload, attacker gains admin session token",
                    recommendation="Test XSS payload visibility in admin context",
                    confidence="medium"
                )
                chains.append(chain)

        return chains

    def identify_object_reference_chains(self) -> List[Chain]:
        """Find IDOR chains via object references"""
        chains = []

        # Look for relationships in graph
        relationships = self.graph.get("relationships", [])
        idor_findings = self.findings_by_type.get("IDOR", [])

        if len(idor_findings) >= 2 and len(relationships) > 0:
            # Check if IDORs are connected via relationships
            for rel in relationships:
                if rel.get("type") == "foreign_key":
                    from_endpoint = rel["from_entity"]
                    to_endpoint = rel["to_entity"]

                    # Find IDORs on these endpoints
                    from_idor = next((f for f in idor_findings if from_endpoint in f.get("target", "")), None)
                    to_idor = next((f for f in idor_findings if to_endpoint in f.get("target", "")), None)

                    if from_idor and to_idor:
                        chain = Chain(
                            chain_id=f"CHAIN-OBJ-{from_idor['id']}-{to_idor['id']}",
                            finding_ids=[from_idor["id"], to_idor["id"]],
                            current_severity=[from_idor.get("severity", "medium"), to_idor.get("severity", "medium")],
                            chained_severity="critical",
                            attack_path=f"{from_endpoint} → {rel['field']} → {to_endpoint}",
                            impact="Multi-hop object reference traversal enables full data graph enumeration",
                            recommendation="Test complete traversal path with proof",
                            confidence="high"
                        )
                        chains.append(chain)

        return chains

    def identify_privilege_paths(self) -> List[Chain]:
        """Find privilege escalation attack paths"""
        chains = []

        # Look for: IDOR + mass assignment = privilege escalation
        idor_findings = self.findings_by_type.get("IDOR", [])
        mass_assignment = [f for f in self.findings if "mass assignment" in f.get("title", "").lower()]

        if idor_findings and mass_assignment:
            chain = Chain(
                chain_id="CHAIN-PRIV-ESC",
                finding_ids=[idor_findings[0]["id"], mass_assignment[0]["id"]],
                current_severity=["medium", "medium"],
                chained_severity="critical",
                attack_path="IDOR → Access victim user object → Mass assignment → Set is_admin=true",
                impact="Complete privilege escalation from user to admin",
                recommendation="Test full chain: IDOR to admin user + PATCH is_admin field",
                confidence="high"
            )
            chains.append(chain)

        return chains

    def identify_data_exfiltration_chains(self) -> List[Chain]:
        """Calculate combined data exposure"""
        chains = []

        # Look for sequential ID patterns + IDOR
        idor_findings = self.findings_by_type.get("IDOR", [])

        if idor_findings:
            # Check if IDs are sequential
            endpoints = self.graph.get("endpoints", [])
            sequential_endpoints = [e for e in endpoints
                                   if any(p.get("pattern") == "sequential"
                                         for p in e.get("parameters", {}).get("path", []))]

            if sequential_endpoints:
                # Calculate potential data exposure
                # Example: 9999 IDs * average records per ID
                chain = Chain(
                    chain_id="CHAIN-DATA-EXFIL",
                    finding_ids=[f["id"] for f in idor_findings],
                    current_severity=[f.get("severity", "medium") for f in idor_findings],
                    chained_severity="critical",
                    attack_path="Sequential IDs (1-9999) + IDOR → Mass enumeration",
                    impact="Potential exposure of 10k+ records via automated enumeration. GDPR/PCI breach scope.",
                    recommendation="Test mass enumeration with rate limiting check",
                    confidence="high"
                )
                chains.append(chain)

        return chains

    def identify_business_context_escalations(self) -> List[SeverityEscalation]:
        """Find findings that escalate with business context"""
        escalations = []

        # Example: IDOR on payment endpoint
        for finding in self.findings:
            target = finding.get("target", "")
            current_severity = finding.get("severity", "medium")

            # Payment/financial endpoints
            if any(keyword in target.lower() for keyword in ["payment", "refund", "credit", "balance"]):
                if current_severity in ["low", "medium"]:
                    escalations.append(SeverityEscalation(
                        finding_id=finding["id"],
                        current_severity=current_severity,
                        recommended_severity="high",
                        reason="Finding affects financial operation",
                        new_impact="Potential financial fraud or unauthorized transactions"
                    ))

            # PII endpoints
            if any(keyword in target.lower() for keyword in ["user", "profile", "personal"]):
                if current_severity == "medium" and finding.get("type") == "IDOR":
                    escalations.append(SeverityEscalation(
                        finding_id=finding["id"],
                        current_severity=current_severity,
                        recommended_severity="high",
                        reason="IDOR exposes PII - GDPR/compliance scope",
                        new_impact="Privacy violation, mandatory breach notification required"
                    ))

        return escalations

    def identify_coverage_gaps(self) -> List[CoverageGap]:
        """Find high-priority untested areas"""
        gaps = []

        endpoints = self.graph.get("endpoints", [])

        # Look for discovered but untested endpoints
        for endpoint in endpoints:
            path = endpoint.get("path", "")

            # Payment operations
            if "payment" in path.lower() or "refund" in path.lower():
                # Check if tested
                tested = any(f.get("target") == path for f in self.findings)
                if not tested:
                    gaps.append(CoverageGap(
                        gap_id=f"GAP-{path}",
                        asset=path,
                        context="Payment endpoint discovered but not tested",
                        risk="Financial operations vulnerable to IDOR/manipulation",
                        recommendation=f"Test {path} with IDOR context and business logic abuse",
                        priority="critical"
                    ))

            # Admin endpoints
            if "admin" in path.lower():
                tested = any(f.get("target") == path for f in self.findings)
                if not tested:
                    gaps.append(CoverageGap(
                        gap_id=f"GAP-{path}",
                        asset=path,
                        context="Admin endpoint discovered but not tested",
                        risk="Privileged operations might lack authorization",
                        recommendation=f"Test {path} with user role for vertical privilege escalation",
                        priority="high"
                    ))

        # Look for untested methods on tested endpoints
        for endpoint in endpoints:
            discovered_methods = set(endpoint.get("methods", []))
            tested_methods = set(endpoint.get("tested_methods", []))
            untested = discovered_methods - tested_methods

            if untested and len(tested_methods) > 0:
                # Especially important: write methods
                write_methods = untested & {"PUT", "PATCH", "DELETE"}
                if write_methods:
                    gaps.append(CoverageGap(
                        gap_id=f"GAP-METHODS-{endpoint['path']}",
                        asset=endpoint["path"],
                        context=f"Write methods {write_methods} untested",
                        risk="IDOR read confirmed, write access might also be vulnerable",
                        recommendation=f"Test {write_methods} methods for IDOR write/delete",
                        priority="high"
                    ))

        return gaps

    def generate_summary(self, report: CriticReport) -> str:
        """Generate executive summary"""
        summary_parts = []

        summary_parts.append(f"Analyzed {report.total_findings} findings")

        if report.chains_identified:
            summary_parts.append(f"{len(report.chains_identified)} potential chains identified")

        if report.coverage_gaps:
            critical_gaps = [g for g in report.coverage_gaps if g.priority == "critical"]
            summary_parts.append(f"{len(critical_gaps)} critical coverage gaps found")

        if report.severity_escalations:
            summary_parts.append(f"{len(report.severity_escalations)} severity escalations recommended")

        return ", ".join(summary_parts)

    def recommend_next_action(self, report: CriticReport) -> str:
        """Recommend highest-priority action"""
        # Priority: Critical chains > Critical gaps > Escalations

        critical_chains = [c for c in report.chains_identified if c.chained_severity == "critical" and c.confidence == "high"]
        if critical_chains:
            chain = critical_chains[0]
            return f"Test critical chain: {chain.attack_path}"

        critical_gaps = [g for g in report.coverage_gaps if g.priority == "critical"]
        if critical_gaps:
            gap = critical_gaps[0]
            return gap.recommendation

        if report.severity_escalations:
            esc = report.severity_escalations[0]
            return f"Re-evaluate {esc.finding_id} with business context: {esc.reason}"

        return "All high-priority chains tested, proceed to final report"


def main():
    parser = argparse.ArgumentParser(description="Analyze findings for missed chains")
    parser.add_argument("graph", help="Path to asset graph JSON")
    parser.add_argument("findings", help="Path to findings JSON")
    parser.add_argument("--output", "-o", help="Output file (default: stdout)")

    args = parser.parse_args()

    # Load inputs
    with open(args.graph) as f:
        graph = json.load(f)

    with open(args.findings) as f:
        findings = json.load(f)

    # Run analysis
    critic = ChainCritic(graph, findings)
    report = critic.analyze()

    # Output
    report_dict = asdict(report)
    json_output = json.dumps(report_dict, indent=2)

    if args.output:
        with open(args.output, 'w') as f:
            f.write(json_output)
        print(f"[+] Critic report saved to {args.output}", file=sys.stderr)
    else:
        print(json_output)


if __name__ == "__main__":
    main()
