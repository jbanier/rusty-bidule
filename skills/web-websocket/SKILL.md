---
name: web-websocket
description: Plans scoped WebSocket assessment checks for authentication propagation, message tampering, replay, origin policy, and command tooling.
metadata:
  keywords: web, websocket, wscat, websocat, replay, tamper
---

# WebSocket Review

Use this skill to plan WebSocket testing. It validates scoped `ws://` or `wss://` endpoints and returns operator-assisted commands.

Tools:
  - name: WebSocket Review
    slug: websocket-review
    description: Validate WebSocket endpoint scope and return review checklist plus wscat/websocat command plan.
    script: scripts/websocket_review.py
    network: true

