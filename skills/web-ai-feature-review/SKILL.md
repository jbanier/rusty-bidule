---
name: web-ai-feature-review
description: Plans and records authorized testing of AI-enabled web application features, including prompt injection, tool abuse, retrieval exposure, and agentic boundary checks.
metadata:
  keywords: web, ai, llm, prompt injection, agentic, mcp
---

# Web AI Feature Review

Use this skill for LLM-enabled web features and agentic workflows. It produces a scoped review plan and evidence checklist, not a payload runner.

Tools:
  - name: AI Feature Review
    slug: ai-feature-review
    description: Build a scoped AI-feature testing checklist for prompt injection, indirect prompt injection, tool abuse, retrieval exposure, and system prompt leakage.
    script: scripts/ai_feature_review.py
    safety_profile: active-safe
    requires_active_authorization: true
    methodology:
      - OWASP Top 10 for LLM Applications
      - OWASP WSTG-BUSL

