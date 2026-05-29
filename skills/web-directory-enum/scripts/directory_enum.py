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
