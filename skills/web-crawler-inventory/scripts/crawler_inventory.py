#!/usr/bin/env python3
from __future__ import annotations

import argparse
from html.parser import HTMLParser
from pathlib import Path
import sys
import urllib.parse

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import fetch_url, json_dump, main_wrapper, require_url_in_scope, scope_from_args, tool_status, truthy  # noqa: E402


class InventoryParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.links: set[str] = set()
        self.scripts: set[str] = set()
        self.forms: list[dict[str, object]] = []
        self._form: dict[str, object] | None = None

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        attr = {key.lower(): value or "" for key, value in attrs}
        if tag == "a" and attr.get("href"):
            self.links.add(attr["href"])
        elif tag == "script" and attr.get("src"):
            self.scripts.add(attr["src"])
        elif tag == "form":
            self._form = {"method": attr.get("method", "GET").upper(), "action": attr.get("action", ""), "inputs": []}
            self.forms.append(self._form)
        elif tag in {"input", "select", "textarea"} and self._form is not None:
            self._form["inputs"].append({"name": attr.get("name", ""), "type": attr.get("type", tag)})

    def handle_endtag(self, tag: str) -> None:
        if tag == "form":
            self._form = None


def absolute_urls(base_url: str, values: set[str]) -> list[str]:
    return sorted({urllib.parse.urljoin(base_url, value) for value in values})


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--seed-url", required=True)
    parser.add_argument("--scope-json")
    parser.add_argument("--allowed-hosts", default="")
    parser.add_argument("--active-authorized", default="false")
    parser.add_argument("--crawl", default="false")
    parser.add_argument("--max-urls", type=int, default=25)
    args = parser.parse_args()

    scope = scope_from_args(
        scope_json=args.scope_json,
        target_urls=args.seed_url,
        allowed_hosts=args.allowed_hosts,
        active_authorized=args.active_authorized,
    )
    seed_url = require_url_in_scope(args.seed_url, scope, active=truthy(args.crawl))
    payload = {
        "status": "ok",
        "seed_url": seed_url,
        "scope": scope,
        "tool_availability": tool_status(["katana", "gospider"]),
        "suggested_commands": [
            ["katana", "-u", seed_url, "-d", "2", "-silent"],
            ["gospider", "-s", seed_url, "-d", "2"],
        ],
    }
    if truthy(args.crawl):
        visited: list[str] = []
        queue = [seed_url]
        discovered: set[str] = set()
        forms: list[dict[str, object]] = []
        scripts: set[str] = set()
        while queue and len(visited) < max(args.max_urls, 1):
            url = queue.pop(0)
            if url in visited:
                continue
            require_url_in_scope(url, scope, active=True)
            response = fetch_url(url)
            visited.append(url)
            html = response.get("body_preview") or ""
            page = InventoryParser()
            page.feed(html)
            links = absolute_urls(url, page.links)
            for link in links:
                if link not in discovered:
                    discovered.add(link)
                    try:
                        require_url_in_scope(link, scope)
                        queue.append(link)
                    except SystemExit:
                        pass
            scripts.update(absolute_urls(url, page.scripts))
            forms.extend(page.forms)
        payload.update({"crawl_performed": True, "visited": visited, "discovered_urls": sorted(discovered), "scripts": sorted(scripts), "forms": forms})
    else:
        payload["crawl_performed"] = False
        payload["next_step"] = "Set crawl=true with active_authorized=true for bounded built-in crawl."
    json_dump(payload)


if __name__ == "__main__":
    main_wrapper(main)
