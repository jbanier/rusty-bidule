#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import json_dump, main_wrapper, require_url_in_scope, scope_from_args, tool_status  # noqa: E402

SAFE_EXCLUDE_TAGS = ["dos", "bruteforce", "intrusive", "destructive", "fuzz", "rce"]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target-url", required=True)
    parser.add_argument("--scope-json")
    parser.add_argument("--allowed-hosts", default="")
    parser.add_argument("--active-authorized", default="false")
    parser.add_argument("--rate-limit", type=int, default=30)
    parser.add_argument("--severity", default="low,medium,high,critical")
    args = parser.parse_args()

    scope = scope_from_args(
        scope_json=args.scope_json,
        target_urls=args.target_url,
        allowed_hosts=args.allowed_hosts,
        active_authorized=args.active_authorized,
    )
    target_url = require_url_in_scope(args.target_url, scope)
    json_dump(
        {
            "status": "ok",
            "target_url": target_url,
            "scope": scope,
            "tool_availability": tool_status(["nuclei", "zap-baseline.py"]),
            "commands": [
                {
                    "tool": "nuclei",
                    "argv": [
                        "nuclei",
                        "-u",
                        target_url,
                        "-severity",
                        args.severity,
                        "-exclude-tags",
                        ",".join(SAFE_EXCLUDE_TAGS),
                        "-rate-limit",
                        str(args.rate_limit),
                    ],
                },
                {
                    "tool": "zap-baseline.py",
                    "argv": ["zap-baseline.py", "-t", target_url, "-m", "5"],
                },
            ],
            "execution_policy": "Plan only. Scanner execution requires active_authorized=true, scoped target, rate limit, and analyst review of templates.",
        }
    )


if __name__ == "__main__":
    main_wrapper(main)
