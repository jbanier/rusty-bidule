#!/bin/bash
#
# Migrate recipes to skills
#
# Extracts the core instructions from recipes and converts them to
# skill format (agentskills.io compliant)
#

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "========================================="
echo "  Recipe → Skill Migration"
echo "========================================="
echo

# Check what recipes exist vs skills
echo "[1/4] Analyzing recipes and skills..."

RECIPES_DIR="${PROJECT_ROOT}/recipes"
SKILLS_DIR="${PROJECT_ROOT}/skills"

# Find all recipes
RECIPES=()
while IFS= read -r recipe_dir; do
    recipe_name=$(basename "$recipe_dir")
    RECIPES+=("$recipe_name")
done < <(find "$RECIPES_DIR" -mindepth 1 -maxdepth 1 -type d | sort)

echo "Found ${#RECIPES[@]} recipes"

# Find corresponding skills
echo
echo "[2/4] Checking which recipes already have skills..."
echo

NEEDS_MIGRATION=()
ALREADY_MIGRATED=()

for recipe in "${RECIPES[@]}"; do
    # Check if skill exists with similar name
    skill_name=$(echo "$recipe" | sed 's/web-app-/web-/')

    if [ -d "${SKILLS_DIR}/${skill_name}" ] || [ -d "${SKILLS_DIR}/${recipe}" ]; then
        ALREADY_MIGRATED+=("$recipe")
        echo "  ✅ $recipe → skill exists"
    else
        NEEDS_MIGRATION+=("$recipe")
        echo "  ⚠️  $recipe → needs migration"
    fi
done

echo
echo "Summary:"
echo "  Already migrated: ${#ALREADY_MIGRATED[@]}"
echo "  Needs migration: ${#NEEDS_MIGRATION[@]}"

if [ ${#NEEDS_MIGRATION[@]} -eq 0 ]; then
    echo
    echo "✅ All recipes already have corresponding skills!"
    exit 0
fi

echo
echo "[3/4] Creating migration plan..."
echo

# Create migration plan
MIGRATION_PLAN="${PROJECT_ROOT}/RECIPE_MIGRATION_PLAN.md"

cat > "$MIGRATION_PLAN" << 'EOF'
# Recipe Migration Plan

**Date**: 2026-06-22

## Status

Recipes that need migration to skills:

EOF

for recipe in "${NEEDS_MIGRATION[@]}"; do
    recipe_file="${RECIPES_DIR}/${recipe}/RECIPE.md"

    if [ -f "$recipe_file" ]; then
        # Extract title and description
        title=$(grep -m 1 "^title:" "$recipe_file" | sed 's/title: //')
        desc=$(grep -m 1 "^description:" "$recipe_file" | sed 's/description: //')

        echo "### $recipe" >> "$MIGRATION_PLAN"
        echo >> "$MIGRATION_PLAN"
        if [ -n "$title" ]; then
            echo "**Title**: $title" >> "$MIGRATION_PLAN"
        fi
        if [ -n "$desc" ]; then
            echo "**Description**: $desc" >> "$MIGRATION_PLAN"
        fi
        echo >> "$MIGRATION_PLAN"
        echo "**Action**: Convert workflow to skill instructions" >> "$MIGRATION_PLAN"
        echo >> "$MIGRATION_PLAN"
    fi
done

cat >> "$MIGRATION_PLAN" << 'EOF'

## Migration Strategy

For each recipe:

1. **Extract core logic**:
   - Remove `Config:` section (tool access now skill-specific)
   - Remove `Workflow:` structure (LLM plans dynamically)
   - Keep `Instructions:` as main skill content
   - Convert `steps` to sequential guidance (not rigid phases)

2. **Create skill file**:
   - `skills/<recipe-name>/SKILL.md`
   - Use agentskills.io format
   - Add proper frontmatter (name, description, metadata)

3. **Handle dependencies**:
   - If recipe activates other skills → document in "Related Skills"
   - If recipe uses specific tools → add to `Tools:` section

4. **Test**:
   - Verify LLM can follow skill instructions
   - Compare results with original recipe execution

## Next Steps

Run:
```bash
# For each recipe needing migration:
# 1. Manually convert recipe/RECIPE.md to skill/SKILL.md
# 2. Test the skill
# 3. Mark recipe as deprecated
```

After all migrations:
```bash
# Move recipes to deprecated
mv recipes/ recipes_deprecated/

# Update .gitignore
echo "recipes_deprecated/" >> .gitignore
```
EOF

echo "Migration plan created: $MIGRATION_PLAN"

echo
echo "[4/4] Next Steps"
echo

echo "Review the migration plan:"
echo "  cat $MIGRATION_PLAN"
echo
echo "Recipes needing migration:"
for recipe in "${NEEDS_MIGRATION[@]}"; do
    echo "  - $recipe"
done

echo
echo "========================================="
echo "Migration analysis complete!"
echo "========================================="
