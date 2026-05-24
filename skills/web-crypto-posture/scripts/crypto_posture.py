#!/usr/bin/env python3
from __future__ import annotations

import argparse
from datetime import datetime, timezone
from pathlib import Path
import re
import sys
import urllib.parse

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import host_allowed, json_dump, main_wrapper, normalize_host, parse_json_arg, resolve_scoped_path, scope_from_args, split_items, tool_status  # noqa: E402


HSTS_MAX_AGE_RE = re.compile(r"max-age=(\d+)", re.IGNORECASE)


def parse_json_or_path(raw: str, path_value: str, default: object) -> object:
    if path_value:
        path = resolve_scoped_path(path_value)
        return parse_json_arg(path.read_text(), default)
    return parse_json_arg(raw, default)


def collect_headers(payload: object) -> dict[str, str]:
    headers: dict[str, str] = {}

    def visit(value: object) -> None:
        if isinstance(value, dict):
            if "headers" in value and isinstance(value["headers"], dict):
                for key, item in value["headers"].items():
                    headers[str(key).lower()] = str(item)
            for key, item in value.items():
                lowered = str(key).lower().replace("_", "-")
                if lowered in {
                    "strict-transport-security",
                    "content-security-policy",
                    "x-content-type-options",
                    "x-frame-options",
                    "referrer-policy",
                    "permissions-policy",
                } and isinstance(item, str):
                    headers[lowered] = item
                elif lowered == "security-headers" and isinstance(item, dict):
                    for header_name, header_data in item.items():
                        if isinstance(header_data, dict) and header_data.get("value"):
                            headers[str(header_name).lower()] = str(header_data["value"])
                else:
                    visit(item)
        elif isinstance(value, list):
            for item in value:
                visit(item)

    visit(payload)
    return headers


def csp_hosts(csp: str) -> list[str]:
    hosts: set[str] = set()
    directives = {
        "base-uri",
        "child-src",
        "connect-src",
        "default-src",
        "font-src",
        "form-action",
        "frame-ancestors",
        "frame-src",
        "img-src",
        "manifest-src",
        "media-src",
        "object-src",
        "prefetch-src",
        "script-src",
        "script-src-attr",
        "script-src-elem",
        "style-src",
        "style-src-attr",
        "style-src-elem",
        "worker-src",
    }
    for token in re.split(r"[\s;]+", csp):
        token = token.strip()
        if not token or token.startswith("'") or token in directives or token in {"https:", "http:", "data:", "blob:"}:
            continue
        if "." not in token and not token.startswith("*.") and token != "localhost" and "://" not in token:
            continue
        parsed = urllib.parse.urlparse(token if "://" in token else f"https://{token.lstrip('*.')}")
        if parsed.hostname:
            hosts.add(parsed.hostname.lower())
    return sorted(hosts)


def hosts_from_ct(value: object) -> list[str]:
    hosts: list[str] = []
    if isinstance(value, str):
        hosts.extend(split_items(value))
    elif isinstance(value, list):
        for item in value:
            hosts.extend(hosts_from_ct(item))
    elif isinstance(value, dict):
        for key in ["host", "hostname", "name", "name_value", "common_name", "dns_names"]:
            item = value.get(key)
            if isinstance(item, str):
                hosts.extend(split_items(item.replace("\n", ",")))
            elif isinstance(item, list):
                hosts.extend(str(entry) for entry in item)
    return [normalize_host(host.lstrip("*.")) for host in hosts if normalize_host(host.lstrip("*."))]


def host_in_scope(host: str, allowed_hosts: list[str]) -> bool:
    return bool(allowed_hosts) and host_allowed(host, allowed_hosts)


