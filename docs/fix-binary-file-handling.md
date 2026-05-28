# Fix: Binary File Handling Guidance

## Problem

The agent was unnecessarily encoding binary files (like zip files) to base64/hex before copying or extracting them, instead of using direct shell commands like `cp` or `unzip`.

## Root Cause

The tool descriptions for `local__read_file` and `local__write_file` didn't explicitly guide agents away from using text/hex encoding for binary file operations. The descriptions emphasized that files could be read/written as "text or hex", which led agents to interpret this as the primary method for ALL file operations, including copying zip files.

## Solution

Updated the tool descriptions in `src/local_tools.rs` and system prompt guidance in `src/orchestrator.rs` to explicitly state:

### For `local__read_file`:
```
Use this ONLY when you need to inspect or modify file content within the LLM context. 
For binary files (zip, tar, images, etc.), use local__exec_cli with commands like cp, 
unzip, tar instead - do not read/write binary files through text/hex encoding.
```

### For `local__write_file`:
```
Use this ONLY for text files you need to create/edit, or small binary files where you 
need programmatic generation. For copying or manipulating binary files (zip, tar, images, 
executables), use local__exec_cli with cp, mv, unzip, tar, etc. instead - do not 
encode/decode binary files unnecessarily.
```

### System prompt guidance:
```
Use `local__list_directory`, `local__read_file`, and `local__write_file` for 
inspecting/editing text files. For binary files (zip, tar, images), use `local__exec_cli` 
with shell commands like cp, mv, unzip, tar - never encode/decode binary files through 
hex unnecessarily.
```

## Changes Made

1. **src/local_tools.rs** (lines ~4584 and ~4600): Updated `local__read_file` and `local__write_file` descriptions
2. **src/orchestrator.rs** (line ~2107): Updated system prompt guidance for filesystem operations

## Expected Behavior After Fix

When the agent needs to work with a zip file, it should now:
- Use `local__exec_cli` with `cp` to copy the file
- Use `local__exec_cli` with `unzip` to extract it
- NOT use `local__read_file` with hex encoding followed by `local__write_file` with hex decoding

## Testing

Compile check passed successfully:
```bash
cargo check
# Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.91s
```

The agent behavior should now prefer direct shell commands for binary file operations, significantly improving performance and reducing context usage.
