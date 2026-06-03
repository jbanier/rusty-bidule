#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import sys

# Import shared utilities from web_assessment_common
SHARED_DIR = Path(__file__).resolve().parents[2] / "_web_pentest_common"
sys.path.insert(0, str(SHARED_DIR))

from web_assessment_common import (
    json_dump,
    main_wrapper,
    normalize_host,
    require_url_in_scope,
    scope_from_args,
    tool_status
)


# Common parameter wordlist (top parameters seen in web apps)
COMMON_PARAMETERS = [
    "id", "page", "limit", "offset", "sort", "order",
    "debug", "test", "dev", "admin", "user", "username",
    "token", "key", "secret", "api_key", "access_token",
    "callback", "redirect", "url", "return", "next",
    "search", "q", "query", "filter", "category",
    "lang", "locale", "format", "output", "view",
    "action", "method", "function", "cmd", "command"
]


def build_arjun_command(url: str, method: str, threads: int, wordlist: str | None = None) -> list[str]:
    """
    Build arjun command with safe defaults.

    arjun is a parameter discovery tool that uses response analysis
    to identify hidden parameters.
    """
    cmd = [
        "arjun",
        "-u", url,
        "-m", method.upper(),
        "-t", str(threads),
        "--stable",  # Use stable mode for more reliable detection
    ]

    if wordlist:
        cmd.extend(["-w", wordlist])

    return cmd


def build_parameth_command(url: str, method: str, wordlist: str | None = None) -> list[str]:
    """
    Build parameth command for parameter discovery.

    parameth brute-forces GET and POST parameters.
    """
    cmd = [
        "parameth",
        "-u", url,
        "-m", method.upper(),
    ]

    if wordlist:
        cmd.extend(["-w", wordlist])

    return cmd


def find_parameter_wordlist(custom_path: str | None) -> dict:
    """Find available parameter wordlist."""
    if custom_path:
        path = Path(custom_path)
        if path.exists():
            return {"path": str(path), "exists": True}
        return {"path": custom_path, "exists": False, "error": "Custom wordlist not found"}

    # Check for common parameter wordlist locations
    candidates = [
        "/usr/share/wordlists/seclists/Discovery/Web-Content/burp-parameter-names.txt",
        "/usr/share/seclists/Discovery/Web-Content/raft-medium-words.txt",
        "/usr/share/wordlists/dirb/common.txt",
    ]

    for candidate in candidates:
        path = Path(candidate)
        if path.exists():
            return {"path": str(path), "exists": True}

    # No wordlist found - can use built-in common parameters
    return {
        "path": None,
        "exists": False,
        "note": "No wordlist found. Tools will use built-in parameter lists.",
        "builtin_parameters": COMMON_PARAMETERS
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target-url", required=True, help="Target URL to test")
    parser.add_argument("--method", default="GET", choices=["GET", "POST"], help="HTTP method")
    parser.add_argument("--scope-json")
    parser.add_argument("--allowed-hosts", default="")
    parser.add_argument("--active-authorized", default="false")
    parser.add_argument("--threads", type=int, default=5, help="Number of threads (max 10)")
    parser.add_argument("--wordlist", help="Custom parameter wordlist path")
    args = parser.parse_args()

    # Enforce thread cap
    threads = min(args.threads, 10)

    # Validate scope
    scope = scope_from_args(
        scope_json=args.scope_json,
        target_urls=args.target_url,
        allowed_hosts=args.allowed_hosts,
        active_authorized=args.active_authorized,
    )
    target_url = require_url_in_scope(args.target_url, scope, active=True)

    # Check tool availability
    tools = tool_status(["arjun", "parameth"])

    # Find wordlist
    wordlist_info = find_parameter_wordlist(args.wordlist)

    # Build commands
    commands = []

    wordlist_path = wordlist_info.get("path") if wordlist_info.get("exists") else None

    if tools.get("arjun", {}).get("available"):
        commands.append({
            "tool": "arjun",
            "phase": "parameter_discovery",
            "argv": build_arjun_command(target_url, args.method, threads, wordlist_path),
            "description": "Response-based parameter discovery with arjun"
        })

    if tools.get("parameth", {}).get("available"):
        commands.append({
            "tool": "parameth",
            "phase": "parameter_discovery",
            "argv": build_parameth_command(target_url, args.method, wordlist_path),
            "description": "Parameter brute-forcing with parameth"
        })

    json_dump({
        "status": "ok",
        "target_url": target_url,
        "method": args.method.upper(),
        "scope": scope,
        "tool_availability": tools,
        "wordlist": wordlist_info,
        "commands": commands,
        "execution_policy": "Execute ONE command via local__exec_cli with managed_job mode. Parse output to extract discovered parameters. Both tools use response analysis.",
        "safety_constraints": {
            "max_threads": threads,
            "timeout_seconds": 30,
            "note": "Parameter fuzzing can generate significant traffic. Use conservative thread counts."
        },
        "analysis_guidance": {
            "validation_steps": [
                "1. Verify parameter actually changes response (not false positive)",
                "2. Test parameter with different values",
                "3. Check if parameter affects application logic",
                "4. Look for security implications (debug mode, admin access)"
            ],
            "interesting_parameters": [
                "debug - May enable verbose errors or debug mode",
                "admin - May expose admin functionality",
                "key/token - May bypass authentication",
                "callback/redirect - May enable open redirect",
                "id - May enable IDOR/parameter tampering"
            ],
            "common_false_positives": [
                "Parameters that appear in error messages but don't affect behavior",
                "Cache-busting parameters that don't change logic",
                "Analytics/tracking parameters"
            ]
        }
    })


if __name__ == "__main__":
    main_wrapper(main)
