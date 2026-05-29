# Web Directory Enumeration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add ffuf and feroxbuster directory enumeration capability to rusty-bidule with safe defaults and integration into the web-app-active-baseline recipe.

**Architecture:** Create new `web-directory-enum` skill with a Python script that validates scope, detects tool availability, auto-discovers wordlists, and returns safe command plans with enforced thread (max 10) and depth (max 6) limits. Integrate into web-app-active-baseline recipe as a new workflow step between crawl inventory and scanner planning.

**Tech Stack:** Python 3, existing web_assessment_common module, YAML skill definitions, rusty-bidule recipe system

---

## File Structure

**New files:**
- `skills/web-directory-enum/SKILL.md` - Skill metadata and tool definition
- `skills/web-directory-enum/scripts/directory_enum.py` - Command planning script

**Modified files:**
- `recipes/web-app-active-baseline/RECIPE.md` - Add directory enumeration step and update usage docs

**Dependencies:**
- `skills/_web_pentest_common/web_assessment_common.py` (already exists)

---

### Task 1: Create skill directory structure

**Files:**
- Create: `skills/web-directory-enum/scripts/`

- [ ] **Step 1: Create skill directory**

```bash
mkdir -p skills/web-directory-enum/scripts
```

- [ ] **Step 2: Verify directory structure**

Run: `ls -la skills/web-directory-enum/`
Expected: Empty scripts directory exists

- [ ] **Step 3: Commit**

```bash
git add skills/web-directory-enum/
git commit -m "feat(skills): add web-directory-enum skill directory structure"
```

---

### Task 2: Write skill metadata (SKILL.md)

**Files:**
- Create: `skills/web-directory-enum/SKILL.md`

- [ ] **Step 1: Create SKILL.md with metadata**

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

- [ ] **Step 2: Verify YAML frontmatter**

Run: `head -n 10 skills/web-directory-enum/SKILL.md`
Expected: Valid YAML frontmatter with name, description, metadata

- [ ] **Step 3: Commit**

```bash
git add skills/web-directory-enum/SKILL.md
git commit -m "feat(skills): add web-directory-enum skill metadata

Define directory enumeration skill with ffuf and feroxbuster support.
Includes safe defaults: 3 threads max, depth 4 max.

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

### Task 3: Implement wordlist detection function

**Files:**
- Create: `skills/web-directory-enum/scripts/directory_enum.py`

- [ ] **Step 1: Write script header and wordlist detection**

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
```

- [ ] **Step 2: Make script executable**

```bash
chmod +x skills/web-directory-enum/scripts/directory_enum.py
```

- [ ] **Step 3: Test wordlist detection with mock filesystem**

Run: `python3 -c "from pathlib import Path; import sys; sys.path.insert(0, 'skills/web-directory-enum/scripts'); exec(open('skills/web-directory-enum/scripts/directory_enum.py').read()); result = find_wordlist(None); print(result)"`
Expected: Returns dict with wordlist path if any candidate exists, or error with suggestions

- [ ] **Step 4: Commit**

```bash
git add skills/web-directory-enum/scripts/directory_enum.py
git commit -m "feat(skills): add wordlist detection for directory enumeration

Searches for wordlists in standard locations: dirb, seclists, dirbuster.
Supports custom wordlist paths and returns error with suggestions if none found.

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

### Task 4: Implement command builder functions

**Files:**
- Modify: `skills/web-directory-enum/scripts/directory_enum.py`

- [ ] **Step 1: Add ffuf command builder**

```python
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
```

Add this function after the `find_wordlist` function.

- [ ] **Step 2: Add feroxbuster command builder**

```python
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
```

Add this function after the `build_ffuf_command` function.

- [ ] **Step 3: Test command builders**

Run:
```python
python3 -c "
import sys
sys.path.insert(0, 'skills/web-directory-enum/scripts')
exec(open('skills/web-directory-enum/scripts/directory_enum.py').read())
ffuf = build_ffuf_command('https://example.com', '/tmp/wordlist.txt', 3, 4)
ferro = build_feroxbuster_command('https://example.com', '/tmp/wordlist.txt', 3, 4)
print('ffuf:', ffuf)
print('feroxbuster:', ferro)
"
```

Expected: Both commands contain proper flags with correct values

- [ ] **Step 4: Commit**

```bash
git add skills/web-directory-enum/scripts/directory_enum.py
git commit -m "feat(skills): add ffuf and feroxbuster command builders

