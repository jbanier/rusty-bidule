#!/usr/bin/env python3
from __future__ import annotations

import argparse
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import sys
import urllib.parse

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import host_allowed, json_dump, main_wrapper, normalize_host, parse_json_arg, resolve_scoped_path, scope_from_args, split_items  # noqa: E402


URL_RE = re.compile(r"""(?:"|')((?:https?://|wss?://|/)[A-Za-z0-9_./?&=%:#@!$,;*+\-\[\]()~]+)(?:"|')""")
OPEN_PATH_RE = re.compile(r"""(?:"|')((?:/[A-Za-z0-9_.~!$&'()*+,;=:@%-]+){1,8}(?:\?[A-Za-z0-9_.~!$&'()*+,;=:@%/?-]+)?)(?:"|')""")
SITEMAP_LOC_RE = re.compile(r"<loc>\s*([^<\s]+)\s*</loc>", re.IGNORECASE)
RISK_PATTERNS = [
    ("dom-inner-html", re.compile(r"\b(?:innerHTML|outerHTML)\b"), "DOM HTML assignment requires source tracing and output encoding review."),
    ("dom-document-write", re.compile(r"\bdocument\.write(?:ln)?\s*\("), "document.write can turn untrusted strings into executable markup."),
    ("dom-insert-adjacent-html", re.compile(r"\binsertAdjacentHTML\s*\("), "insertAdjacentHTML requires strict trust boundaries for HTML strings."),
    ("dynamic-code", re.compile(r"\b(?:eval|Function)\s*\("), "Dynamic code execution requires proof that input is trusted."),
    ("string-timer", re.compile(r"\bset(?:Timeout|Interval)\s*\(\s*['\"]"), "String timers can behave like eval when input reaches them."),
    ("location-source", re.compile(r"\b(?:location\.(?:hash|search|href)|document\.(?:URL|referrer))\b"), "URL-derived sources should be traced to sinks."),
    ("post-message-listener", re.compile(r"addEventListener\s*\(\s*['\"]message['\"]|\.onmessage\s*="), "postMessage handlers require origin and schema validation."),
    ("post-message-send", re.compile(r"\.postMessage\s*\("), "postMessage sends require constrained target origins."),
    ("browser-storage", re.compile(r"\b(?:localStorage|sessionStorage)\b"), "Browser storage should not hold secrets or bearer tokens."),
]


class ClientArtifactParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.scripts: list[dict[str, object]] = []
        self.styles: list[dict[str, object]] = []
        self.forms: list[dict[str, object]] = []
        self.meta_csp: list[str] = []
        self.inline_scripts: list[str] = []
        self._in_script = False

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        attr = {key.lower(): value or "" for key, value in attrs}
        tag = tag.lower()
        if tag == "script":
            self._in_script = True
            if attr.get("src"):
                self.scripts.append(
                    {
                        "url": attr["src"],
                        "integrity": attr.get("integrity", ""),
                        "crossorigin": attr.get("crossorigin", ""),
                    }
                )
        elif tag == "link":
            rel = attr.get("rel", "").lower()
            if attr.get("href") and ("stylesheet" in rel or "preload" in rel or "modulepreload" in rel):
                self.styles.append(
                    {
                        "url": attr["href"],
                        "rel": rel,
                        "integrity": attr.get("integrity", ""),
                        "crossorigin": attr.get("crossorigin", ""),
                    }
                )
        elif tag == "form":
            self.forms.append({"method": attr.get("method", "GET").upper(), "action": attr.get("action", "")})
        elif tag == "meta" and attr.get("http-equiv", "").lower() == "content-security-policy":
            if attr.get("content"):
                self.meta_csp.append(attr["content"])

    def handle_data(self, data: str) -> None:
        if self._in_script and data.strip():
            self.inline_scripts.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag.lower() == "script":
            self._in_script = False


def read_texts(paths: str) -> list[dict[str, str]]:
    docs = []
    for path_value in split_items(paths):
        path = resolve_scoped_path(path_value)
        docs.append({"source": str(path), "text": path.read_text(errors="replace")})
    return docs


def parse_json_or_file(raw: str, paths: str, default: object) -> list[object]:
    values: list[object] = []
    parsed = parse_json_arg(raw, default)
    if parsed not in ({}, [], None, ""):
        values.append(parsed)
    for path_value in split_items(paths):
        path = resolve_scoped_path(path_value)
        values.append(json.loads(path.read_text()))
    return values


def absolute_url(base_url: str, value: str) -> str:
    value = value.strip()
    if not value:
        return ""
    if value.startswith("//"):
        return f"https:{value}"
    if base_url and value.startswith("/"):
        return urllib.parse.urljoin(base_url, value)
    return value


def origin(value: str) -> str:
    candidate = value.strip()
    if candidate.startswith("//"):
        candidate = f"https:{candidate}"
    parsed = urllib.parse.urlparse(candidate)
    if parsed.scheme not in {"http", "https", "ws", "wss"} or not parsed.hostname:
        return ""
    port = f":{parsed.port}" if parsed.port else ""
    return f"{parsed.scheme}://{parsed.hostname.lower()}{port}"


