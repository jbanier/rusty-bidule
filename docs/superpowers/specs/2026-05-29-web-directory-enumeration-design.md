# Web Directory Enumeration Skill Design

**Date:** 2026-05-29  
**Status:** Approved  
**Purpose:** Add ffuf and feroxbuster directory enumeration capability to rusty-bidule with safe defaults and integration into the web-app-active-baseline recipe.

## Overview

This design introduces a new `web-directory-enum` skill that provides directory and content discovery using ffuf and feroxbuster. The skill is designed to find hidden or non-referenced paths after initial web application crawling, with built-in safety constraints to prevent service disruption.

### Key Goals

1. Enable directory enumeration as a standalone, ad-hoc capability
2. Integrate smoothly into the existing web-app-active-baseline recipe workflow
3. Enforce safe defaults: 3 threads max, depth 4 max, conservative timeouts
4. Follow established patterns from web-discovery-recon and other web pentest skills
5. Support both ffuf and feroxbuster with intelligent fallback based on availability

## Architecture

### Skill Structure

```
skills/web-directory-enum/
├── SKILL.md                    # Skill metadata and tool definitions
└── scripts/
    └── directory_enum.py       # Python script for command planning
```

### Skill Metadata (SKILL.md)

```yaml
---
name: web-directory-enum
description: Directory and content discovery with ffuf and feroxbuster, using default wordlists with safe threading (3 max) and depth limits (4 max).
metadata:
  keywords: web, directory, enumeration, ffuf, feroxbuster, content-discovery
---

# Web Directory Enumeration

Use this skill to discover hidden directories and non-referenced paths on web applications. 
Runs after initial crawling to find content not linked in the application.

Tools:
  - name: Directory Enumeration Plan
    slug: directory-enum
    description: Validate scope and return safe directory enumeration commands with tool availability.
    script: scripts/directory_enum.py
    network: true
```

## Script Implementation

### Input Parameters

The `directory_enum.py` script accepts:

