#!/usr/bin/env python3
from __future__ import annotations

import argparse
from collections import Counter
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import json_dump, main_wrapper, parse_json_arg  # noqa: E402


WSTG_CORE = [
    "WSTG-INFO",
    "WSTG-CONF",
    "WSTG-IDNT",
    "WSTG-ATHN",
    "WSTG-ATHZ",
    "WSTG-SESS",
    "WSTG-INPV",
    "WSTG-ERRH",
    "WSTG-CRYP",
    "WSTG-BUSL",
    "WSTG-CLNT",
    "WSTG-APIT",
]

API_TOP10_2023 = [f"API{i}" for i in range(1, 11)]


def entries(raw: object) -> list[dict[str, object]]:
    return [item for item in raw if isinstance(item, dict)] if isinstance(raw, list) else []


def values_for(item: dict[str, object], key: str) -> list[str]:
    value = item.get(key)
    if isinstance(value, list):
        return [str(entry).strip() for entry in value if str(entry).strip()]
    if isinstance(value, str):
        return [entry.strip() for entry in value.replace("\n", ",").split(",") if entry.strip()]
    return []


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--coverage-json", default="[]")
    parser.add_argument("--findings-json", default="[]")
    parser.add_argument("--skipped-checks-json", default="[]")
    args = parser.parse_args()

    coverage = entries(parse_json_arg(args.coverage_json, []))
    findings = entries(parse_json_arg(args.findings_json, []))
    skipped = entries(parse_json_arg(args.skipped_checks_json, []))

    tested_wstg = Counter()
    tested_api = Counter()
    for item in coverage + findings:
        for value in values_for(item, "wstg_ids"):
            tested_wstg[value.split("-")[0] + "-" + value.split("-")[1] if value.startswith("WSTG-") and len(value.split("-")) > 1 else value] += 1
        for value in values_for(item, "api_top10_ids"):
            tested_api[value.upper()] += 1

    skipped_names = [str(item.get("check") or item.get("id") or "").strip() for item in skipped]
    skipped_names = [name for name in skipped_names if name]

    json_dump(
        {
            "status": "ok",
            "coverage": {
                "entries": len(coverage),
                "findings": len(findings),
                "tested_wstg_categories": sorted(tested_wstg),
                "tested_api_top10_categories": sorted(tested_api),
                "wstg_gaps": [item for item in WSTG_CORE if item not in tested_wstg],
                "api_top10_gaps": [item for item in API_TOP10_2023 if item not in tested_api],
                "skipped_checks": skipped,
                "skipped_check_count": len(skipped_names),
            },
            "policy": "Coverage gaps and skipped checks must stay visible in summaries and reports.",
        }
    )


if __name__ == "__main__":
    main_wrapper(main)

