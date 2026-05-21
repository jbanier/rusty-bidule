---
name: web-api-graphql
description: Reviews OpenAPI and GraphQL posture, including endpoint inventory, auth hints, object authorization, introspection, batching, depth, and rate-limit risks.
metadata:
  keywords: web, api, openapi, graphql, schema, bola
---

# Web API And GraphQL Review

Use this skill to analyze API specifications or plan safe GraphQL/API checks. It validates scoped URLs before fetching specs.

Tools:
  - name: API GraphQL Review
    slug: api-graphql-review
    description: Parse an OpenAPI JSON file or URL and return endpoint inventory plus GraphQL/API posture checklist.
    script: scripts/api_graphql_review.py
    network: true
    filesystem: read_only

