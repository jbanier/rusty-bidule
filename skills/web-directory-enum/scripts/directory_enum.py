#!/usr/bin/env python3
from __future__ import annotations

import argparse
import shutil
import subprocess
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


WORDLIST_CANDIDATES = [
    "/usr/share/wordlists/dirb/common.txt",
    "/usr/share/seclists/Discovery/Web-Content/common.txt",
    "/usr/share/wordlists/dirbuster/directory-list-2.3-small.txt",
]


def find_wordlist(custom_path: str | None) -> dict:
    """Find an available wordlist, preferring custom path if provided."""
    if custom_path:
        path = Path(custom_path)
        if path.exists():
            return {"path": str(path), "exists": True}
        return {"path": custom_path, "exists": False, "error": "Custom wordlist not found"}

    for candidate in WORDLIST_CANDIDATES:
        path = Path(candidate)
        if path.exists():
            return {"path": str(path), "exists": True}

    return {
        "path": None,
        "exists": False,
        "error": "No default wordlist found. Install dirb, seclists, or dirbuster wordlists.",
        "suggestions": WORDLIST_CANDIDATES
    }


def build_ffuf_command(target_url: str, wordlist: str, threads: int, depth: int) -> list[str]:
    """Build ffuf command with safe defaults."""
    return [
        "ffuf",
        "-u", f"{target_url}/FUZZ",
        "-w", wordlist,
        "-t", str(threads),
        "-recursion",
        "-recursion-depth", str(depth),
        "-mc", "200,204,301,302,307,401,403",
        "-fc", "404",
        "-timeout", "10",
        "-maxtime", "3600",
    ]


def build_feroxbuster_command(target_url: str, wordlist: str, threads: int, depth: int) -> list[str]:
    """Build feroxbuster command with safe defaults."""
    return [
        "feroxbuster",
        "-u", target_url,
        "-w", wordlist,
        "-t", str(threads),
        "-d", str(depth),
        "--auto-bail",
        "--timeout", "10",
        "--time-limit", "1h",
    ]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target-url", required=True)
    parser.add_argument("--scope-json")
    parser.add_argument("--allowed-hosts", default="")
    parser.add_argument("--active-authorized", default="false")
    parser.add_argument("--threads", type=int, default=3)
    parser.add_argument("--depth", type=int, default=4)
    parser.add_argument("--wordlist")
    args = parser.parse_args()

    # Enforce hard caps
    threads = min(args.threads, 10)
    depth = min(args.depth, 6)

    # Validate scope
    scope = scope_from_args(
        scope_json=args.scope_json,
        target_urls=args.target_url,
        allowed_hosts=args.allowed_hosts,
        active_authorized=args.active_authorized,
    )
    target_url = require_url_in_scope(args.target_url, scope, active=True)

    # Find wordlist
    wordlist_info = find_wordlist(args.wordlist)
    if not wordlist_info["exists"]:
        json_dump({
            "status": "error",
            "error": wordlist_info.get("error", "Wordlist not found"),
            "suggestions": wordlist_info.get("suggestions", [])
        })
        return

    wordlist = wordlist_info["path"]

    # Check tool availability
    tools = tool_status(["ffuf", "feroxbuster"])

    # Build commands
    commands = []
    if tools.get("ffuf", {}).get("available"):
        commands.append({
            "tool": "ffuf",
            "phase": "directory_enumeration",
            "argv": build_ffuf_command(target_url, wordlist, threads, depth)
        })

    if tools.get("feroxbuster", {}).get("available"):
        commands.append({
            "tool": "feroxbuster",
            "phase": "directory_enumeration",
            "argv": build_feroxbuster_command(target_url, wordlist, threads, depth)
        })

    json_dump({
        "status": "ok",
        "scope": scope,
        "target_url": target_url,
        "tool_availability": tools,
        "wordlist": wordlist_info,
        "commands": commands,
        "execution_policy": "Execute ONE command after scope validation. Pick based on tool availability and preference. Respect thread and depth limits.",
        "safety_constraints": {
            "max_threads": threads,
            "max_depth": depth,
            "rate_limit_note": "Tools run with conservative defaults to avoid service disruption"
        }
    })


if __name__ == "__main__":
    main_wrapper(main)
