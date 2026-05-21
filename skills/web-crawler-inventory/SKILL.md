---
name: web-crawler-inventory
description: Builds a scoped web application inventory of URLs, forms, scripts, and parameters using a safe built-in crawler or operator-approved crawler commands.
metadata:
  keywords: web, crawl, inventory, urls, forms, javascript, parameters
---

# Web Crawler Inventory

Use this skill after scope validation to build an endpoint and client-side inventory.

Constraints:

- Keep crawl depth and URL count bounded.
- Crawl only scoped hosts.
- Prefer built-in low-rate crawling first; use tools like Katana, gospider, or ZAP only when authorized and rate limits are clear.

Tools:
  - name: Crawl Inventory
    slug: crawl-inventory
    description: Validate scope and optionally crawl same-scope pages for links, forms, scripts, and input names.
    script: scripts/crawler_inventory.py
    network: true