def cert_findings(cert_payload: object) -> list[dict[str, object]]:
    findings: list[dict[str, object]] = []
    values = cert_payload if isinstance(cert_payload, list) else [cert_payload] if isinstance(cert_payload, dict) else []
    now = datetime.now(timezone.utc)
    for item in values:
        if not isinstance(item, dict):
            continue
        not_after = item.get("not_after") or item.get("expires") or item.get("valid_to")
        if isinstance(not_after, str) and not_after:
            normalized = not_after.replace("Z", "+00:00")
            try:
                expires = datetime.fromisoformat(normalized)
                if expires.tzinfo is None:
                    expires = expires.replace(tzinfo=timezone.utc)
                days = (expires - now).days
                if days < 30:
                    findings.append({"type": "certificate-expiry", "severity": "medium" if days >= 0 else "high", "days_remaining": days, "evidence": not_after})
            except ValueError:
                findings.append({"type": "certificate-expiry-unparsed", "severity": "info", "evidence": not_after})
        if item.get("self_signed") is True:
            findings.append({"type": "self-signed-certificate", "severity": "medium", "evidence": "self_signed=true"})
    return findings


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target-urls", default="")
    parser.add_argument("--allowed-hosts", default="")
    parser.add_argument("--scope-json")
    parser.add_argument("--headers-json", default="{}")
    parser.add_argument("--http-baseline-json", default="{}")
    parser.add_argument("--cert-observations-json", default="[]")
    parser.add_argument("--scanner-json", default="{}")
    parser.add_argument("--scanner-path", default="")
    parser.add_argument("--ct-hosts", default="")
    parser.add_argument("--ct-json", default="[]")
    parser.add_argument("--csp-origins", default="")
    args = parser.parse_args()

    scope = scope_from_args(scope_json=args.scope_json, target_urls=args.target_urls, allowed_hosts=args.allowed_hosts)
    allowed_hosts = list(scope.get("allowed_hosts") or [])
    target_urls = list(scope.get("target_urls") or [])
    headers = collect_headers(parse_json_arg(args.headers_json, {}))
    headers.update(collect_headers(parse_json_arg(args.http_baseline_json, {})))
    cert_payload = parse_json_arg(args.cert_observations_json, [])
    scanner_payload = parse_json_or_path(args.scanner_json, args.scanner_path, {})

    related_hosts: list[dict[str, object]] = []
    for host in hosts_from_ct(args.ct_hosts) + hosts_from_ct(parse_json_arg(args.ct_json, [])):
        related_hosts.append({"host": host, "source": "certificate-transparency", "in_scope": host_in_scope(host, allowed_hosts)})
    for host in split_items(args.csp_origins) + csp_hosts(headers.get("content-security-policy", "")):
        host = normalize_host(host)
        if host:
            related_hosts.append({"host": host, "source": "content-security-policy", "in_scope": host_in_scope(host, allowed_hosts)})

    deduped_related = []
    seen_related: set[tuple[str, str]] = set()
    for item in related_hosts:
        key = (str(item["host"]), str(item["source"]))
        if key not in seen_related:
            seen_related.add(key)
            deduped_related.append(item)

    findings: list[dict[str, object]] = []
    hsts = headers.get("strict-transport-security", "")
    if not hsts:
        findings.append({"type": "missing-hsts", "severity": "low", "evidence": "Strict-Transport-Security header not present in supplied evidence."})
    else:
        match = HSTS_MAX_AGE_RE.search(hsts)
        if match and int(match.group(1)) < 15552000:
            findings.append({"type": "short-hsts-max-age", "severity": "low", "evidence": hsts})
        if "includesubdomains" not in hsts.lower():
            findings.append({"type": "hsts-without-includesubdomains", "severity": "info", "evidence": hsts})
    csp = headers.get("content-security-policy", "")
    if csp:
        if "'unsafe-eval'" in csp or "'unsafe-inline'" in csp:
            findings.append({"type": "relaxed-csp", "severity": "info", "evidence": "CSP contains unsafe-inline or unsafe-eval."})
        if "http:" in csp:
            findings.append({"type": "csp-allows-http-source", "severity": "low", "evidence": "CSP includes http: source expression."})
    else:
        findings.append({"type": "missing-csp", "severity": "info", "evidence": "Content-Security-Policy not present in supplied evidence."})
    findings.extend(cert_findings(cert_payload))
    if isinstance(scanner_payload, dict) and scanner_payload:
        findings.append({"type": "scanner-evidence-supplied", "severity": "info", "evidence": "External TLS/scanner JSON was supplied; normalize details manually before reporting."})

    confirmed_targets = []
    command_plan = []
    for url in target_urls:
        parsed = urllib.parse.urlparse(url)
        host = parsed.hostname or ""
        if not host:
            continue
        confirmed_targets.append({"url": url, "host": host, "in_scope": host_in_scope(host, allowed_hosts)})
        if host_in_scope(host, allowed_hosts):
            command_plan.append(
                {
                    "target": host,
                    "authorization_required": "active authorization and rate limits must be confirmed before running",
                    "commands": [
                        ["testssl.sh", "--fast", "--warnings", "batch", url],
                        ["nmap", "--script", "ssl-enum-ciphers", "-p", str(parsed.port or 443), host],
                        ["openssl", "s_client", "-connect", f"{host}:{parsed.port or 443}", "-servername", host],
                    ],
                }
            )

    json_dump(
        {
            "status": "ok",
            "confirmed_in_scope_targets": confirmed_targets,
            "related_host_candidates": deduped_related,
            "headers_reviewed": sorted(headers.keys()),
            "crypto_findings": findings,
            "tls_follow_up_commands": command_plan,
            "tool_availability": tool_status(["testssl.sh", "nmap", "openssl"]),
            "policy": "Related hosts are candidates only. Do not actively test CT/CSP-derived hosts unless they match authorized scope.",
        }
    )


if __name__ == "__main__":
    main_wrapper(main)
