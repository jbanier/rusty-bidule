---
name: evidence-bundle
description: Builds a local evidence bundle from an existing conversation directory into a timestamped export folder with a JSON summary and SHA256 manifest. This is an analyst convenience workflow, not a signed chain-of-custody mechanism.
keywords: evidence, bundle, manifest, sha256, export, conversation
---

# Evidence Bundle Writer

Use this skill only when the user explicitly wants a local convenience bundle.

Constraints:

- This is not a cryptographically signed archive.
- This does not provide immutable chain-of-custody guarantees.
- The analyst must review the output before treating it as final evidence.
- Prefer using this skill through the `evidence-bundle` recipe so the limitations are stated clearly.

Tools:
  - name: Build Evidence Bundle
    slug: build
    description: Copy a conversation directory into a timestamped export folder and emit a JSON summary plus SHA256 manifest.
    script: scripts/build_bundle.py
    filesystem: read_write
