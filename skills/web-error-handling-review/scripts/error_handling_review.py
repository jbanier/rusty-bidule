#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys
import urllib.parse

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import json_dump, main_wrapper, parse_json_arg, require_url_in_scope, scope_from_args, split_items  # noqa: E402


PATTERNS = [
    ("stack-trace-python", re.compile(r"Traceback \(most recent call last\)|File \"[^\"]+\", line \d+", re.IGNORECASE), "medium", "Python stack trace disclosure."),
    ("stack-trace-java", re.compile(r"\bat [A-Za-z0-9_.$]+\(.*\.java:\d+\)|java\.lang\.[A-Za-z]+Exception", re.IGNORECASE), "medium", "Java stack trace disclosure."),
    ("stack-trace-dotnet", re.compile(r"System\.[A-Za-z.]+Exception| at [A-Za-z0-9_.<>]+\(.*\) in [A-Za-z]:\\", re.IGNORECASE), "medium", ".NET exception or local path disclosure."),
    ("stack-trace-node", re.compile(r"\bat .+\((?:/|[A-Za-z]:\\).+:\d+:\d+\)|(?:TypeError|ReferenceError|SyntaxError):", re.IGNORECASE), "medium", "Node.js exception or stack trace disclosure."),
    ("sql-error", re.compile(r"SQL syntax|mysql_fetch|ORA-\d{5}|PostgreSQL.*ERROR|SQLite/JDBC|ODBC Driver|unterminated quoted string", re.IGNORECASE), "medium", "Database error disclosure."),
    ("debug-mode", re.compile(r"DEBUG\s*=\s*True|Werkzeug Debugger|Django Debug|Laravel.*Whoops|Rails.*development|Express error handler", re.IGNORECASE), "high", "Framework debug mode or debug error page."),
    ("path-disclosure", re.compile(r"(?:/var/www|/home/[A-Za-z0-9_.-]+|C:\\\\inetpub|C:\\\\Users\\\\|/srv/www|/app/[A-Za-z0-9_.-]+)", re.IGNORECASE), "low", "Server filesystem path disclosure."),
    ("verbose-status-500", re.compile(r"\b(?:500 Internal Server Error|HTTP/1\.[01] 500|status[\"']?\s*:\s*500)\b", re.IGNORECASE), "info", "Server error response observed."),
]


def text_fragments(value: object, source: str = "input") -> list[tuple[str, str]]:
    fragments: list[tuple[str, str]] = []
    if isinstance(value, dict):
        for key, item in value.items():
            child_source = f"{source}.{key}"
            if isinstance(item, str) and key.lower() in {"body", "response", "error", "message", "title", "stack", "trace", "html", "text"}:
                fragments.append((child_source, item))
            else:
                fragments.extend(text_fragments(item, child_source))
    elif isinstance(value, list):
        for index, item in enumerate(value):
            fragments.extend(text_fragments(item, f"{source}[{index}]"))
    elif isinstance(value, str):
        fragments.append((source, value))
    return fragments


def flatten_routes(value: object) -> list[str]:
    routes: list[str] = []
    if isinstance(value, str):
        routes.append(value)
    elif isinstance(value, dict):
        for key in ["routes", "api_paths", "client_routes", "api_candidates", "websocket_urls", "shadow_api_candidates"]:
            item = value.get(key)
            if isinstance(item, list):
                for entry in item:
                    if isinstance(entry, str):
                        routes.append(entry)
                    elif isinstance(entry, dict):
                        candidate = entry.get("candidate") or entry.get("url") or entry.get("path")
                        if candidate:
                            routes.append(str(candidate))
        for item in value.values():
            if isinstance(item, (dict, list)):
                routes.extend(flatten_routes(item))
    elif isinstance(value, list):
        for item in value:
            routes.extend(flatten_routes(item))
    return routes


def validate_url_or_path(value: str, scope: dict[str, object]) -> str:
    value = value.strip()
    if not value:
        return ""
    parsed = urllib.parse.urlparse(value)
    if parsed.scheme in {"http", "https", "ws", "wss"}:
        return require_url_in_scope(value, scope)
    return value


def priority_reason(value: str) -> str:
    lowered = value.lower()
    reasons = []
    if "?" in value or re.search(r"/\{?[A-Za-z0-9_.-]*(?:id|uuid|slug|name)\}?", lowered):
        reasons.append("parameterized or object-specific route")
    if any(marker in lowered for marker in ["/api/", "/graphql", "/v1/", "/v2/", "/admin", "/upload"]):
        reasons.append("API or high-value application route")
    if not reasons:
        reasons.append("client-discovered route")
    return "; ".join(reasons)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--observations-json", default="[]")
    parser.add_argument("--routes-json", default="[]")
    parser.add_argument("--route-inventory-json", default="{}")
    parser.add_argument("--urls", default="")
    parser.add_argument("--scope-json")
    parser.add_argument("--target-urls", default="")
    parser.add_argument("--allowed-hosts", default="")
    args = parser.parse_args()

    scope = scope_from_args(scope_json=args.scope_json, target_urls=args.target_urls, allowed_hosts=args.allowed_hosts)
    observations = parse_json_arg(args.observations_json, [])
    routes_json = parse_json_arg(args.routes_json, [])
    route_inventory = parse_json_arg(args.route_inventory_json, {})

    findings = []
    for source, fragment in text_fragments(observations):
        for finding_type, pattern, severity, description in PATTERNS:
            match = pattern.search(fragment)
            if match:
                snippet = fragment[max(match.start() - 80, 0) : min(match.end() + 160, len(fragment))]
                findings.append(
                    {
                        "type": finding_type,
                        "severity": severity,
                        "source": source,
                        "description": description,
                        "evidence_snippet": snippet.replace("\n", " ")[:320],
                    }
                )

    routes = split_items(args.urls)
    routes.extend(flatten_routes(routes_json))
    routes.extend(flatten_routes(route_inventory))
    validated_routes = []
    for route in routes:
        validated = validate_url_or_path(route, scope)
        if validated and validated not in validated_routes:
            validated_routes.append(validated)

    prioritized = [
        {
            "target": route,
            "reason": priority_reason(route),
            "expected_evidence": [
                "status code and response body for normal application error handling",
                "absence of stack traces, framework debug pages, local paths, database errors, and secrets",
                "consistent generic error response across anonymous and authenticated roles where applicable",
            ],
        }
        for route in validated_routes[:100]
        if any(marker in route.lower() for marker in ["?", "/api/", "/graphql", "/v1/", "/v2/", "/admin", "/upload", "{"])
    ]

    safe_prompts = [
        "Use normal application workflows and already-observed invalid states before considering any active test.",
        "If active authorization exists, validate error behavior with low-impact malformed requests only against scoped targets.",
        "Do not fuzz, brute force, trigger expensive exceptions, or include exploit payloads from this review.",
    ]

    json_dump(
        {
            "status": "ok",
            "findings": findings[:200],
            "finding_count": len(findings),
            "prioritized_validation_targets": prioritized,
            "safe_validation_prompts": safe_prompts,
            "expected_evidence": {
                "reportable": "request/response evidence showing sensitive error disclosure and security impact",
                "lead_only": "client route or generic 500 without sensitive details",
            },
            "policy": "Passive error-handling review only. Generated validation targets are not permission to actively test.",
        }
    )


if __name__ == "__main__":
    main_wrapper(main)