def csp_origins(csp_values: list[str]) -> list[str]:
    origins: set[str] = set()
    ignored = {
        "'self'",
        "'none'",
        "'unsafe-inline'",
        "'unsafe-eval'",
        "'strict-dynamic'",
        "'unsafe-hashes'",
        "data:",
        "blob:",
        "https:",
        "http:",
    }
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
    for csp in csp_values:
        for raw_token in re.split(r"[\s;]+", csp):
            token = raw_token.strip()
            if not token or token in ignored or token in directives or token.startswith("'nonce-") or token.startswith("'sha"):
                continue
            if token.endswith(":") and "://" not in token:
                continue
            if "." not in token and not token.startswith("*.") and token != "localhost" and "://" not in token:
                continue
            parsed = urllib.parse.urlparse(token if "://" in token else f"https://{token.lstrip('*.')}")
            if parsed.hostname:
                origins.add(parsed.hostname.lower())
    return sorted(origins)


def headers_to_csp(headers_payload: object) -> list[str]:
    values: list[str] = []

    def visit(value: object) -> None:
        if isinstance(value, dict):
            for key, item in value.items():
                if str(key).lower() in {"content-security-policy", "content_security_policy", "csp"} and isinstance(item, str):
                    values.append(item)
                else:
                    visit(item)
        elif isinstance(value, list):
            for item in value:
                visit(item)

    visit(headers_payload)
    return values


def flatten_routes(value: object) -> list[str]:
    routes: list[str] = []
    if isinstance(value, str):
        routes.append(value)
    elif isinstance(value, dict):
        for key in ["routes", "api_paths", "websocket_urls", "script_sources", "client_routes", "api_candidates", "shadow_api_candidates"]:
            item = value.get(key)
            if isinstance(item, list):
                for entry in item:
                    if isinstance(entry, str):
                        routes.append(entry)
                    elif isinstance(entry, dict):
                        candidate = entry.get("url") or entry.get("path") or entry.get("candidate")
                        if candidate:
                            routes.append(str(candidate))
        forms = value.get("forms")
        if isinstance(forms, list):
            for form in forms:
                if isinstance(form, dict) and form.get("action"):
                    routes.append(str(form["action"]))
        observations = value.get("observations")
        if isinstance(observations, list):
            routes.extend(flatten_routes(observations))
    elif isinstance(value, list):
        for item in value:
            routes.extend(flatten_routes(item))
    return routes


def openapi_paths(values: list[object]) -> list[str]:
    paths: list[str] = []
    for value in values:
        if isinstance(value, dict):
            raw_paths = value.get("paths")
            if isinstance(raw_paths, dict):
                paths.extend(str(path) for path in raw_paths.keys())
            servers = value.get("servers")
            if isinstance(servers, list):
                for server in servers:
                    if isinstance(server, dict) and server.get("url"):
                        paths.append(str(server["url"]))
    return paths


def dedupe(values: list[str], limit: int = 500) -> list[str]:
    out: list[str] = []
    for value in values:
        value = value.strip()
        if value and value not in out:
            out.append(value)
        if len(out) >= limit:
            break
    return out


