---
name: web-input-probe
description: Produces an authorized manual input-validation probe plan across PortSwigger-style categories without embedding destructive payload libraries.
metadata:
  keywords: web, input validation, sqli, xss, ssrf, xxe, ssti, traversal
---

# Web Input Probe

Use this skill to plan structured manual testing for input validation categories. It intentionally returns probe objectives and evidence requirements, not exploit payload libraries.

Tools:
  - name: Input Probe Plan
    slug: input-probe
    description: Generate category-specific manual probe checklist for selected parameters and contexts.
    script: scripts/input_probe.py