| Parameter | Required | Default | Description |
|-----------|----------|---------|-------------|
| `--target-url` | Yes | - | Root URL to enumerate (e.g., https://example.com) |
| `--scope-json` | No | - | Path to scope validation JSON file |
| `--allowed-hosts` | No | "" | Comma-separated list of allowed hosts |
| `--active-authorized` | No | "false" | Whether active testing is authorized ("true"/"false") |
| `--threads` | No | 3 | Thread count (max 10, capped internally) |
| `--depth` | No | 4 | Recursion depth (max 6, capped internally) |
| `--wordlist` | No | auto-detect | Custom wordlist path (future-proofing) |

### Output Format

The script outputs JSON with the following structure:

```json
{
  "status": "ok",
  "scope": {
    "allowed_hosts": ["example.com"],
    "active_authorized": true
  },
  "target_url": "https://example.com",
  "tool_availability": {
    "ffuf": {
      "installed": true,
      "version": "2.1.0",
      "path": "/usr/bin/ffuf"
    },
    "feroxbuster": {
      "installed": true,
      "version": "2.10.0",
      "path": "/usr/bin/feroxbuster"
    }
  },
  "wordlist": {
    "path": "/usr/share/wordlists/dirb/common.txt",
    "exists": true,
    "entry_count": 4614
  },
  "commands": [
    {
      "tool": "ffuf",
      "phase": "directory_enumeration",
      "argv": [
        "ffuf",
        "-u", "https://example.com/FUZZ",
        "-w", "/usr/share/wordlists/dirb/common.txt",
        "-t", "3",
        "-recursion",
        "-recursion-depth", "4",
        "-mc", "200,204,301,302,307,401,403",
        "-fc", "404",
        "-timeout", "10",
        "-maxtime", "3600"
      ]
    },
    {
      "tool": "feroxbuster",
      "phase": "directory_enumeration",
      "argv": [
        "feroxbuster",
        "-u", "https://example.com",
        "-w", "/usr/share/wordlists/dirb/common.txt",
        "-t", "3",
        "-d", "4",
        "--auto-bail",
        "--timeout", "10",
        "--time-limit", "1h"
      ]
    }
  ],
  "execution_policy": "Execute ONE command after scope validation. Pick based on tool availability and preference. Respect thread and depth limits.",
  "safety_constraints": {
    "max_threads": 3,
    "max_depth": 4,
    "rate_limit_note": "Tools run with conservative defaults to avoid service disruption"
  }
}
```

### Command Construction Logic

#### ffuf Command

```bash
ffuf \
  -u <target-url>/FUZZ \
  -w <wordlist-path> \
  -t <threads> \
  -recursion \
  -recursion-depth <depth> \
  -mc 200,204,301,302,307,401,403 \
  -fc 404 \
  -timeout 10 \
  -maxtime 3600
```

**Flag rationale:**
- `-u` with `/FUZZ`: Standard fuzzing pattern for directory discovery
- `-w`: Wordlist path (auto-detected or user-provided)
- `-t`: Thread count (default 3, user-adjustable up to 10)
- `-recursion` + `-recursion-depth`: Enable recursive enumeration with depth limit
- `-mc`: Match status codes that indicate interesting findings (success, redirects, auth required, forbidden)
- `-fc 404`: Filter out standard not-found responses
- `-timeout 10`: Per-request timeout to avoid hanging on slow endpoints
- `-maxtime 3600`: Overall scan timeout (1 hour safety limit)

#### feroxbuster Command

```bash
feroxbuster \
  -u <target-url> \
  -w <wordlist-path> \
  -t <threads> \
  -d <depth> \
  --auto-bail \
  --timeout 10 \
  --time-limit 1h
```

**Flag rationale:**
- `-u`: Target URL
- `-w`: Wordlist path
- `-t`: Thread count (default 3, user-adjustable up to 10)
- `-d`: Recursion depth (default 4, user-adjustable up to 6)
- `--auto-bail`: Automatically stop when error patterns are detected (WAF blocks, rate limiting)
- `--timeout 10`: Per-request timeout
- `--time-limit 1h`: Overall scan timeout

### Wordlist Detection Strategy

The script searches for wordlists in this priority order:

1. User-provided `--wordlist` path (if specified)
2. `/usr/share/wordlists/dirb/common.txt` (most common installation)
3. `/usr/share/seclists/Discovery/Web-Content/common.txt` (SecLists)
4. `/usr/share/wordlists/dirbuster/directory-list-2.3-small.txt` (DirBuster)

If no wordlist is found, the script returns an error with installation suggestions.

### Tool Availability Detection

The script uses `shutil.which()` to detect installed tools and attempts to extract version information via:
- `ffuf -V` for ffuf
- `feroxbuster --version` for feroxbuster

If neither tool is installed, the script returns a warning status with installation instructions but still provides command templates.

### Scope Validation

The script leverages the existing `web_assessment_common` module (same as `web-discovery-recon`) to:
- Parse scope from `--scope-json` or construct from `--allowed-hosts` and `--target-url`
- Validate that `target_url` is within the allowed scope
- Check that `active_authorized` is true before returning commands
- Normalize host extraction from URLs

Error conditions:
- Target URL out of scope → return error with violation details
- Active authorization not granted → return error blocking execution
- Invalid URL format → return error with parsing details

### Safety Constraints

**Thread Limiting:**
- Default: 3 threads
- User override: accepted via `--threads`
- Hard cap: 10 threads (enforced in script)
- Rationale: Prevents accidental DDoS, respects target resources

**Depth Limiting:**
- Default: 4 levels
- User override: accepted via `--depth`
- Hard cap: 6 levels (enforced in script)
- Rationale: Prevents infinite recursion, keeps scan bounded

**Timeout Controls:**
- Per-request timeout: 10 seconds (prevents hanging on slow endpoints)
- Overall scan timeout: 1 hour (prevents indefinite runs)

**Auto-Bail (feroxbuster):**
- Enabled by default via `--auto-bail`
- Detects WAF blocks, rate limiting, and error patterns
- Automatically stops scan when defense mechanisms are triggered

## Integration with web-app-active-baseline Recipe

### Recipe Workflow Update

Add a new step after "Crawl inventory" and before "Safe scanner plan":

```yaml
- name: Directory enumeration
  prompt: |
    Activate and run web-directory-enum for validated targets to discover hidden directories and non-referenced paths. Pick ONE enumeration command based on tool availability (prefer feroxbuster if both are available). Run using local__exec_cli with execution_mode managed_job and wait_for_result true. Summarize discovered paths and status codes; keep output under 60 lines. Do not paste full directory listings.
  local_tools:
    - local__activate_skill
    - local__run_skill
    - local__exec_cli
    - local__get_job
    - local__list_jobs
    - local__get_investigation_memory
    - local__update_investigation_memory
    - local__write_file
```

### Recipe RECIPE.md Updates

Update the "Use:" section in `/home/jbanier/Documents/work/rusty-bidule/recipes/web-app-active-baseline/RECIPE.md`:

```markdown
Use:
- `web-crawler-inventory` for bounded same-scope crawling,
- `web-directory-enum` for directory and content discovery with ffuf or feroxbuster,
- `web-discovery-recon` for tool availability and command planning,
- `web-scanner-safe` for conservative nuclei/ZAP baseline plans.
```

### Execution Flow

1. **Crawl inventory** runs first (existing step) → discovers linked paths
2. **Directory enumeration** runs second (new step) → discovers hidden/unlinked paths
3. **Safe scanner plan** runs third (existing step) → uses combined path inventory

This ordering ensures maximum path coverage before vulnerability scanning.

## Implementation Details

### Script Structure (directory_enum.py)

```python
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
    target_url = require_url_in_scope(args.target_url, scope)
    
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
    if tools.get("ffuf", {}).get("installed"):
        commands.append({
            "tool": "ffuf",
            "phase": "directory_enumeration",
            "argv": build_ffuf_command(target_url, wordlist, threads, depth)
        })
    
    if tools.get("feroxbuster", {}).get("installed"):
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
```

### Shared Module Dependencies

The script depends on `_web_pentest_common/web_assessment_common.py` for:
- `json_dump()`: Safe JSON output with error handling
- `main_wrapper()`: Exception handling and error formatting
- `normalize_host()`: Extract hostname from URL
- `require_url_in_scope()`: Scope validation with error on violation
- `scope_from_args()`: Parse and construct scope object
- `tool_status()`: Check tool installation and version

These utilities are already implemented and used by other web pentest skills.

## Error Handling

### Script-Level Errors

| Condition | Behavior |
|-----------|----------|
| Target URL out of scope | Return `{"status": "error", "error": "Target URL out of scope: ..."}` |
| Active authorization not granted | Return `{"status": "error", "error": "Active testing not authorized"}` |
| Wordlist not found | Return error with installation suggestions |
| Neither tool installed | Return warning with installation instructions but include command templates |
| Invalid URL format | Return error with parsing details |
| Invalid thread/depth values | Clamp to safe limits (don't error) |

### Runtime Errors (Tool Execution)

These are handled by the recipe's managed job execution:
- Tool exits with non-zero status → captured in job result
- Timeout exceeded → job terminated, status captured
- WAF/rate limiting detected (feroxbuster auto-bail) → clean exit with partial results
- Network errors → captured in stderr

The recipe should summarize failures without exposing full error output.

## Allowed Commands Verification

Both `ffuf` and `feroxbuster` are already present in the default allowed CLI tools list in `src/config.rs`:

```rust
fn default_allowed_cli_tools() -> Vec<String> {
    [
        // ... other tools ...
        "ffuf",
        "feroxbuster",
        // ... other tools ...
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}
```

No configuration changes are required. The tools are immediately available to `local__exec_cli`.

## Testing Strategy

### Unit Testing

Test the Python script with:
- Valid scope and tool availability → returns proper command structure
- Out-of-scope URL → returns error
- Missing wordlist → returns error with suggestions
- Thread/depth overrides → respects caps
- Custom wordlist path → uses provided path

### Integration Testing

Test the skill integration:
1. Run skill activation from a recipe
2. Verify command output structure
3. Execute ffuf command via `local__exec_cli` with managed_job mode
4. Execute feroxbuster command via `local__exec_cli` with managed_job mode
5. Verify results are captured and summarized correctly

### End-to-End Testing

Test the full web-app-active-baseline recipe:
1. Run crawl inventory step
2. Run directory enumeration step (new)
3. Verify discovered paths from both steps are available to scanner plan
4. Confirm no service disruption due to aggressive scanning

## Future Enhancements

### Custom Wordlist Management

Add support for:
- Project-specific wordlists stored in `var/wordlists/`
- Multiple wordlist tiers (small, medium, large)
- Technology-specific wordlists (e.g., WordPress, API endpoints)

Implementation would add `--wordlist-size` parameter with "small", "medium", "large" options.

### Output Parsing and Filtering

Add a companion script to:
- Parse ffuf/feroxbuster JSON output
- Filter results by status code, size, word count
- Deduplicate findings across multiple runs
- Format as structured evidence for reporting

### Status Code Analysis

Enhance the output to include:
- Categorize findings by status code (success, redirects, auth, forbidden)
- Flag interesting patterns (401/403 on admin paths, 200 on backup files)
- Compare against baseline to identify new paths

### Integration with Coverage Tracking

Connect to `web-coverage-status` skill to:
- Track which paths have been enumerated
- Avoid duplicate enumeration across recipe runs
- Identify coverage gaps

## Deliverables

1. **Skill definition:** `skills/web-directory-enum/SKILL.md`
2. **Script implementation:** `skills/web-directory-enum/scripts/directory_enum.py`
3. **Recipe update:** Modified `recipes/web-app-active-baseline/RECIPE.md` with new step
4. **Test scripts:** Unit tests for directory_enum.py
5. **Documentation:** This design document

## Success Criteria

- [ ] Skill can be invoked standalone with valid scope
- [ ] Skill integrates into web-app-active-baseline recipe workflow
- [ ] ffuf and feroxbuster commands execute with safe defaults
- [ ] Thread and depth limits are enforced
- [ ] Scope validation prevents out-of-scope enumeration
- [ ] Results are captured and summarized without overwhelming output
- [ ] No service disruption during test scans
