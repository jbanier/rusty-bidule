---
name: web-client-side-audit
description: Reviews supplied client-side HTML, JavaScript, browser evidence, sitemap, and OpenAPI artifacts for passive route discovery, CSP/SRI posture, DOM risk indicators, postMessage handling, storage exposure, and shadow API candidates.
metadata:
  keywords: web, javascript, client, csp, sri, dom, shadow-api, openapi, sitemap
---

# Web Client-Side Audit

Use this skill for passive review of scoped client-side artifacts. It adapts bug bounty recon patterns into structured inventories and manual validation plans; it does not execute one-liners, fuzz routes, brute force, or actively probe discovered URLs.

Inputs may include HTML, JavaScript, browser evidence, route extractor output, sitemap XML, OpenAPI JSON, and response headers. Treat extracted endpoints as leads until validated with scoped HTTP evidence and authorization checks.

Tools:
  - name: Client-Side Audit
    slug: client-side-audit
    description: Build passive JS/API/CSP/SRI/DOM inventories and shadow API candidates from supplied client-side artifacts.
    script: scripts/client_side_audit.py
    network: false
    filesystem: read_only
    safety_profile: passive
    requires_active_authorization: false
    methodology:
      - OWASP WSTG-CLNT
      - OWASP WSTG-INFO
      - OWASP ASVS V14

