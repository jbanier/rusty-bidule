---
name: web-js-route-extractor
description: Extracts routes, API paths, WebSocket URLs, source maps, forms, and interesting parameters from scoped HTML or JavaScript without sending payloads.
metadata:
  keywords: web, javascript, routes, sourcemap, api, websocket
---

# Web JS Route Extractor

Use this skill to analyze provided or scoped fetched HTML/JavaScript for client-side attack surface. It extracts structural facts only.

Tools:
  - name: Extract JS Routes
    slug: extract-js-routes
    description: Extract routes, API paths, WebSocket URLs, source maps, forms, and likely parameter names from HTML/JavaScript text, files, or scoped URLs.
    script: scripts/js_route_extractor.py
    network: true
    filesystem: read_only
    safety_profile: passive
    requires_active_authorization: true
    methodology:
      - OWASP WSTG-CLNT
      - OWASP WSTG-INFO

