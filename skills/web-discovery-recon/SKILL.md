---
name: web-discovery-recon
description: Plans scoped web discovery and reconnaissance with optional tools such as subfinder, dnsx, httpx, naabu, nmap, wafw00f, and testssl.sh.
metadata:
  keywords: web, recon, discovery, dns, subdomain, tls, waf, ports
---

# Web Discovery Recon

Use this skill to create a scoped recon command plan and identify which recon tools are installed. Run active commands only after scope is validated and active testing is authorized.

Tools:
  - name: Discovery Recon Plan
    slug: discovery-recon
    description: Validate scope and return bounded recon commands plus local tool availability.
    script: scripts/discovery_recon.py
    network: true

