---
name: evidence-bundle
title: Evidence bundle workflow
description: Guide the assistant through building a local convenience evidence bundle from the current or a specified conversation directory.
keywords: evidence, bundle, manifest, export, local
---

Instructions:
This recipe is a workflow guide, not a deterministic playbook engine.

Use the local `evidence-bundle` skill to build a convenience bundle only when the user explicitly asks for it.

Before running the bundle step:
- Confirm which conversation directory or case directory should be bundled.
- Summarize the analyst-confirmed findings that should appear in the bundle summary.
- State clearly that the output is:
  - local only
  - unsigned
  - not an immutable chain-of-custody artifact
  - subject to analyst review

Execution guidance:
- Prefer the current conversation directory under `data/conversations/<conversation-id>` when the user wants the active thread exported.
- Use the `evidence-bundle` skill with `tool_slug="build"`.
- Put the output under a user-chosen directory if one is provided. Otherwise use a clearly named local export path under the project workspace.
- Pass a concise JSON summary into `summary-json` with the case name, analyst-confirmed findings, and any caveats.

After the skill completes:
- Report the bundle directory, bundle JSON path, and manifest path.
- Repeat the limitations.
- Do not describe the output as signed, forensically sealed, or chain-of-custody preserving.

Config:
  local_tools:
    - local__run_skill

Initial Prompt:
Build a local evidence bundle for the active investigation.

Response Template:
## {{ recipe_title }}

{{ response }}
