#!/usr/bin/env python3
from __future__ import annotations

import argparse
import re
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import ScopeError, json_dump, main_wrapper, parse_json_arg, require_url_in_scope, scope_from_args  # noqa: E402


SECRET_PATTERNS = [
    re.compile(r"Authorization:\s*Bearer\s+[A-Za-z0-9_.=-]{24,}", re.I),
    re.compile(r"\bCookie:\s*[^[][^;\n]{20,}", re.I),
    re.compile(r"\bSet-Cookie:\s*[^[][^;\n]{20,}", re.I),
    re.compile(r"\b(api[_-]?key|x-api-key|x-auth-token|csrf[_-]?token)\b\s*[:=]\s*['\"]?[A-Za-z0-9_.=-]{16,}", re.I),
]


def truthy(value: object) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    return str(value).strip().lower() in {"1", "true", "yes", "y", "pass", "validated"}


def text(item: dict[str, object], *keys: str) -> str:
    for key in keys:
        value = item.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return ""


def has_secret(value: str) -> bool:
    return any(pattern.search(value) for pattern in SECRET_PATTERNS)


def gate(name: str, passed: bool, reason: str) -> dict[str, str]:
    return {"gate": name, "status": "pass" if passed else "fail", "reason": reason}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--finding-json", required=True)
    parser.add_argument("--scope-json")
    parser.add_argument("--target-urls", default="")
    parser.add_argument("--allowed-hosts", default="")
    args = parser.parse_args()

    finding = parse_json_arg(args.finding_json, {})
    if not isinstance(finding, dict):
        raise ScopeError("finding-json must be a JSON object")
    endpoint = text(finding, "affected_endpoint", "endpoint", "url")
    scope = scope_from_args(
        scope_json=args.scope_json,
        target_urls=",".join(filter(None, [args.target_urls, endpoint])),
        allowed_hosts=args.allowed_hosts,
    )

    request = text(finding, "request", "http_request", "repro_request")
    response = text(finding, "response", "http_response", "evidence_response")
    steps = text(finding, "steps", "reproduction_steps", "poc")
    impact = text(finding, "impact", "demonstrated_impact")
    real_vuln = finding.get("real_vulnerability", finding.get("confirmed_attack_scenario"))
    client_repro = finding.get("client_reproducible", finding.get("reproducible"))
    evidence_blob = "\n".join([request, response, steps, impact, text(finding, "evidence")])

    in_scope = False
    scope_reason = "affected endpoint missing"
    if endpoint:
        try:
            require_url_in_scope(endpoint, scope)
            in_scope = True
            scope_reason = "affected endpoint is allowed by scope"
        except SystemExit as exc:
            scope_reason = str(exc)

    gates = [
        gate("reproducible-request", bool(request or steps), "request or reproduction steps present" if request or steps else "missing request or reproduction steps"),
        gate("http-evidence", bool(request and response), "request and response evidence present" if request and response else "missing request or response evidence"),
        gate("impact-demonstrated", bool(impact), "impact is described" if impact else "missing demonstrated impact"),
        gate("in-scope", in_scope, scope_reason),
        gate("real-vulnerability", truthy(real_vuln), "real attack scenario asserted" if truthy(real_vuln) else "not established beyond a lead or informational issue"),
        gate("client-reproducible", truthy(client_repro), "client reproduction is documented" if truthy(client_repro) else "client reproducibility is missing or unreliable"),
        gate("credential-redaction", not has_secret(evidence_blob), "no obvious live secrets detected" if not has_secret(evidence_blob) else "possible live credential or token found in evidence"),
    ]
    failed = [item for item in gates if item["status"] != "pass"]
    if not failed:
        recommended_status = "validated"
    elif any(item["gate"] in {"in-scope", "credential-redaction"} for item in failed):
        recommended_status = "rejected"
    else:
        recommended_status = "needs-work"

    json_dump(
        {
            "status": "ok",
            "recommended_status": recommended_status,
            "validation_gates": gates,
            "finding_patch": {
                "status": recommended_status,
                "validation_gates": gates,
                "affected_endpoint": endpoint or None,
            },
        }
    )


if __name__ == "__main__":
    main_wrapper(main)

