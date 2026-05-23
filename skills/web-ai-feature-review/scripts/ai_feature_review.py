#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import ScopeError, json_dump, main_wrapper, parse_json_arg, split_items, truthy  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--features", default="")
    parser.add_argument("--tools", default="")
    parser.add_argument("--data-sources", default="")
    parser.add_argument("--observations-json", default="[]")
    parser.add_argument("--include-oob", default="false")
    parser.add_argument("--oob-authorized", default="false")
    args = parser.parse_args()

    if truthy(args.include_oob) and not truthy(args.oob_authorized):
        raise ScopeError("AI OOB callback checks require oob_authorized=true")
    observations = parse_json_arg(args.observations_json, [])
    if not isinstance(observations, list):
        observations = []

    json_dump(
        {
            "status": "ok",
            "features": split_items(args.features),
            "tools": split_items(args.tools),
            "data_sources": split_items(args.data_sources),
            "observations": observations,
            "checklist": [
                "Confirm the feature treats page content, uploaded files, retrieved documents, and tool output as untrusted input.",
                "Test direct prompt injection with benign instruction-conflict probes.",
                "Test indirect prompt injection through retrieved or user-controlled content when authorized.",
                "Verify tool calls are scoped, least-privileged, logged, and require confirmation for sensitive actions.",
                "Check for system prompt, connector secret, retrieval corpus, and cross-tenant data exposure.",
                "Confirm model output cannot trigger unsafe browser, MCP, or backend actions without policy checks.",
            ],
            "evidence_to_collect": [
                "Original user prompt and relevant retrieved content.",
                "Model/tool response with secrets redacted.",
                "Tool call authorization decision and resulting server-side behavior.",
                "Impact statement tied to data exposure or unauthorized action.",
            ],
            "oob_allowed": truthy(args.include_oob),
        }
    )


if __name__ == "__main__":
    main_wrapper(main)

