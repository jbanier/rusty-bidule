#!/usr/bin/env python3
from __future__ import annotations

import argparse
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import sys
import urllib.parse

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python < 3.11 fallback
    tomllib = None  # type: ignore[assignment]

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import json_dump, main_wrapper, parse_json_arg, resolve_scoped_path, split_items, tool_status  # noqa: E402


VERSION_RE = re.compile(r"(?:@|/|[-_.]v?)(\d+\.\d+(?:\.\d+)?(?:[-+][A-Za-z0-9_.-]+)?)")
CDN_MARKERS = ["cdn", "jsdelivr", "unpkg", "cdnjs", "googleapis", "bootstrapcdn", "cloudfront", "akamai"]


class AssetParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.assets: list[dict[str, object]] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        attr = {key.lower(): value or "" for key, value in attrs}
        if tag == "script" and attr.get("src"):
            self.assets.append({"type": "script", "url": attr["src"], "integrity": attr.get("integrity", ""), "crossorigin": attr.get("crossorigin", "")})
        elif tag == "link" and attr.get("href") and any(marker in attr.get("rel", "").lower() for marker in ["stylesheet", "preload", "modulepreload"]):
            self.assets.append({"type": "style", "url": attr["href"], "integrity": attr.get("integrity", ""), "crossorigin": attr.get("crossorigin", "")})


def read_json_path(path_value: str) -> object:
    return json.loads(resolve_scoped_path(path_value).read_text())


def parse_package_json(path: Path, payload: dict[str, object]) -> dict[str, object]:
    deps = {}
    for key in ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"]:
        value = payload.get(key)
        if isinstance(value, dict):
            deps[key] = {str(name): str(version) for name, version in value.items()}
    return {"path": str(path), "type": "package.json", "package": payload.get("name") or "", "dependency_groups": deps}


def parse_package_lock(path: Path, payload: dict[str, object]) -> dict[str, object]:
    packages = payload.get("packages") if isinstance(payload.get("packages"), dict) else {}
    dependencies = payload.get("dependencies") if isinstance(payload.get("dependencies"), dict) else {}
    return {
        "path": str(path),
        "type": "package-lock.json",
        "lockfile_version": payload.get("lockfileVersion"),
        "package_count": len(packages),
        "dependency_count": len(dependencies),
    }


def parse_requirements(path: Path, text: str) -> dict[str, object]:
    packages = []
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#") or line.startswith("-"):
            continue
        package = re.split(r"\s*(?:==|>=|<=|~=|>|<|!=)\s*", line, maxsplit=1)[0].strip()
        if package:
            packages.append({"name": package, "raw": line})
    return {"path": str(path), "type": "requirements.txt", "packages": packages[:500], "package_count": len(packages)}


def parse_pyproject(path: Path, text: str) -> dict[str, object]:
    if tomllib is None:
        return {"path": str(path), "type": "pyproject.toml", "parsed": False, "reason": "tomllib unavailable"}
    payload = tomllib.loads(text)
    project = payload.get("project") if isinstance(payload, dict) else {}
    deps = project.get("dependencies") if isinstance(project, dict) else []
    return {"path": str(path), "type": "pyproject.toml", "project": project.get("name") if isinstance(project, dict) else "", "dependency_count": len(deps) if isinstance(deps, list) else 0}


def parse_manifest(path_value: str) -> dict[str, object]:
    path = resolve_scoped_path(path_value)
    text = path.read_text(errors="replace")
    name = path.name.lower()
    if name == "package.json":
        return parse_package_json(path, json.loads(text))
    if name in {"package-lock.json", "npm-shrinkwrap.json"}:
        return parse_package_lock(path, json.loads(text))
    if name.startswith("requirements") and name.endswith(".txt"):
        return parse_requirements(path, text)
    if name == "pyproject.toml":
        return parse_pyproject(path, text)
    return {"path": str(path), "type": "unparsed", "reason": "recognized as inventory evidence but no lightweight parser is implemented"}


def origin(value: str) -> str:
    candidate = value.strip()
    if candidate.startswith("//"):
        candidate = f"https:{candidate}"
    parsed = urllib.parse.urlparse(candidate)
    if parsed.scheme not in {"http", "https"} or not parsed.hostname:
        return ""
    port = f":{parsed.port}" if parsed.port else ""
    return f"{parsed.scheme}://{parsed.hostname.lower()}{port}"


def asset_pinning(url: str) -> dict[str, object]:
    parsed = urllib.parse.urlparse(url if not url.startswith("//") else f"https:{url}")
    host = (parsed.hostname or "").lower()
    version_match = VERSION_RE.search(parsed.path) or VERSION_RE.search(parsed.query)
    return {
        "host": host,
        "origin": origin(url),
        "cdn_like": any(marker in host for marker in CDN_MARKERS),
        "version_pinned": bool(version_match),
        "version_hint": version_match.group(1) if version_match else "",
    }


