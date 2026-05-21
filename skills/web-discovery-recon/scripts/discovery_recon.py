#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import sys

SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import json_dump, main_wrapper, normalize_host, require_url_in_scope, scope_from_args, tool_status  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target-url", required=True)
    parser.add_argument("--scope-json")
    parser.add_argument("--allowed-hosts", default="")
    parser.add_argument("--active-authorized", default="false")
    parser.add_argument("--ports", default="80,443")
    args = parser.parse_args()

    scope = scope_from_args(
        scope_json=args.scope_json,
        target_urls=args.target_url,
        allowed_hosts=args.allowed_hosts,
        active_authorized=args.active_authorized,
    )
    target_url = require_url_in_scope(args.target_url, scope)
    host = normalize_host(target_url)
    commands = [
        {"phase": "technology_fingerprint", "argv": ["httpx", "-u", target_url, "-title", "-tech-detect", "-status-code", "-follow-redirects"]},
        {"phase": "waf_detection", "argv": ["wafw00f", target_url]},
        {"phase": "tls_posture", "argv": ["testssl.sh", "--fast", target_url]},
        {"phase": "port_baseline", "argv": ["nmap", "-sT", "-sV", "-p", args.ports, host]},
        {"phase": "subdomain_passive", "argv": ["subfinder", "-d", host, "-silent"]},
        {"phase": "dns_resolution", "argv": ["dnsx", "-silent"]},
        {"phase": "safe_port_discovery", "argv": ["naabu", "-host", host, "-p", args.ports]},
    ]
    json_dump(
        {
            "status": "ok",
            "scope": scope,
            "target_url": target_url,
            "tool_availability": tool_status(["subfinder", "dnsx", "httpx", "naabu", "nmap", "wafw00f", "testssl.sh"]),
            "commands": commands,
            "execution_policy": "Command plan only. Execute manually or through allowed local CLI after confirming scope, rate limits, and tool availability.",
        }
    )


if __name__ == "__main__":
    main_wrapper(main)
