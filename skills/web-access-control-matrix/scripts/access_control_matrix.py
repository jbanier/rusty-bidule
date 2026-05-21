#!/usr/bin/env python3
from __future__ import annotations

import argparse
from collections import defaultdict
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import json_dump, main_wrapper, parse_json_arg  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--observations-json", required=True, help="Array of {role, method, path, object_id, status, expected}")
    args = parser.parse_args()
    observations = parse_json_arg(args.observations_json, [])

    groups: dict[tuple[str, str, str], list[dict[str, object]]] = defaultdict(list)
    for item in observations:
        groups[(str(item.get("method", "GET")).upper(), str(item.get("path", "")), str(item.get("object_id", "")))].append(item)

    findings = []
    for key, items in groups.items():
        statuses = {str(item.get("role")): int(item.get("status", 0) or 0) for item in items}
        unexpected_success = [
            item for item in items if bool(item.get("expected")) is False and int(item.get("status", 0) or 0) < 400
        ]
        if unexpected_success:
            findings.append({"endpoint": key, "type": "unexpected_access_success", "roles": [item.get("role") for item in unexpected_success], "statuses": statuses})

    json_dump(
        {
            "status": "ok",
            "observation_count": len(observations),
            "groups": len(groups),
            "findings": findings,
            "required_followup": [
                "Reproduce unexpected successes manually within scope.",
                "Confirm object ownership and intended authorization model.",
                "Collect request/response evidence for each affected role.",
            ],
        }
    )


if __name__ == "__main__":
    main_wrapper(main)