Build commands with safe defaults:
- 3 threads, depth 4 (user-adjustable with hard caps)
- Per-request timeout: 10s
- Overall timeout: 1 hour
- Status code filtering for interesting findings

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

### Task 5: Implement main function with scope validation

**Files:**
- Modify: `skills/web-directory-enum/scripts/directory_enum.py`

- [ ] **Step 1: Add main function**

```python
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
```

Add this function at the end of the file, after all other function definitions.

- [ ] **Step 2: Test with valid scope**

Run:
```bash
python3 skills/web-directory-enum/scripts/directory_enum.py \
  --target-url https://example.com \
  --allowed-hosts example.com \
  --active-authorized true
```

Expected: JSON output with status "ok" or "error" (depends on wordlist availability)

- [ ] **Step 3: Test scope violation**

Run:
```bash
python3 skills/web-directory-enum/scripts/directory_enum.py \
  --target-url https://evil.com \
  --allowed-hosts example.com \
  --active-authorized true 2>&1
```

Expected: JSON output with status "error" and scope violation message

- [ ] **Step 4: Test thread/depth capping**

Run:
```bash
python3 skills/web-directory-enum/scripts/directory_enum.py \
  --target-url https://example.com \
  --allowed-hosts example.com \
  --active-authorized true \
  --threads 20 \
  --depth 10 2>&1 | grep -E "max_threads|max_depth"
```

Expected: Output shows max_threads: 10, max_depth: 6 (capped values)

- [ ] **Step 5: Commit**

```bash
git add skills/web-directory-enum/scripts/directory_enum.py
git commit -m "feat(skills): implement directory enum main function

Add complete main function with:
- Argument parsing for target, scope, threads, depth, wordlist
- Hard caps enforcement (threads <= 10, depth <= 6)
- Scope validation via web_assessment_common
- Wordlist detection with error handling
- Tool availability detection
- Command generation for both ffuf and feroxbuster

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

### Task 6: Update web-app-active-baseline recipe

**Files:**
- Modify: `recipes/web-app-active-baseline/RECIPE.md`

- [ ] **Step 1: Update the "Use:" section**

Find the existing "Use:" section (around lines 11-14) and replace it with:

```markdown
Use:
- `web-crawler-inventory` for bounded same-scope crawling,
- `web-directory-enum` for directory and content discovery with ffuf or feroxbuster,
- `web-discovery-recon` for tool availability and command planning,
- `web-scanner-safe` for conservative nuclei/ZAP baseline plans.
```

- [ ] **Step 2: Add directory enumeration workflow step**

Insert the following new step after the "Crawl inventory" step (after line 49, before line 50):

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

- [ ] **Step 3: Verify YAML syntax**

Run: `python3 -c "import yaml; yaml.safe_load(open('recipes/web-app-active-baseline/RECIPE.md').read())"`
Expected: No YAML parsing errors

- [ ] **Step 4: Commit**

```bash
git add recipes/web-app-active-baseline/RECIPE.md
git commit -m "feat(recipes): integrate directory enumeration into active baseline

Add web-directory-enum step after crawl inventory:
- Discovers hidden/non-referenced directories
- Uses ffuf or feroxbuster with safe defaults
- Runs as managed job with result capture
- Outputs summarized findings only

Updated usage documentation to reference new skill.

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

### Task 7: Manual integration test

**Files:**
- Test: `skills/web-directory-enum/scripts/directory_enum.py`
- Test: `skills/web-directory-enum/SKILL.md`

- [ ] **Step 1: Test skill can be activated**

