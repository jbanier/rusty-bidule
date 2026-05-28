#!/usr/bin/env bash
# Migration script to move data from project-local data/ to ~/.rusty/

set -euo pipefail

PROJECT_DATA="data"
HOME_DATA="$HOME/.rusty"

echo "rusty-bidule Data Directory Migration"
echo "======================================"
echo

# Check if project data directory exists
if [ ! -d "$PROJECT_DATA" ]; then
    echo "✗ No project-local data/ directory found."
    echo "  Nothing to migrate. You can start using ~/.rusty immediately."
    exit 0
fi

# Check if home data directory already exists
if [ -d "$HOME_DATA" ] && [ "$(ls -A "$HOME_DATA" 2>/dev/null)" ]; then
    echo "⚠ Warning: ~/.rusty already contains data"
    echo
    ls -lh "$HOME_DATA" | head -10
    echo
    read -p "Overwrite existing data? [y/N] " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Migration cancelled."
        exit 1
    fi
fi

# Show what will be migrated
echo "Source: $PROJECT_DATA"
echo "Destination: $HOME_DATA"
echo
echo "Contents to migrate:"
ls -lh "$PROJECT_DATA" | head -10
echo
read -p "Proceed with migration? [y/N] " -n 1 -r
echo

if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Migration cancelled."
    exit 1
fi

# Create destination directory
mkdir -p "$HOME_DATA"

# Copy data
echo
echo "Copying data..."
cp -rv "$PROJECT_DATA"/* "$HOME_DATA/" 2>&1 | grep -v '^$'

# Verify
echo
echo "Migration complete!"
echo
echo "Verification:"
echo "  Project data: $(du -sh "$PROJECT_DATA" | cut -f1)"
echo "  Home data:    $(du -sh "$HOME_DATA" | cut -f1)"
echo

# Suggest next steps
echo "Next steps:"
echo "1. Update your config/config.local.yaml:"
echo "   data_dir: ~/.rusty"
echo
echo "2. Test the agent to verify conversations are accessible"
echo
echo "3. (Optional) Remove old data directory:"
echo "   rm -rf $PROJECT_DATA"
echo
echo "4. (Optional) Update .gitignore if needed"
