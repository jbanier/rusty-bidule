#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import ScopeError, json_dump, main_wrapper, parse_json_arg, resolve_scoped_path  # noqa: E402


def finding_block(item: dict[str, object]) -> str:
    title = item.get("title") or item.get("name") or "Untitled finding"
    severity = item.get("severity", "informational")
    evidence = item.get("evidence", item.get("source_artifact", ""))
    remediation = item.get("remediation", "Define remediation with the owning engineering team.")
    return "\n".join(
        [
            f"### {title}",
            f"- Severity: {severity}",
            f"- Status: {item.get('status', 'confirmed' if item.get('confirmed') else 'lead')}",
            f"- Evidence: {evidence}",
            f"- Impact: {item.get('impact', 'Impact not yet confirmed.')}",
            f"- Remediation: {remediation}",
            "",
        ]
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--case-name", default="web-app-posture-assessment")
    parser.add_argument("--scope-summary", default="")
    parser.add_argument("--findings-json", default="[]")
    parser.add_argument("--output-path", default="")
    args = parser.parse_args()
    findings = parse_json_arg(args.findings_json, [])
    report = "\n".join(
        [
            f"# {args.case_name}",
            "",
            "## Scope",
            args.scope_summary or "Scope summary not provided.",
            "",
            "## Findings",
            "\n".join(finding_block(item) for item in findings) if findings else "No confirmed findings supplied.",
            "## Retest Checklist",
            "- Re-run affected workflow with fixed build.",
            "- Confirm access control, input handling, and logging behavior.",
            "- Update severity if exploitability or impact changes.",
            "",
        ]
    )
    written_path = None
    if args.output_path:
        path = resolve_scoped_path(args.output_path, must_exist=False)
        if not path.parent.exists():
            raise ScopeError(f"output parent directory does not exist: {path.parent}")
        path.write_text(report)
        written_path = str(path)
    json_dump({"status": "ok", "report_markdown": report, "output_path": written_path, "finding_count": len(findings)})


if __name__ == "__main__":
    main_wrapper(main)
