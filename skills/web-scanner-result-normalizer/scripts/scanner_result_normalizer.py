#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import json_dump, main_wrapper, parse_json_arg, require_url_in_scope, resolve_scoped_path, scope_from_args  # noqa: E402


def load_input(path: str, inline: str) -> object:
    if path:
        raw = resolve_scoped_path(path).read_text()
    else:
        raw = inline
    raw = raw.strip()
    if not raw:
        return []
    if "\n" in raw and not raw.startswith("[") and not raw.startswith("{"):
        return [json.loads(line) for line in raw.splitlines() if line.strip()]
    return json.loads(raw)


def map_category(name: str, tags: list[str]) -> tuple[list[str], list[str]]:
    value = " ".join([name] + tags).lower()
    wstg: list[str] = []
    api: list[str] = []
    if any(term in value for term in ["cors", "header", "tls", "cookie", "hsts"]):
        wstg.append("WSTG-CONF")
    if any(term in value for term in ["auth", "jwt", "session"]):
        wstg.append("WSTG-ATHN")
        api.append("API2")
    if any(term in value for term in ["idor", "bola", "authorization", "access control"]):
        wstg.append("WSTG-ATHZ")
        api.append("API1")
    if any(term in value for term in ["sqli", "xss", "injection", "ssti", "xxe"]):
        wstg.append("WSTG-INPV")
        api.append("API8")
    if not wstg:
        wstg.append("WSTG-INFO")
    return sorted(set(wstg)), sorted(set(api))


def normalize_nuclei(items: list[dict[str, object]], scope: dict[str, object]) -> list[dict[str, object]]:
    leads = []
    for item in items:
        url = str(item.get("matched-at") or item.get("host") or item.get("url") or "").strip()
        if not url:
            continue
        try:
            scoped_url = require_url_in_scope(url, scope)
        except SystemExit:
            continue
        info = item.get("info") if isinstance(item.get("info"), dict) else {}
        name = str(info.get("name") or item.get("template-id") or "nuclei lead")
        tags = [str(tag) for tag in info.get("tags", [])] if isinstance(info.get("tags"), list) else str(info.get("tags") or "").split(",")
        wstg, api = map_category(name, tags)
        leads.append(
            {
                "scanner": "nuclei",
                "status": "lead",
                "title": name,
                "severity": str(info.get("severity") or "informational").lower(),
                "affected_endpoint": scoped_url,
                "vuln_class": str(item.get("template-id") or ""),
                "wstg_ids": wstg,
                "api_top10_ids": api,
                "evidence": item.get("matcher-name") or item.get("extracted-results") or "",
            }
        )
    return leads


def normalize_zap(payload: object, scope: dict[str, object]) -> list[dict[str, object]]:
    leads = []
    sites = payload.get("site", []) if isinstance(payload, dict) else []
    if isinstance(sites, dict):
        sites = [sites]
    for site in sites if isinstance(sites, list) else []:
        alerts = site.get("alerts", []) if isinstance(site, dict) else []
        base = str(site.get("@name") or site.get("name") or "").strip()
        for alert in alerts if isinstance(alerts, list) else []:
            if not isinstance(alert, dict):
                continue
            instances = alert.get("instances", []) or [{}]
            for instance in instances if isinstance(instances, list) else [{}]:
                url = str(instance.get("uri") or instance.get("url") or base).strip()
                if not url:
                    continue
                try:
                    scoped_url = require_url_in_scope(url, scope)
                except SystemExit:
                    continue
                name = str(alert.get("name") or "ZAP baseline lead")
                wstg, api = map_category(name, [])
                leads.append(
                    {
                        "scanner": "zap",
                        "status": "lead",
                        "title": name,
                        "severity": str(alert.get("riskdesc") or alert.get("risk") or "informational").lower(),
                        "affected_endpoint": scoped_url,
                        "vuln_class": str(alert.get("pluginid") or ""),
                        "wstg_ids": wstg,
                        "api_top10_ids": api,
                        "evidence": alert.get("desc") or "",
                    }
                )
    return leads


def dedupe(leads: list[dict[str, object]]) -> list[dict[str, object]]:
    seen: set[tuple[str, str, str]] = set()
    out = []
    for lead in leads:
        key = (str(lead.get("scanner")), str(lead.get("affected_endpoint")), str(lead.get("title")))
        if key in seen:
            continue
        seen.add(key)
        out.append(lead)
    return out


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--scanner", choices=["auto", "nuclei", "zap"], default="auto")
    parser.add_argument("--input-path", default="")
    parser.add_argument("--input-json", default="")
    parser.add_argument("--scope-json")
    parser.add_argument("--target-urls", default="")
    parser.add_argument("--allowed-hosts", default="")
    args = parser.parse_args()

    payload = load_input(args.input_path, args.input_json)
    scope = scope_from_args(scope_json=args.scope_json, target_urls=args.target_urls, allowed_hosts=args.allowed_hosts)
    items = payload if isinstance(payload, list) else [payload] if isinstance(payload, dict) else []
    scanner = args.scanner
    if scanner == "auto":
        scanner = "zap" if isinstance(payload, dict) and "site" in payload else "nuclei"
    leads = normalize_zap(payload, scope) if scanner == "zap" else normalize_nuclei([item for item in items if isinstance(item, dict)], scope)

    json_dump(
        {
            "status": "ok",
            "scanner": scanner,
            "lead_count": len(dedupe(leads)),
            "leads": dedupe(leads),
            "policy": "Scanner output is normalized as lead status only. Run web-finding-validator before report inclusion.",
        }
    )


if __name__ == "__main__":
    main_wrapper(main)

