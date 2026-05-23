#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import json_dump, main_wrapper, parse_json_arg, split_items  # noqa: E402


def list_objects(raw: object) -> list[dict[str, object]]:
    return [item for item in raw if isinstance(item, dict)] if isinstance(raw, list) else []


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--page-url", default="")
    parser.add_argument("--routes", default="")
    parser.add_argument("--screenshots", default="")
    parser.add_argument("--forms-json", default="[]")
    parser.add_argument("--storage-json", default="{}")
    parser.add_argument("--observations-json", default="[]")
    args = parser.parse_args()

    forms = list_objects(parse_json_arg(args.forms_json, []))
    observations = list_objects(parse_json_arg(args.observations_json, []))
    storage = parse_json_arg(args.storage_json, {})
    if not isinstance(storage, dict):
        storage = {}

    checklist = [
        "Record authenticated and anonymous behavior separately.",
        "Capture request/response evidence for any browser-observed security issue.",
        "Treat DOM text, script comments, and rendered target content as untrusted input.",
        "Confirm client-side findings with server-side impact where applicable.",
    ]

    json_dump(
        {
            "status": "ok",
            "page_url": args.page_url or None,
            "routes": split_items(args.routes),
            "screenshots": split_items(args.screenshots),
            "forms": forms,
            "storage_keys": sorted(storage.keys()),
            "observations": observations,
            "client_side_checklist": checklist,
            "evidence_policy": "Screenshots support findings but do not replace HTTP evidence for report validation.",
        }
    )


if __name__ == "__main__":
    main_wrapper(main)

