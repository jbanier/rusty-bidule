#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import fetch_url, json_dump, main_wrapper, require_url_in_scope, resolve_scoped_path, scope_from_args, truthy  # noqa: E402


def load_openapi(args: argparse.Namespace, scope: dict[str, object]) -> dict[str, object] | None:
    if args.openapi_path:
        path = resolve_scoped_path(args.openapi_path)
        return json.loads(path.read_text())
    if args.openapi_url:
        url = require_url_in_scope(args.openapi_url, scope, active=truthy(args.fetch))
        if not truthy(args.fetch):
            return {"spec_url": url, "fetch_required": True}
        response = fetch_url(url, max_bytes=1_000_000)
        return json.loads(response.get("body_preview") or "{}")
    return None


def summarize_openapi(spec: dict[str, object] | None) -> dict[str, object]:
    if not spec:
        return {"present": False}
    paths = spec.get("paths") if isinstance(spec.get("paths"), dict) else {}
    operations = []
    for path, methods in paths.items():
        if not isinstance(methods, dict):
            continue
        for method, operation in methods.items():
            if method.lower() not in {"get", "post", "put", "patch", "delete", "head", "options"}:
                continue
            operations.append(
                {
                    "method": method.upper(),
                    "path": path,
                    "operation_id": operation.get("operationId") if isinstance(operation, dict) else None,
                    "has_security": bool(operation.get("security")) if isinstance(operation, dict) else False,
                }
            )
    return {"present": True, "operation_count": len(operations), "operations": operations[:200]}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target-url", required=True)
    parser.add_argument("--scope-json")
    parser.add_argument("--allowed-hosts", default="")
    parser.add_argument("--active-authorized", default="false")
    parser.add_argument("--openapi-path", default="")
    parser.add_argument("--openapi-url", default="")
    parser.add_argument("--graphql-endpoint", default="")
    parser.add_argument("--fetch", default="false")
    args = parser.parse_args()

    scope = scope_from_args(
        scope_json=args.scope_json,
        target_urls=",".join(filter(None, [args.target_url, args.openapi_url, args.graphql_endpoint])),
        allowed_hosts=args.allowed_hosts,
        active_authorized=args.active_authorized,
    )
    require_url_in_scope(args.target_url, scope)
    if args.graphql_endpoint:
        require_url_in_scope(args.graphql_endpoint, scope)
    spec = load_openapi(args, scope)
    json_dump(
        {
            "status": "ok",
            "scope": scope,
            "openapi": summarize_openapi(spec),
            "graphql_endpoint": args.graphql_endpoint or None,
            "graphql_checklist": [
                "Confirm introspection policy is intentional.",
                "Check authentication and object authorization on queries and mutations.",
                "Review batching, aliasing, nesting/depth, and rate limits.",
                "Check error messages for schema or implementation leakage.",
            ],
            "api_checklist": [
                "Map unauthenticated vs authenticated endpoints.",
                "Compare object access across roles.",
                "Review unsafe methods and state-changing routes for CSRF/auth controls.",
                "Check documented vs discovered endpoints for drift.",
            ],
        }
    )


if __name__ == "__main__":
    main_wrapper(main)