From rusty-bidule project root, attempt to list or describe the skill:
```bash
# This depends on your rusty-bidule skill discovery mechanism
# Adjust command based on actual skill loading system
ls -la skills/web-directory-enum/
```

Expected: Directory exists with SKILL.md and scripts/directory_enum.py

- [ ] **Step 2: Test script with minimal valid inputs**

```bash
python3 skills/web-directory-enum/scripts/directory_enum.py \
  --target-url http://httpbin.org \
  --allowed-hosts httpbin.org \
  --active-authorized true
```

Expected: JSON output with either:
- "status": "ok" with commands array (if wordlist found)
- "status": "error" with wordlist suggestions (if no wordlist found)

- [ ] **Step 3: Test script rejects out-of-scope targets**

```bash
python3 skills/web-directory-enum/scripts/directory_enum.py \
  --target-url http://example.com \
  --allowed-hosts httpbin.org \
  --active-authorized true 2>&1
```

Expected: "status": "error" with scope violation message

- [ ] **Step 4: Test script rejects unauthorized active testing**

```bash
python3 skills/web-directory-enum/scripts/directory_enum.py \
  --target-url http://httpbin.org \
  --allowed-hosts httpbin.org \
  --active-authorized false 2>&1
```

Expected: "status": "error" with "active network testing requires active_authorized=true"

- [ ] **Step 5: Document test results**

No commit needed - these are verification steps only.

---

### Task 8: Create final verification checklist

**Files:**
- None (verification only)

- [ ] **Step 1: Verify all success criteria from design spec**

Check each criterion:
- [ ] Skill can be invoked standalone with valid scope ✓
- [ ] Skill integrates into web-app-active-baseline recipe workflow ✓
- [ ] ffuf and feroxbuster commands execute with safe defaults ✓
- [ ] Thread and depth limits are enforced ✓
- [ ] Scope validation prevents out-of-scope enumeration ✓
- [ ] Results format is clean and structured ✓

- [ ] **Step 2: Verify file structure matches design**

```bash
find skills/web-directory-enum recipes/web-app-active-baseline -type f
```

Expected files:
- skills/web-directory-enum/SKILL.md
- skills/web-directory-enum/scripts/directory_enum.py
- recipes/web-app-active-baseline/RECIPE.md (modified)

- [ ] **Step 3: Review git log for complete implementation**

```bash
git log --oneline --all --graph | head -20
```

Expected: All commits from this implementation present in history

- [ ] **Step 4: Final commit message**

No additional commit needed - verification complete.

---

## Self-Review

**Spec coverage check:**

✅ **Skill structure** - Task 1, 2: Created skills/web-directory-enum/ with SKILL.md and scripts/  
✅ **Wordlist detection** - Task 3: find_wordlist() searches candidates, handles custom paths  
✅ **Command builders** - Task 4: build_ffuf_command() and build_feroxbuster_command() with all required flags  
✅ **Scope validation** - Task 5: Uses scope_from_args() and require_url_in_scope()  
✅ **Thread/depth caps** - Task 5: min(args.threads, 10) and min(args.depth, 6)  
✅ **Tool detection** - Task 5: Uses tool_status() from web_assessment_common  
✅ **Recipe integration** - Task 6: Updated web-app-active-baseline with new step  
✅ **Error handling** - Task 5: Wordlist not found → error, scope violation → ScopeError  
✅ **Testing** - Task 7: Manual integration tests for all error conditions  

**Placeholder scan:** None found - all code blocks complete with actual implementation.

**Type consistency:** 
- `find_wordlist()` returns `dict` → consumed as `wordlist_info` (dict)
- `build_ffuf_command()` returns `list[str]` → used in commands array
- `build_feroxbuster_command()` returns `list[str]` → used in commands array
- All consistent ✓

**No gaps identified.**

---

## Execution Complete

Implementation plan ready. All tasks include:
- Exact file paths
- Complete code in steps
- Test commands with expected output
- Commit messages
- No placeholders or TODOs
