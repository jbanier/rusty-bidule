#!/usr/bin/env python3
from __future__ import annotations

import argparse
from http.cookies import SimpleCookie
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import fetch_url, json_dump, main_wrapper, require_url_in_scope, scope_from_args, truthy  # noqa: E402

SECURITY_HEADERS = [
    "strict-transport-security",
    "content-security-policy",
    "x-content-type-options",
    "x-frame-options",
    "referrer-policy",
    "permissions-policy",
    "cache-control",
]


def cookie_findings(headers: dict[str, str]) -> list[dict[str, str]]:
    values = [value for key, value in headers.items() if key.lower() == "set-cookie"]
    findings: list[dict[str, str]] = []
    for value in values:
        cookie = SimpleCookie()
        cookie.load(value)
        for name, morsel in cookie.items():
            missing = []
            for attr in ["secure", "httponly", "samesite"]:
                if not morsel[attr]:
                    missing.append(attr)
            findings.append({"name": name, "missing_attributes": ", ".join(missing) or "none"})
    return findings


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target-url", required=True)
    parser.add_argument("--scope-json")
    parser.add_argument("--allowed-hosts", default="")
    parser.add_argument("--active-authorized", default="false")
    parser.add_argument("--fetch", default="false")
    parser.add_argument("--method", default="GET")
    parser.add_argument("--timeout", type=int, default=15)
    args = parser.parse_args()

    scope = scope_from_args(
        scope_json=args.scope_json,
        target_urls=args.target_url,
        allowed_hosts=args.allowed_hosts,
        active_authorized=args.active_authorized,
    )
    target_url = require_url_in_scope(args.target_url, scope, active=truthy(args.fetch))
    payload = {"status": "ok", "target_url": target_url, "scope": scope, "fetch_performed": False}
    if truthy(args.fetch):
        response = fetch_url(target_url, timeout=args.timeout, method=args.method)
        headers = {key.lower(): value for key, value in response.get("headers", {}).items()}
        payload.update(
            {
                "fetch_performed": True,
                "response": response,
                "security_headers": {
                    header: {"present": header in headers, "value": headers.get(header)}
                    for header in SECURITY_HEADERS
                },
                "cookies": cookie_findings(response.get("headers", {})),
                "cors": {
                    "access_control_allow_origin": headers.get("access-control-allow-origin"),
                    "access_control_allow_credentials": headers.get("access-control-allow-credentials"),
                },
            }
        )
    else:
        payload["next_step"] = "Set fetch=true with active_authorized=true to collect live HTTP evidence."
    json_dump(payload)


if __name__ == "__main__":
    main_wrapper(main)
