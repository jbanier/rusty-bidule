#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import json_dump, main_wrapper, split_items  # noqa: E402


CATALOG = {
    "sqli": [
        {"family": "syntax and error probes", "safety": "active-safe", "wstg_ids": ["WSTG-INPV-05"]},
        {"family": "boolean differential probes", "safety": "active-safe", "wstg_ids": ["WSTG-INPV-05"]},
        {"family": "time-based probes", "safety": "intrusive", "wstg_ids": ["WSTG-INPV-05"]},
    ],
    "xss": [
        {"family": "context reflection markers", "safety": "benign", "wstg_ids": ["WSTG-INPV-01"]},
        {"family": "HTML/attribute/script context probes", "safety": "active-safe", "wstg_ids": ["WSTG-INPV-01"]},
    ],
    "ssrf": [
        {"family": "loopback and metadata reachability checks", "safety": "intrusive", "wstg_ids": ["WSTG-INPV-19"]},
        {"family": "out-of-band callback checks", "safety": "oob", "wstg_ids": ["WSTG-INPV-19"]},
    ],
    "xxe": [
        {"family": "well-formed parser behavior probes", "safety": "active-safe", "wstg_ids": ["WSTG-INPV-07"]},
        {"family": "external entity callback checks", "safety": "oob", "wstg_ids": ["WSTG-INPV-07"]},
    ],
    "file-upload": [
        {"family": "extension, MIME, and magic-byte mismatch checks", "safety": "active-safe", "wstg_ids": ["WSTG-BUSL-09"]},
        {"family": "server-side execution checks", "safety": "destructive", "wstg_ids": ["WSTG-BUSL-09"]},
    ],
    "auth-access": [
        {"family": "role/object comparison matrices", "safety": "benign", "wstg_ids": ["WSTG-ATHZ"]},
        {"family": "JWT claim and algorithm review", "safety": "benign", "wstg_ids": ["WSTG-SESS"]},
    ],
    "ai-llm": [
        {"family": "instruction hierarchy and prompt-injection probes", "safety": "active-safe", "wstg_ids": ["WSTG-BUSL"]},
        {"family": "tool-use and retrieval boundary checks", "safety": "active-safe", "wstg_ids": ["WSTG-BUSL"]},
    ],
}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--categories", default="")
    args = parser.parse_args()

    requested = split_items(args.categories) or sorted(CATALOG)
    selected = {category: CATALOG[category] for category in requested if category in CATALOG}
    json_dump(
        {
            "status": "ok",
            "catalog": selected,
            "unknown_categories": [category for category in requested if category not in CATALOG],
            "safety_labels": ["benign", "active-safe", "intrusive", "oob", "destructive"],
            "references": [
                "OWASP Web Security Testing Guide",
                "PayloadAllTheThings and SecLists can be consulted by the operator when licensed/available.",
            ],
            "policy": "Catalog entries are references for manual authorized testing and must not be auto-run.",
        }
    )


if __name__ == "__main__":
    main_wrapper(main)

