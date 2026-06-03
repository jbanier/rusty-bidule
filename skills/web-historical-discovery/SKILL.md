---
name: web-historical-discovery
description: Historical endpoint and parameter discovery using Wayback Machine, GitHub, and archive sources
metadata:
  keywords: web, historical, wayback, endpoints, parameters, discovery
---

# Web Historical Discovery

Use this skill to discover historical endpoints, parameters, and paths from Wayback Machine, GitHub, 
and other archive sources that may still be accessible but are no longer linked in the current application.

Historical analysis often reveals:
- Deprecated admin panels or debug endpoints
- Forgotten API endpoints with weaker security
- Parameter names used in older versions
- Technology stack changes over time
- Exposed configuration files or backups

Tools:
  - name: Historical Endpoint Discovery
    slug: historical-discovery
    description: Fetch historical URLs and extract endpoints/parameters for testing
    script: scripts/historical_discovery.py
    network: true
