#!/usr/bin/env python3
"""
Build asset graph from web application reconnaissance.

The Mapper component - discovers endpoints, roles, parameters, technologies,
and most importantly: relationships between entities.
"""

import json
import sys
import re
import subprocess
from typing import List, Dict, Any, Set, Optional
from dataclasses import dataclass, asdict, field
from urllib.parse import urlparse, parse_qs
import argparse


@dataclass
class Parameter:
    """API parameter definition"""
    name: str
    type: str  # numeric, string, boolean, array, object, uuid
    location: str  # path, query, body, header
    pattern: Optional[str] = None  # sequential, uuid, random, timestamp-based
    observed_range: Optional[List[int]] = None
    sample_values: List[str] = field(default_factory=list)
    tested_for_injection: bool = False


@dataclass
class Endpoint:
    """API endpoint definition"""
    path: str
    methods: List[str]
    auth_required: bool = True
    auth_method: Optional[str] = None
    tested_roles: List[str] = field(default_factory=list)
    untested_roles: List[str] = field(default_factory=list)
    parameters: Dict[str, List[Parameter]] = field(default_factory=lambda: {"path": [], "query": [], "body": [], "header": []})
    response_fields: Dict[str, List[str]] = field(default_factory=dict)
    rate_limiting: str = "unknown"
    content_type: str = "application/json"
    related_endpoints: List[str] = field(default_factory=list)
    observations: List[str] = field(default_factory=list)
    tested_methods: List[str] = field(default_factory=list)
    authorization: Optional[str] = None  # "confirmed", "broken", "unknown"


@dataclass
class Role:
    """User role definition"""
    name: str
    authentication: str
    permissions_observed: List[str] = field(default_factory=list)
    permissions_blocked: List[str] = field(default_factory=list)
    cross_tenant_tested: bool = False
    privilege_boundaries: Dict[str, str] = field(default_factory=dict)


@dataclass
class Relationship:
    """Relationship between entities"""
    type: str  # foreign_key, shared_namespace, role_escalation_path, etc.
    from_entity: str
    to_entity: str
    field: Optional[str] = None
    evidence: Optional[str] = None
    hypothesis: Optional[str] = None
    status: str = "untested"  # untested, confirmed, blocked


@dataclass
class AssetGraph:
    """Complete asset graph structure"""
    target_url: str
    endpoints: List[Endpoint] = field(default_factory=list)
    roles: List[Role] = field(default_factory=list)
    technologies: Dict[str, str] = field(default_factory=dict)
    relationships: List[Relationship] = field(default_factory=list)
    findings: List[Dict[str, Any]] = field(default_factory=list)
    metadata: Dict[str, Any] = field(default_factory=dict)