def collect_assets(html_text: str, client_audit: object, browser_evidence: object) -> list[dict[str, object]]:
    parser = AssetParser()
    parser.feed(html_text)
    assets = parser.assets[:]
    if isinstance(client_audit, dict):
        for key in ["script_assets", "style_assets"]:
            value = client_audit.get(key)
            if isinstance(value, list):
                for item in value:
                    if isinstance(item, dict) and item.get("url"):
                        assets.append(
                            {
                                "type": "script" if key == "script_assets" else "style",
                                "url": str(item["url"]),
                                "integrity": "present" if item.get("integrity_present") else "",
                                "crossorigin": item.get("crossorigin") or "",
                            }
                        )
    if isinstance(browser_evidence, dict):
        for route in browser_evidence.get("routes", []) if isinstance(browser_evidence.get("routes"), list) else []:
            if isinstance(route, str) and origin(route):
                assets.append({"type": "browser-route", "url": route, "integrity": "", "crossorigin": ""})
    deduped: list[dict[str, object]] = []
    seen: set[tuple[str, str]] = set()
    for asset in assets:
        key = (str(asset.get("type")), str(asset.get("url")))
        if key not in seen:
            seen.add(key)
            deduped.append(asset)
    return deduped


def normalize_sca(payload: object) -> list[dict[str, object]]:
    leads: list[dict[str, object]] = []
    if isinstance(payload, dict):
        vulnerabilities = payload.get("vulnerabilities")
        if isinstance(vulnerabilities, dict):
            for name, item in vulnerabilities.items():
                if isinstance(item, dict):
                    leads.append({"source": "npm-audit", "package": name, "severity": str(item.get("severity") or "unknown"), "title": str(item.get("title") or item.get("name") or name)})
        results = payload.get("results")
        if isinstance(results, list):
            leads.extend(normalize_sca(results))
        vulns = payload.get("vulns") or payload.get("vulnerabilities")
        if isinstance(vulns, list):
            leads.extend(normalize_sca(vulns))
    elif isinstance(payload, list):
        for item in payload:
            if not isinstance(item, dict):
                continue
            package = item.get("name") or item.get("package") or item.get("dependency") or item.get("module_name") or item.get("id") or ""
            severity = item.get("severity") or item.get("cvss_score") or item.get("rating") or "unknown"
            title = item.get("title") or item.get("advisory") or item.get("description") or item.get("id") or package or "SCA lead"
            leads.append({"source": "supplied-sca", "package": str(package), "severity": str(severity).lower(), "title": str(title)[:240]})
    return leads[:500]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest-paths", default="")
    parser.add_argument("--asset-html-text", default="")
    parser.add_argument("--asset-html-paths", default="")
    parser.add_argument("--browser-evidence-json", default="{}")
    parser.add_argument("--client-audit-json", default="{}")
    parser.add_argument("--scanner-results-json", default="{}")
    parser.add_argument("--scanner-results-path", default="")
    args = parser.parse_args()

    manifests = []
    for path_value in split_items(args.manifest_paths):
        manifests.append(parse_manifest(path_value))

    html_text = args.asset_html_text
    for path_value in split_items(args.asset_html_paths):
        html_text += "\n" + resolve_scoped_path(path_value).read_text(errors="replace")
    browser_evidence = parse_json_arg(args.browser_evidence_json, {})
    client_audit = parse_json_arg(args.client_audit_json, {})
    scanner_payload = read_json_path(args.scanner_results_path) if args.scanner_results_path else parse_json_arg(args.scanner_results_json, {})

    assets = collect_assets(html_text, client_audit, browser_evidence)
    third_party_assets = []
    for asset in assets:
        url = str(asset.get("url") or "")
        asset_origin = origin(url)
        if not asset_origin:
            continue
        pinning = asset_pinning(url)
        third_party_assets.append(
            {
                "type": asset.get("type"),
                "url": url,
                "origin": asset_origin,
                "integrity_present": bool(asset.get("integrity")),
                "crossorigin": asset.get("crossorigin") or "",
                "cdn_like": pinning["cdn_like"],
                "version_pinned": pinning["version_pinned"],
                "version_hint": pinning["version_hint"],
            }
        )

    missing_sri = [asset for asset in third_party_assets if not asset["integrity_present"]]
    floating_assets = [asset for asset in third_party_assets if not asset["version_pinned"] and asset["cdn_like"]]
    command_plan = [
        {
            "ecosystem": "node",
            "prerequisites": "Run from the project directory with operator approval for network SCA queries.",
            "commands": [["npm", "audit", "--json"], ["osv-scanner", "--lockfile", "package-lock.json"], ["retire", "--outputformat", "json"]],
        },
        {
            "ecosystem": "python",
            "prerequisites": "Run against the intended virtual environment or requirements files with operator approval for network SCA queries.",
            "commands": [["pip-audit", "-f", "json"], ["osv-scanner", "--recursive", "."]],
        },
        {
            "ecosystem": "browser-assets",
            "prerequisites": "Validate third-party script/style ownership and SRI needs manually before changing production assets.",
            "commands": [["dependency-check", "--scan", "."]],
        },
    ]

    json_dump(
        {
            "status": "ok",
            "package_manifests": manifests,
            "third_party_assets": third_party_assets[:500],
            "sri_coverage": {
                "asset_count": len(third_party_assets),
                "missing_sri_count": len(missing_sri),
                "missing_sri_assets": missing_sri[:100],
            },
            "pinning_observations": {
                "floating_cdn_asset_count": len(floating_assets),
                "floating_cdn_assets": floating_assets[:100],
            },
            "normalized_sca_leads": normalize_sca(scanner_payload),
            "tool_availability": tool_status(["npm", "osv-scanner", "pip-audit", "retire", "dependency-check"]),
            "sca_command_plan": command_plan,
            "policy": "No network SCA scanner was run. Scanner commands are follow-up plans requiring explicit operator approval.",
        }
    )


if __name__ == "__main__":
    main_wrapper(main)

