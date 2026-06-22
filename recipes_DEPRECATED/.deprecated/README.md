# Recipes - DEPRECATED

**Date**: 2026-06-22

## Status: Migrated to Skills

All recipes have been migrated to skills following agentskills.io best practices.

## Why Deprecated

Recipes used declarative YAML workflows that were:
- Rigid (fixed phases)
- Inflexible (couldn't adapt mid-execution)
- Recipe-specific config scattered
- Required users to remember recipe names

## New Approach: Skill-Driven

Skills provide LLM-guided instructions without rigid workflows:
- **Flexible**: LLM decides execution approach
- **Adaptive**: Can change based on discoveries
- **Natural**: Just describe what you want
- **Composable**: Skills reference other skills

## Migration Mapping

| Recipe (DEPRECATED) | Skill (USE THIS) |
|---------------------|------------------|
| web-app-hypothesis-loop | web-hypothesis-driven-testing |
| web-app-input-validation | web-input-validation-testing |
| web-app-scope-intake | web-scope-intake |
| web-app-business-logic-race | web-business-logic-race |
| web-app-final-report | web-final-report-generation |
| web-app-passive-recon | web-passive-recon |
| web-app-access-control | web-access-control-testing |
| web-app-auth-session | web-auth-session-testing |
| web-app-client-side-review | web-client-side-testing |
| web-app-cms-wordpress | web-cms-wordpress-testing |
| web-app-dependency-integrity | web-dependency-testing |
| web-app-engagement-governance | web-engagement-governance |
| web-app-error-crypto-posture | web-error-crypto-testing |
| web-app-files-cache-host | web-files-cache-testing |
| web-app-scanner-normalization | web-scanner-normalization |
| web-app-api-graphql-websocket | web-api-graphql-websocket-testing |
| web-app-active-baseline | web-active-baseline-testing |
| ip-reputation | ip-reputation-analysis |
| morning-routine | morning-routine-briefing |

## How to Use Skills

**Before** (Recipe):
```
User: /recipe use web-app-hypothesis-loop
User: Start testing https://api.example.com
```

**After** (Skill):
```
User: Run hypothesis-driven testing on https://api.example.com
```

LLM automatically:
1. Activates relevant skills
2. Creates dynamic execution plan
3. Adapts based on discoveries
4. Manages todos for tracking

## Documentation

See:
- `docs/SKILL_DRIVEN_ARCHITECTURE.md` - Architecture overview
- `docs/MIGRATION_EXAMPLE.md` - Before/after comparison
- `skills/task-planner/SKILL.md` - Orchestration meta-skill
- `skills/*/SKILL.md` - Individual skill documentation

---

**Do not use recipes - use skills instead!**
