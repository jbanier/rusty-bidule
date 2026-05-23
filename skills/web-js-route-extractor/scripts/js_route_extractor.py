#!/usr/bin/env python3
from __future__ import annotations

import argparse
from html.parser import HTMLParser
from pathlib import Path
import re
import sys
import urllib.parse

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import fetch_url, json_dump, main_wrapper, require_url_in_scope, resolve_scoped_path, scope_from_args, truthy  # noqa: E402


URL_RE = re.compile(r"""(?:"|')((?:https?://|wss?://|/)[A-Za-z0-9_./?&=%:#@+-]{2,})(?:"|')""")
PARAM_RE = re.compile(r"""(?:params|query|body|data|headers)\s*[:=]\s*[{[]|[?&]([A-Za-z0-9_.-]{2,40})=""")
SOURCE_MAP_RE = re.compile(r"sourceMappingURL=([^\s*]+)")


class FormParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.forms: list[dict[str, object]] = []
        self.scripts: set[str] = set()
        self._form: dict[str, object] | None = None

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        attr = {key.lower(): value or "" for key, value in attrs}
        if tag == "script" and attr.get("src"):
            self.scripts.add(attr["src"])
        if tag == "form":
            self._form = {"method": attr.get("method", "GET").upper(), "action": attr.get("action", ""), "inputs": []}
            self.forms.append(self._form)
        if tag in {"input", "select", "textarea"} and self._form is not None:
            self._form["inputs"].append({"name": attr.get("name", ""), "type": attr.get("type", tag)})

    def handle_endtag(self, tag: str) -> None:
        if tag == "form":
            self._form = None


def collect_text(args: argparse.Namespace, scope: dict[str, object]) -> tuple[str, str | None]:
    if args.input_text:
        return args.input_text, None
    if args.input_path:
        path = resolve_scoped_path(args.input_path)
        return path.read_text(errors="replace"), str(path)
    if args.input_url:
        url = require_url_in_scope(args.input_url, scope, active=truthy(args.fetch))
        if not truthy(args.fetch):
            return "", url
        response = fetch_url(url, max_bytes=args.max_bytes)
        return str(response.get("body_preview") or ""), url
    return "", None


def absolute(base_url: str | None, value: str) -> str:
    if base_url and value.startswith("/"):
        return urllib.parse.urljoin(base_url, value)
    return value


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input-text", default="")
    parser.add_argument("--input-path", default="")
    parser.add_argument("--input-url", default="")
    parser.add_argument("--scope-json")
    parser.add_argument("--allowed-hosts", default="")
    parser.add_argument("--active-authorized", default="false")
    parser.add_argument("--fetch", default="false")
    parser.add_argument("--max-bytes", type=int, default=500000)
    args = parser.parse_args()

    scope = scope_from_args(
        scope_json=args.scope_json,
        target_urls=args.input_url,
        allowed_hosts=args.allowed_hosts,
        active_authorized=args.active_authorized,
    )
    body, source = collect_text(args, scope)
    parser_html = FormParser()
    parser_html.feed(body)
    urls = sorted({absolute(args.input_url or None, match.group(1)) for match in URL_RE.finditer(body)})
    websockets = [url for url in urls if url.startswith(("ws://", "wss://"))]
    api_paths = [url for url in urls if "/api/" in url or url.startswith("/graphql") or url.endswith(".json")]
    params = sorted({match.group(1) for match in PARAM_RE.finditer(body) if match.group(1)})

    json_dump(
        {
            "status": "ok",
            "source": source,
            "fetch_performed": truthy(args.fetch) and bool(args.input_url),
            "routes": urls[:500],
            "api_paths": api_paths[:500],
            "websocket_urls": websockets[:100],
            "source_maps": sorted(set(SOURCE_MAP_RE.findall(body)))[:100],
            "script_sources": sorted(parser_html.scripts)[:200],
            "forms": parser_html.forms[:200],
            "parameters": params[:300],
            "policy": "Extracted client-side routes are inventory leads. Validate server-side authorization and behavior separately.",
        }
    )


if __name__ == "__main__":
    main_wrapper(main)