def in_scope(value: str, allowed_hosts: list[str]) -> bool:
    parsed = urllib.parse.urlparse(value if not value.startswith("//") else f"https:{value}")
    if not parsed.hostname:
        return False
    return host_allowed(parsed.hostname, allowed_hosts)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default="")
    parser.add_argument("--html-text", default="")
    parser.add_argument("--html-paths", default="")
    parser.add_argument("--js-text", default="")
    parser.add_argument("--js-paths", default="")
    parser.add_argument("--browser-evidence-json", default="{}")
    parser.add_argument("--route-inventory-json", default="{}")
    parser.add_argument("--sitemap-text", default="")
    parser.add_argument("--sitemap-paths", default="")
    parser.add_argument("--openapi-json", default="{}")
    parser.add_argument("--openapi-paths", default="")
    parser.add_argument("--headers-json", default="{}")
    parser.add_argument("--scope-json")
    parser.add_argument("--target-urls", default="")
    parser.add_argument("--allowed-hosts", default="")
    args = parser.parse_args()

    scope = scope_from_args(scope_json=args.scope_json, target_urls=args.target_urls or args.base_url, allowed_hosts=args.allowed_hosts)
    allowed_hosts = list(scope.get("allowed_hosts") or [])
    base_url = args.base_url or (scope.get("target_urls") or [""])[0]

    html_docs = [{"source": "inline-html", "text": args.html_text}] if args.html_text else []
    html_docs.extend(read_texts(args.html_paths))
    js_docs = [{"source": "inline-js", "text": args.js_text}] if args.js_text else []
    js_docs.extend(read_texts(args.js_paths))
    sitemap_docs = [{"source": "inline-sitemap", "text": args.sitemap_text}] if args.sitemap_text else []
    sitemap_docs.extend(read_texts(args.sitemap_paths))

    parser_html = ClientArtifactParser()
    for doc in html_docs:
        parser_html.feed(doc["text"])

    html_text = "\n".join(doc["text"] for doc in html_docs)
    js_text = "\n".join(doc["text"] for doc in js_docs + [{"source": "inline-script", "text": text} for text in parser_html.inline_scripts])
    browser_evidence = parse_json_arg(args.browser_evidence_json, {})
    route_inventory = parse_json_arg(args.route_inventory_json, {})
    openapi_values = parse_json_or_file(args.openapi_json, args.openapi_paths, {})
    headers_payload = parse_json_arg(args.headers_json, {})

    raw_routes = [match.group(1) for match in URL_RE.finditer(f"{html_text}\n{js_text}")]
    raw_routes.extend(match.group(1) for match in OPEN_PATH_RE.finditer(js_text))
    raw_routes.extend(item["url"] for item in parser_html.scripts if isinstance(item.get("url"), str))
    raw_routes.extend(item["url"] for item in parser_html.styles if isinstance(item.get("url"), str))
    raw_routes.extend(form["action"] for form in parser_html.forms if isinstance(form.get("action"), str))
    raw_routes.extend(flatten_routes(browser_evidence))
    raw_routes.extend(flatten_routes(route_inventory))
    for doc in sitemap_docs:
        raw_routes.extend(match.group(1) for match in SITEMAP_LOC_RE.finditer(doc["text"]))
    raw_routes.extend(openapi_paths(openapi_values))

    client_routes = dedupe([absolute_url(base_url, route) for route in raw_routes if route])
    api_candidates = [
        route
        for route in client_routes
        if any(marker in route.lower() for marker in ["/api/", "/graphql", "/rest/", "/v1/", "/v2/", "/swagger", "/openapi"])
    ]
    script_assets = [
        {
            "url": absolute_url(base_url, str(asset.get("url") or "")),
            "integrity_present": bool(asset.get("integrity")),
            "crossorigin": asset.get("crossorigin") or "",
        }
        for asset in parser_html.scripts
    ]
    style_assets = [
        {
            "url": absolute_url(base_url, str(asset.get("url") or "")),
            "integrity_present": bool(asset.get("integrity")),
            "rel": asset.get("rel") or "",
        }
        for asset in parser_html.styles
    ]
    asset_urls = [str(asset["url"]) for asset in script_assets + style_assets]
    external_origins = sorted({origin(url) for url in client_routes + asset_urls if origin(url) and (not allowed_hosts or not in_scope(url, allowed_hosts))})
    csp_values = headers_to_csp(headers_payload) + parser_html.meta_csp
    csp_hosts = csp_origins(csp_values)

    dom_risk_indicators = []
    combined_js = js_text[:2_000_000]
    for name, pattern, guidance in RISK_PATTERNS:
        matches = list(pattern.finditer(combined_js))
        if matches:
            dom_risk_indicators.append(
                {
                    "indicator": name,
                    "count": len(matches),
                    "guidance": guidance,
                }
            )

    missing_sri = [asset for asset in script_assets + style_assets if origin(str(asset["url"])) and not asset["integrity_present"]]
    shadow_api_candidates = []
    for candidate in dedupe(api_candidates + [route for route in client_routes if route.endswith((".json", ".xml"))], limit=200):
        shadow_api_candidates.append(
            {
                "candidate": candidate,
                "source": "passive-client-artifact",
                "in_scope": in_scope(candidate, allowed_hosts) if allowed_hosts and urllib.parse.urlparse(candidate).hostname else None,
                "reason": "Discovered in JavaScript, browser evidence, sitemap, OpenAPI, CSP, or HTML assets; validate authorization and ownership before testing.",
            }
        )

    manual_validation_commands = [
        {
            "action": "review-route",
            "target": candidate,
            "authorization_required": "active authorization required before fetching or probing",
            "tool_hint": "web-http-baseline can collect low-impact evidence when scope and active_authorized=true are set.",
        }
        for candidate in api_candidates[:20]
    ]

    json_dump(
        {
            "status": "ok",
            "sources": {
                "html_documents": [doc["source"] for doc in html_docs],
                "js_documents": [doc["source"] for doc in js_docs],
                "sitemap_documents": [doc["source"] for doc in sitemap_docs],
                "openapi_documents": len(openapi_values),
            },
            "client_routes": client_routes[:500],
            "api_candidates": api_candidates[:300],
            "external_origins": external_origins,
            "csp_origins": csp_hosts,
            "script_assets": script_assets[:300],
            "style_assets": style_assets[:200],
            "sri_coverage": {
                "asset_count": len(script_assets) + len(style_assets),
                "missing_sri_count": len(missing_sri),
                "missing_sri_assets": missing_sri[:100],
            },
            "dom_risk_indicators": dom_risk_indicators,
            "shadow_api_candidates": shadow_api_candidates,
            "manual_validation_commands": manual_validation_commands,
            "policy": "Passive client-side inventory only. Do not fetch, fuzz, or scan candidates without explicit scope and active authorization.",
        }
    )


if __name__ == "__main__":
    main_wrapper(main)
