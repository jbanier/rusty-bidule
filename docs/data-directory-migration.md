# Data Directory Migration Guide

## Overview

Starting with this version, `rusty-bidule` now defaults to storing all agent working files in `~/.rusty/` instead of the project-local `data/` directory. This change provides better separation between the project codebase and agent-generated data.

## New Directory Structure

When you run the agent, files are now organized under `~/.rusty/`:

```
~/.rusty/
├── conversations/     # Conversation history and message logs
├── oauth/            # OAuth tokens for MCP server authentication
├── audit.jsonl       # Audit log of all agent actions
└── ...               # Tool output files (e.g., scan results, reports)
```

## Configuration

### Default Behavior

If you omit `data_dir` from your config file, it will default to `~/.rusty`:

```yaml
# config/config.local.yaml
llm_provider: azure_anthropic
azure_anthropic:
  api_key: env:AZURE_ANTHROPIC_API_KEY
  # ... other settings
# data_dir is omitted - defaults to ~/.rusty
```

### Custom Location

You can specify a custom location using an absolute path or tilde expansion:

```yaml
# Use a custom location in home directory
data_dir: ~/my-agent-workspace

# Use an absolute path
data_dir: /var/rusty-bidule/data

# Use project-local storage (old behavior)
data_dir: data
```

### Path Expansion

The configuration system now supports tilde (`~`) expansion in paths:

- `~/.rusty` expands to `/home/username/.rusty` on Unix
- `~/.rusty` expands to `C:\Users\username\.rusty` on Windows

## Migration Steps

If you have existing data in the project-local `data/` directory and want to migrate it:

### Option 1: Continue Using Project-Local Storage

Keep your existing setup by explicitly setting `data_dir: data` in your config:

```yaml
data_dir: data  # Keeps using ./data/ relative to project root
```

### Option 2: Migrate to ~/.rusty

1. Stop any running agent instances
2. Copy your existing data:
   ```bash
   mkdir -p ~/.rusty
   cp -r data/* ~/.rusty/
   ```
3. Update your config to use the new default:
   ```yaml
   data_dir: ~/.rusty  # Or omit entirely for same effect
   ```
4. Test that conversations are accessible
5. (Optional) Remove the old `data/` directory

### Option 3: Fresh Start

Simply update your config and let the agent create new directories:

```yaml
# Omit data_dir or set it explicitly
data_dir: ~/.rusty
```

The agent will automatically create the directory structure on first run.

## Benefits of the New Default

1. **Separation of Concerns**: Project code and agent data are kept separate
2. **Consistent Location**: Data is in a predictable location regardless of where you run the agent
3. **Multiple Projects**: Run agents in different project directories while sharing conversation history
4. **Cleaner Git**: Agent-generated data doesn't clutter your project workspace
5. **Standard Practice**: Follows Unix conventions for user-specific application data

## Implementation Details

### Code Changes

The following components were updated:

1. **`src/config.rs`**:
   - Added `expand_paths()` method to handle tilde expansion
   - Updated `data_dir()` to default to `~/.rusty` when omitted
   - Added `shellexpand` dependency for path expansion

2. **Configuration Examples**:
   - Updated `config/config.example.yaml` with new default
   - Updated `config/config.local.yaml` with documentation

3. **Documentation**:
   - Updated `README.md` with data directory section
   - Added this migration guide

### Test Coverage

New tests verify:
- Tilde expansion works correctly
- Default path resolves to `~/.rusty`
- Explicit paths are honored

Run tests with:
```bash
cargo test expands_tilde defaults_to_home
```

## Troubleshooting

### Permission Issues

If you encounter permission errors:
```bash
chmod 700 ~/.rusty
```

### Disk Space

The agent stores conversation history, OAuth tokens, and tool output. Monitor disk usage:
```bash
du -sh ~/.rusty
```

### Missing Conversations

If conversations don't appear after migration, verify:
1. Data was copied correctly: `ls -la ~/.rusty/conversations/`
2. Config points to the right location: check `data_dir` setting
3. Directory permissions allow read/write access

## Rollback

To revert to the old behavior temporarily:

1. Set `data_dir: data` in your config
2. Copy data back if needed: `cp -r ~/.rusty/* data/`
3. Restart the agent

## Questions?

For issues or questions about this migration, check:
- [Project README](../README.md)
- [Example Config](../config/config.example.yaml)
- [GitHub Issues](https://github.com/yourusername/rusty-bidule/issues)
