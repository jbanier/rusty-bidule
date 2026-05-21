#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import json_dump, main_wrapper, scope_from_args, split_items  # noqa: E402


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
    parser.add_argument("--credentials-notes", default="")
    parser.add_argument("--blackout-windows", default="")
    parser.add_argument("--reporting-requirements", default="")
    parser.add_argument("--notes", default="")
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
    scope.update(
        {
            "credentials_notes": args.credentials_notes,
            "blackout_windows": split_items(args.blackout_windows),
            "reporting_requirements": args.reporting_requirements,
            "notes": args.notes,
        }
    )
    warnings = []
    if not scope["target_urls"]:
        warnings.append("No target URLs were provided.")
    if not scope["active_authorized"]:
        warnings.append("Active network testing is disabled until active_authorized=true.")
    if scope["destructive_authorized"]:
        warnings.append("Destructive testing is enabled; recipes must still avoid DoS unless explicitly requested.")

    json_dump(
        {
            "status": "ok",
            "scope": scope,
            "warnings": warnings,
            "investigation_memory_patch": {
                "summary": "Authorized web application posture assessment scope captured.",
                "entities": [{"type": "web_assessment_scope", "value": scope}],
                "decisions": [
                    {
                        "decision": "Use validated scope before active web assessment tools.",
                        "allowed_hosts": scope["allowed_hosts"],
                    }
                ],
                "unresolved_questions": warnings,
            },
        }
    )


if __name__ == "__main__":
    main_wrapper(main)
