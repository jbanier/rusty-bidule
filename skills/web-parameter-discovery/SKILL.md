---
name: web-parameter-discovery
description: Discover hidden GET/POST parameters using arjun, parameth, and wordlist-based fuzzing with safe threading limits
metadata:
  keywords: web, parameters, fuzzing, discovery, arjun, parameth, hidden
---

# Web Parameter Discovery

Use this skill to discover hidden or undocumented GET/POST parameters that may expose additional 
functionality, bypass client-side restrictions, or reveal security vulnerabilities.

Hidden parameters often lead to:
- Debug modes and verbose error messages
- Admin functionality not linked in UI
- Bypass of client-side validation
- Alternative authentication flows
- API endpoints with different behavior

Parameter discovery uses response analysis to identify parameters that:
- Change HTTP status codes
- Modify response content or headers
- Trigger different application behavior
- Cause errors or exceptions

Tools:
  - name: Parameter Discovery
    slug: parameter-discovery
    description: Find hidden parameters via fuzzing and response analysis
    script: scripts/parameter_discovery.py
    network: true
