---
name: web-browser-evidence
description: Normalizes browser-driven web assessment observations for JavaScript-heavy or authenticated flows, including DOM routes, forms, screenshots, and client-side evidence.
metadata:
  keywords: web, browser, dom, screenshot, authenticated, javascript
---

# Web Browser Evidence

Use this skill after browser-assisted inspection or authenticated navigation. It does not drive a browser by itself; it organizes observations captured by the operator or browser automation tools.

Tools:
  - name: Normalize Browser Evidence
    slug: normalize-browser-evidence
    description: Normalize browser observations, screenshots, DOM routes, forms, storage, and client-side notes into evidence-ready JSON.
    script: scripts/browser_evidence.py
    safety_profile: passive
    methodology:
      - OWASP WSTG-CLNT
      - OWASP WSTG-SESS

