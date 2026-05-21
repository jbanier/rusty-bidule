#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import json_dump, main_wrapper, require_url_in_scope, scope_from_args, tool_status  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--websocket-url", required=True)
    parser.add_argument("--scope-json")
    parser.add_argument("--allowed-hosts", default="")
    parser.add_argument("--active-authorized", default="false")
    parser.add_argument("--headers", default="")
    args = parser.parse_args()
    scope = scope_from_args(
        scope_json=args.scope_json,
        target_urls=args.websocket_url,
        allowed_hosts=args.allowed_hosts,
        active_authorized=args.active_authorized,
    )
    websocket_url = require_url_in_scope(args.websocket_url, scope)
    json_dump(
        {
            "status": "ok",
            "websocket_url": websocket_url,
            "scope": scope,
            "tool_availability": tool_status(["wscat", "websocat"]),
            "commands": [
                ["wscat", "-c", websocket_url],
                ["websocat", websocket_url],
            ],
            "checklist": [
                "Confirm WebSocket handshake requires expected authentication.",
                "Check Origin handling and cross-site WebSocket hijacking exposure.",
                "Replay representative messages across roles and sessions.",
                "Tamper object IDs and action fields within authorized test accounts.",
                "Confirm server-side authorization is enforced per message, not only at connection time.",
            ],
            "headers_note": args.headers,
        }
    )


if __name__ == "__main__":
    main_wrapper(main)