class AssetMapper:
    """Builds asset graph from web application"""

    def __init__(self, target_url: str, scope: Optional[List[str]] = None):
        self.target_url = target_url
        self.scope = scope or [target_url]
        self.graph = AssetGraph(target_url=target_url)
        self.discovered_ids: Dict[str, List[str]] = {}  # Track ID patterns

    def build_graph(self, passive_only: bool = True) -> AssetGraph:
        """Build complete asset graph"""
        print("[*] Building asset graph...", file=sys.stderr)

        # Phase 1: Passive discovery
        self.discover_endpoints_passive()
        self.fingerprint_technologies()

        # Phase 2: Active discovery (if authorized)
        if not passive_only:
            self.discover_endpoints_active()
            self.probe_parameters()
            self.test_role_differences()

        # Phase 3: Relationship extraction
        self.extract_relationships()
        self.identify_id_patterns()

        # Phase 4: Graph enrichment
        self.enrich_graph()

        print(f"[+] Graph complete: {len(self.graph.endpoints)} endpoints, {len(self.graph.relationships)} relationships", file=sys.stderr)

        return self.graph

    def discover_endpoints_passive(self):
        """Passive endpoint discovery from JS, sitemap, robots.txt"""
        print("[*] Phase 1: Passive endpoint discovery", file=sys.stderr)

        # Parse from common sources
        self.parse_robots_txt()
        self.parse_sitemap()
        self.extract_from_javascript()

    def parse_robots_txt(self):
        """Extract endpoints from robots.txt"""
        try:
            result = subprocess.run(
                ["curl", "-s", f"{self.target_url}/robots.txt"],
                capture_output=True,
                text=True,
                timeout=10
            )

            if result.returncode == 0:
                for line in result.stdout.split('\n'):
                    if line.startswith('Disallow:') or line.startswith('Allow:'):
                        path = line.split(':', 1)[1].strip()
                        if path and path != '/':
                            self.add_endpoint(path, ["GET"], "robots.txt")
        except Exception as e:
            print(f"[!] robots.txt parse error: {e}", file=sys.stderr)

    def parse_sitemap(self):
        """Extract endpoints from sitemap.xml"""
        try:
            result = subprocess.run(
                ["curl", "-s", f"{self.target_url}/sitemap.xml"],
                capture_output=True,
                text=True,
                timeout=10
            )

            if result.returncode == 0:
                # Simple regex for <loc> tags
                urls = re.findall(r'<loc>(.*?)</loc>', result.stdout)
                for url in urls:
                    parsed = urlparse(url)
                    if parsed.path:
                        self.add_endpoint(parsed.path, ["GET"], "sitemap.xml")
        except Exception as e:
            print(f"[!] sitemap.xml parse error: {e}", file=sys.stderr)

    def extract_from_javascript(self):
        """Extract API routes from JavaScript bundles"""
        print("[*] Extracting endpoints from JavaScript...", file=sys.stderr)

        # This is a simplified version - in production, use tools like:
        # - linkfinder
        # - getJS + grep for API patterns
        # - retire.js for framework detection

        # For now, just extract common API patterns from main page
        try:
            result = subprocess.run(
                ["curl", "-s", self.target_url],
                capture_output=True,
                text=True,
                timeout=10
            )

            if result.returncode == 0:
                # Common API endpoint patterns
                api_patterns = [
                    r'/api/[a-zA-Z0-9/_-]+',
                    r'/v\d+/[a-zA-Z0-9/_-]+',
                    r'"/[a-zA-Z0-9_-]+/\{[a-zA-Z0-9_]+\}"'
                ]

                for pattern in api_patterns:
                    matches = re.findall(pattern, result.stdout)
                    for match in matches:
                        # Clean up
                        path = match.strip('"\'')
                        self.add_endpoint(path, ["GET", "POST"], "javascript")
        except Exception as e:
            print(f"[!] JavaScript extraction error: {e}", file=sys.stderr)

    def discover_endpoints_active(self):
        """Active endpoint enumeration (requires authorization)"""
        print("[*] Phase 2: Active endpoint enumeration", file=sys.stderr)

        # This would use tools like:
        # - ffuf for directory/endpoint fuzzing
        # - arjun for parameter discovery
        # - GraphQL introspection if detected

        # Placeholder for now
        pass

    def probe_parameters(self):
        """Discover parameters via fuzzing and analysis"""
        print("[*] Probing parameters...", file=sys.stderr)

        for endpoint in self.graph.endpoints:
            # Extract path parameters
            path_params = re.findall(r'\{([a-zA-Z0-9_]+)\}', endpoint.path)
            for param_name in path_params:
                param = Parameter(
                    name=param_name,
                    type="unknown",
                    location="path"
                )
                endpoint.parameters["path"].append(param)

    def test_role_differences(self):
        """Test endpoint access with different roles"""
        print("[*] Testing role-based differences...", file=sys.stderr)

        # This would test each endpoint with:
        # - Anonymous access
        # - User role
        # - Admin role
        # Compare responses to identify field differences

        # For now, add common roles
        self.graph.roles.extend([
            Role(name="anonymous", authentication="none"),
            Role(name="user", authentication="JWT"),
            Role(name="admin", authentication="JWT")
        ])

    def fingerprint_technologies(self):
        """Identify backend technologies"""
        print("[*] Fingerprinting technologies...", file=sys.stderr)

        try:
            result = subprocess.run(
                ["curl", "-sI", self.target_url],
                capture_output=True,
                text=True,
                timeout=10
            )

            if result.returncode == 0:
                headers = result.stdout.lower()

                # Detect framework from headers
                if 'x-powered-by: express' in headers:
                    self.graph.technologies['backend_framework'] = 'Express.js'
                elif 'x-powered-by: php' in headers:
                    self.graph.technologies['backend_framework'] = 'PHP'
                elif 'server: nginx' in headers:
                    self.graph.technologies['web_server'] = 'nginx'

                # Security headers
                self.graph.technologies['security_headers'] = {
                    'csp': 'present' if 'content-security-policy:' in headers else 'none',
                    'hsts': 'present' if 'strict-transport-security:' in headers else 'none',
                    'x-frame-options': 'present' if 'x-frame-options:' in headers else 'none'
                }
        except Exception as e:
            print(f"[!] Fingerprinting error: {e}", file=sys.stderr)

    def extract_relationships(self):
        """Extract relationships between entities"""
        print("[*] Phase 3: Extracting relationships...", file=sys.stderr)

        # Identify object reference patterns
        # Example: /api/orders/{id} might reference /api/users/{user_id}

        for endpoint in self.graph.endpoints:
            # Check if endpoint path suggests a resource relationship
            # e.g., /api/orders/{id} and /api/users/{id} share ID pattern

            if '{id}' in endpoint.path:
                # Find other endpoints with {id}
                for other in self.graph.endpoints:
                    if other.path != endpoint.path and '{id}' in other.path:
                        # Potential shared namespace
                        self.graph.relationships.append(Relationship(
                            type="shared_namespace",
                            from_entity=endpoint.path,
                            to_entity=other.path,
                            evidence="Both use {id} parameter",
                            hypothesis=f"Test if {endpoint.path} ID can access {other.path} resources"
                        ))

    def identify_id_patterns(self):
        """Analyze ID patterns across endpoints"""
        print("[*] Identifying ID patterns...", file=sys.stderr)

        # Track observed IDs to identify patterns
        # In real implementation, this would analyze actual responses

        for endpoint in self.graph.endpoints:
            for param in endpoint.parameters.get("path", []):
                if param.name in ["id", "user_id", "order_id", "payment_id"]:
                    # Mark as numeric, assume sequential until proven otherwise
                    param.type = "numeric"
                    param.pattern = "sequential"
                    param.sample_values = ["1234", "1235", "1236"]  # Example
                    param.observed_range = [1000, 9999]

    def enrich_graph(self):
        """Add metadata and computed insights"""
        print("[*] Phase 4: Enriching graph...", file=sys.stderr)

        self.graph.metadata = {
            "discovery_date": "2026-06-21",
            "endpoint_count": len(self.graph.endpoints),
            "relationship_count": len(self.graph.relationships),
            "role_count": len(self.graph.roles),
            "technologies_identified": len(self.graph.technologies)
        }

        # Add observations based on patterns
        for endpoint in self.graph.endpoints:
            # Check for common security issues
            if not endpoint.auth_required:
                endpoint.observations.append("No authentication required - test for unauthorized access")

            if '{id}' in endpoint.path:
                endpoint.observations.append("ID parameter present - test for IDOR")

            if endpoint.rate_limiting == "unknown":
                endpoint.observations.append("Rate limiting not confirmed - test for abuse")

    def add_endpoint(self, path: str, methods: List[str], source: str):
        """Add endpoint to graph if not already present"""
        # Check if already exists
        for endpoint in self.graph.endpoints:
            if endpoint.path == path:
                # Update methods
                for method in methods:
                    if method not in endpoint.methods:
                        endpoint.methods.append(method)
                return

        # Add new endpoint
        endpoint = Endpoint(
            path=path,
            methods=methods,
            auth_required=True  # Assume required until proven otherwise
        )
        endpoint.observations.append(f"Discovered via {source}")

        self.graph.endpoints.append(endpoint)

    def to_json(self) -> str:
        """Convert graph to JSON"""
        # Custom serializer for dataclasses
        def serialize(obj):
            if hasattr(obj, '__dataclass_fields__'):
                return asdict(obj)
            return obj

        graph_dict = asdict(self.graph)
        return json.dumps(graph_dict, indent=2, default=serialize)


def main():
    parser = argparse.ArgumentParser(description="Build asset graph from web application")
    parser.add_argument("target", help="Target URL (e.g., https://api.example.com)")
    parser.add_argument("--passive-only", action="store_true", help="Passive discovery only (no active probing)")
    parser.add_argument("--output", "-o", help="Output file (default: stdout)")
    parser.add_argument("--scope", nargs="+", help="Additional URLs in scope")

    args = parser.parse_args()

    # Build graph
    mapper = AssetMapper(args.target, scope=args.scope)
    graph = mapper.build_graph(passive_only=args.passive_only)

    # Output
    json_output = mapper.to_json()

    if args.output:
        with open(args.output, 'w') as f:
            f.write(json_output)
        print(f"[+] Asset graph saved to {args.output}", file=sys.stderr)
    else:
        print(json_output)


if __name__ == "__main__":
    main()
