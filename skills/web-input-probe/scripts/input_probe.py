#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import ScopeError, json_dump, main_wrapper, split_items, truthy  # noqa: E402

PORTSWIGGER_CATEGORIES = [
    "SQL injection",
    "Cross-site scripting",
    "DOM-based vulnerabilities",
    "Cross-origin resource sharing",
    "XXE",
    "SSRF",
    "HTTP request smuggling",
    "OS command injection",
    "Server-side template injection",
    "Path traversal",
    "NoSQL injection",
    "Prototype pollution",
]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--parameters", default="")
    parser.add_argument("--contexts", default="query,form,json,cookie,header")
    parser.add_argument("--include-oob", default="false")
    parser.add_argument("--oob-authorized", default="false")
    args = parser.parse_args()
    include_oob = truthy(args.include_oob)
    if include_oob and not truthy(args.oob_authorized):
        raise ScopeError("OOB probe planning requires oob_authorized=true")
    json_dump(
        {
            "status": "ok",
            "parameters": split_items(args.parameters),
            "contexts": split_items(args.contexts),
            "categories": PORTSWIGGER_CATEGORIES,
            "probe_policy": "Use benign/manual probes first. Do not run destructive commands, data extraction, or WAF evasion without explicit authorization.",
            "evidence_to_collect": [
                "Original request and response baseline.",
                "Input location and server-side behavior change.",
                "Error, timing, reflection, callback, or authorization evidence.",
                "Impact statement and safe reproduction steps.",
            ],
            "oob_allowed": include_oob,
        }
    )


if __name__ == "__main__":
    main_wrapper(main)
