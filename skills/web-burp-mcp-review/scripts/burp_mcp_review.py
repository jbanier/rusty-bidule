#!/usr/bin/env python3
from __future__ import annotations

import argparse
from collections import defaultdict
from pathlib import Path
import sys
import urllib.parse

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import json_dump, main_wrapper, parse_json_arg, require_url_in_scope, scope_from_args  # noqa: E402


def exchange_items(raw: object) -> list[dict[str, object]]:
    return [item for item in raw if isinstance(item, dict)] if isinstance(raw, list) else []


def params_from_url(url: str) -> list[str]:
    parsed = urllib.parse.urlparse(url)
    return sorted({key for key, _ in urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)})


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--scope-json")
    parser.add_argument("--target-url", default="")
    parser.add_argument("--allowed-hosts", default="")
    parser.add_argument("--exchanges-json", default="[]")
    args = parser.parse_args()

    exchanges = exchange_items(parse_json_arg(args.exchanges_json, []))
    targets = ",".join(filter(None, [args.target_url] + [str(item.get("url") or "") for item in exchanges]))
    scope = scope_from_args(scope_json=args.scope_json, target_urls=targets, allowed_hosts=args.allowed_hosts)

    endpoints: dict[tuple[str, str], dict[str, object]] = {}
    interesting: list[dict[str, object]] = []
    by_host: dict[str, int] = defaultdict(int)
    for item in exchanges:
        url = str(item.get("url") or "").strip()
        if not url:
            continue
        try:
            scoped_url = require_url_in_scope(url, scope)
        except SystemExit:
            interesting.append({"url": url, "reason": "out_of_scope_or_invalid"})
            continue
        parsed = urllib.parse.urlparse(scoped_url)
        by_host[parsed.hostname or "unknown"] += 1
        method = str(item.get("method") or "GET").upper()
        key = (method, parsed.path or "/")
        endpoint = endpoints.setdefault(
            key,
            {
                "method": method,
                "path": parsed.path or "/",
                "statuses": [],
                "parameters": set(),
                "evidence_artifacts": [],
            },
        )
        status = item.get("status")
        if status is not None and status not in endpoint["statuses"]:
            endpoint["statuses"].append(status)
        endpoint["parameters"].update(params_from_url(scoped_url))
        for param in item.get("parameters", []) if isinstance(item.get("parameters"), list) else []:
            endpoint["parameters"].add(str(param))
        artifact = item.get("artifact") or item.get("evidence_artifact")
        if artifact and artifact not in endpoint["evidence_artifacts"]:
            endpoint["evidence_artifacts"].append(artifact)
        if int(item.get("status", 0) or 0) >= 500:
            interesting.append({"url": scoped_url, "reason": "server_error", "status": item.get("status")})

    normalized = []
    for endpoint in endpoints.values():
        endpoint["parameters"] = sorted(endpoint["parameters"])
        normalized.append(endpoint)

    json_dump(
        {
            "status": "ok",
            "scope": scope,
            "exchange_count": len(exchanges),
            "scoped_endpoint_count": len(normalized),
            "hosts": dict(sorted(by_host.items())),
            "endpoints": sorted(normalized, key=lambda item: (str(item["path"]), str(item["method"]))),
            "validation_candidates": interesting,
            "policy": "Burp observations are leads until manually validated with scoped request and response evidence.",
        }
    )


if __name__ == "__main__":
    main_wrapper(main)

