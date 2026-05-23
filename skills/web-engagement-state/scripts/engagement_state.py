#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import json_dump, main_wrapper, parse_json_arg, split_items, scope_from_args  # noqa: E402


def normalize_endpoints(raw: object) -> list[dict[str, object]]:
    endpoints = raw if isinstance(raw, list) else []
    out: list[dict[str, object]] = []
    seen: set[tuple[str, str]] = set()
    for item in endpoints:
        if not isinstance(item, dict):
            continue
        method = str(item.get("method") or "GET").upper()
        path = str(item.get("path") or item.get("url") or "").strip()
        if not path:
            continue
        key = (method, path)
        if key in seen:
            continue
        seen.add(key)
        out.append(
            {
                "method": method,
                "path": path,
                "source": item.get("source") or "operator",
                "auth_required": bool(item.get("auth_required", False)),
                "parameters": split_items(item.get("parameters") if isinstance(item.get("parameters"), str) else ",".join(item.get("parameters", [])) if isinstance(item.get("parameters"), list) else ""),
            }
        )
    return out


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--scope-json")
    parser.add_argument("--target-urls", default="")
    parser.add_argument("--allowed-hosts", default="")
    parser.add_argument("--active-authorized", default="false")
    parser.add_argument("--destructive-authorized", default="false")
    parser.add_argument("--oob-authorized", default="false")
    parser.add_argument("--rate-limit-per-minute", type=int)
    parser.add_argument("--excluded-tests", default="")
    parser.add_argument("--endpoints-json", default="[]")
    parser.add_argument("--skipped-checks-json", default="[]")
    parser.add_argument("--unresolved-approvals", default="")
    args = parser.parse_args()

    scope = scope_from_args(
        scope_json=args.scope_json,
        target_urls=args.target_urls,
        allowed_hosts=args.allowed_hosts,
        active_authorized=args.active_authorized,
        destructive_authorized=args.destructive_authorized,
        oob_authorized=args.oob_authorized,
        rate_limit_per_minute=args.rate_limit_per_minute,
        excluded_tests=args.excluded_tests,
    )
    endpoints = normalize_endpoints(parse_json_arg(args.endpoints_json, []))
    skipped = parse_json_arg(args.skipped_checks_json, [])
    if not isinstance(skipped, list):
        skipped = []

    json_dump(
        {
            "status": "ok",
            "engagement_state": {
                "scope": scope,
                "endpoint_inventory": endpoints,
                "endpoint_count": len(endpoints),
                "skipped_checks": skipped,
                "unresolved_approvals": split_items(args.unresolved_approvals),
                "coverage_policy": "Treat untested or skipped areas as gaps, not as clean results.",
            },
            "investigation_memory_patch": {
                "summary": "Authorized web assessment engagement state updated.",
                "entities": [{"type": "web_engagement_state", "value": {"scope": scope, "endpoint_count": len(endpoints)}}],
                "decisions": [{"decision": "Use normalized engagement state before active web testing.", "allowed_hosts": scope.get("allowed_hosts", [])}],
                "unresolved_questions": split_items(args.unresolved_approvals),
            },
        }
    )


if __name__ == "__main__":
    main_wrapper(main)

