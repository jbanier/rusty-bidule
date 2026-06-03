#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import sys
from urllib.parse import urlparse, parse_qs
from collections import defaultdict

# Import shared utilities from web_assessment_common
SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import (
    json_dump,
    main_wrapper,
    normalize_host,
    require_url_in_scope,
    scope_from_args,
    tool_status
)


def extract_endpoints_from_urls(urls: list[str], target_domain: str) -> list[dict]:
    """
    Extract unique endpoints and parameters from URL list.

    Returns list of endpoint dicts with path, parameters, and metadata.
    """
    endpoints = {}

    for url in urls:
        try:
            parsed = urlparse(url)

            # Skip if not target domain
            if target_domain not in parsed.hostname:
                continue

            # Extract path (without query)
            path = parsed.path or "/"

            # Extract parameters
            params = list(parse_qs(parsed.query).keys())

            # Categorize by file extension
            extension = ""
            if "." in path:
                extension = path.rsplit(".", 1)[-1].lower()

            # Create unique key
            key = f"{path}?{'&'.join(sorted(params))}" if params else path

            if key not in endpoints:
                endpoints[key] = {
                    "path": path,
                    "parameters": params,
                    "extension": extension,
                    "depth": path.count("/"),
                    "interesting": is_interesting_endpoint(path, params, extension)
                }
        except Exception:
            continue

    return sorted(endpoints.values(), key=lambda x: (-x["interesting"], -x["depth"], x["path"]))


def is_interesting_endpoint(path: str, params: list[str], extension: str) -> int:
    """
    Score endpoint interest level (higher = more interesting).

    Prioritizes admin panels, debug endpoints, APIs, and unusual parameters.
    """
    score = 0

    path_lower = path.lower()

    # High-interest paths
    if any(keyword in path_lower for keyword in ["admin", "debug", "test", "backup", "old", "dev", "staging"]):
        score += 10

    # API endpoints
    if "api" in path_lower or "/v1/" in path_lower or "/v2/" in path_lower:
        score += 8

    # Configuration/sensitive files
    if extension in ["config", "conf", "xml", "json", "env", "bak", "sql", "log"]:
        score += 15

    # Interesting parameters
    interesting_params = ["debug", "admin", "test", "dev", "key", "token", "secret", "password", "user", "id"]
    for param in params:
        if any(keyword in param.lower() for keyword in interesting_params):
            score += 5

    # Has parameters at all
    if params:
        score += 2

    return score


def categorize_endpoints(endpoints: list[dict]) -> dict:
    """Group endpoints by type for easier analysis."""
    categories = defaultdict(list)

    for endpoint in endpoints:
        if endpoint["interesting"] >= 10:
            categories["high_interest"].append(endpoint)
        elif endpoint["interesting"] >= 5:
            categories["medium_interest"].append(endpoint)
        else:
            categories["standard"].append(endpoint)

        if endpoint["extension"]:
            categories[f"extension_{endpoint['extension']}"].append(endpoint)

    return dict(categories)


def build_gau_command(domain: str) -> list[str]:
    """Build gau (Get All URLs) command with safe defaults."""
    return [
        "gau",
        "--subs",  # Include subdomains
        "--threads", "5",
        domain
    ]


def build_waybackurls_command(domain: str) -> list[str]:
    """Build waybackurls command."""
    return [
        "waybackurls",
        domain
    ]


def build_hakrawler_command(url: str) -> list[str]:
    """Build hakrawler command for endpoint discovery."""
    return [
        "hakrawler",
        "-url", url,
        "-depth", "3",
        "-plain"
    ]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target-url", required=True, help="Target URL or domain")
    parser.add_argument("--scope-json")
    parser.add_argument("--allowed-hosts", default="")
    parser.add_argument("--active-authorized", default="false")
    args = parser.parse_args()

    # Validate scope
    scope = scope_from_args(
        scope_json=args.scope_json,
        target_urls=args.target_url,
        allowed_hosts=args.allowed_hosts,
        active_authorized=args.active_authorized,
    )
    target_url = require_url_in_scope(args.target_url, scope, active=True)

    # Extract domain from target URL
    target_domain = normalize_host(target_url)

    # Check tool availability
    tools = tool_status(["gau", "waybackurls", "hakrawler"])

    # Build commands
    commands = []

    if tools.get("gau", {}).get("available"):
        commands.append({
            "tool": "gau",
            "phase": "historical_discovery",
            "argv": build_gau_command(target_domain),
            "description": "Fetch URLs from Wayback Machine, Common Crawl, and other sources"
        })

    if tools.get("waybackurls", {}).get("available"):
        commands.append({
            "tool": "waybackurls",
            "phase": "historical_discovery",
            "argv": build_waybackurls_command(target_domain),
            "description": "Fetch URLs from Wayback Machine CDX API"
        })

    if tools.get("hakrawler", {}).get("available"):
        commands.append({
            "tool": "hakrawler",
            "phase": "endpoint_crawl",
            "argv": build_hakrawler_command(target_url),
            "description": "Crawl target for JavaScript endpoints and URLs"
        })

    # Note: Actual URL fetching would happen via local__exec_cli in recipes
    # This script provides command planning only

    json_dump({
        "status": "ok",
        "target_domain": target_domain,
        "target_url": target_url,
        "scope": scope,
        "tool_availability": tools,
        "commands": commands,
        "execution_policy": "Execute commands via local__exec_cli with managed_job mode. Parse output to extract endpoints. Tools may return large result sets (1000+ URLs).",
        "safety_constraints": {
            "threads": "5 max for gau",
            "depth": "3 for hakrawler",
            "note": "Historical URLs should be validated for current accessibility before testing"
        },
        "analysis_guidance": {
            "interesting_indicators": [
                "admin, debug, test paths",
                "api endpoints with version numbers",
                "config, backup, or log files",
                "parameters: debug, admin, key, token, id"
            ],
            "recommended_workflow": [
                "1. Run gau or waybackurls to fetch historical URLs",
                "2. Extract unique endpoints with extract_endpoints_from_urls()",
                "3. Prioritize by is_interesting_endpoint() score",
                "4. Test high-interest endpoints first",
                "5. Check if deprecated endpoints still respond"
            ]
        }
    })


if __name__ == "__main__":
    main_wrapper(main)
