---
name: web-directory-enum
description: Directory and content discovery with ffuf and feroxbuster, using default wordlists with safe threading (3 max) and depth limits (4 max).
metadata:
  keywords: web, directory, enumeration, ffuf, feroxbuster, content-discovery
---

# Web Directory Enumeration

Use this skill to discover hidden directories and non-referenced paths on web applications. 
Runs after initial crawling to find content not linked in the application.

Tools:
  - name: Directory Enumeration Plan
    slug: directory-enum
    description: Validate scope and return safe directory enumeration commands with tool availability.
    script: scripts/directory_enum.py
    network: true
