---
name: web-payload-catalog
description: Provides opt-in, safety-labeled web payload reference categories for authorized testing without auto-running exploit payloads.
metadata:
  keywords: web, payloads, catalog, references, safety labels
---

# Web Payload Catalog

Use this skill when an operator needs a curated reference for manual authorized testing. It returns payload families and safety labels, not an automated exploit list.

Constraints:

- Do not auto-run catalog items.
- Prefer benign detection probes before intrusive payloads.
- Do not include web shells or persistence payloads.
- Require explicit authorization before OOB, destructive, brute-force, or intrusive classes.

Tools:
  - name: List Payload Catalog
    slug: list-payload-catalog
    description: Return safety-labeled payload families, references, and usage cautions for selected web vulnerability categories.
    script: scripts/payload_catalog.py
    safety_profile: passive
    methodology:
      - OWASP WSTG

