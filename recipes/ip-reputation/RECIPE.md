---
name: ip-reputation
title: IP reputation check
description: Determine whether an IP belongs to Cisco or related infrastructure, then gather exposure and reputation context for the asset.
keywords: reputation, validation, intel, whois, shodan, runzero, ip
---

Instructions:
The goal is to assess one or more IP addresses and determine whether each one is internal to Cisco or a related subsidiary, or external. Then gather enough context to explain what the asset is, what services are exposed, and whether the hosting or exposure looks suspicious.

Work IP-by-IP. Keep the findings for each address separate in the final response.

**1 — Ownership check (use shell)**
- Use `whois <ip>` to determine the owning network or ASN.
- Classify the address as `Internal` if it belongs to Cisco or a clearly related subsidiary or brand such as Webex.
- If the ownership is ambiguous, say so and continue with cautious wording instead of forcing a classification.

**2 — Internal branch: RunZero and ownership context**
- For Cisco or related assets, use the RunZero MCP against the `Cisco & EXternals fullscan` organization.
- Retrieve the services and vulnerabilities visible for that asset.
- Use DCE to obtain contact information and team ownership when available.
- Summarize the likely system purpose based on the discovered services, not just the hostname.

**3 — External branch: hosting and exposure checks**
- For non-Cisco assets, use web search to evaluate the hosting provider or owner named by `whois`.
- Specifically check whether the provider has a reputation for abuse, bulletproof hosting, or other trust concerns.
- Use the `shodan` skill to gather public exposure: most recent scan date, exposed services, and any CVE context.
- If Shodan has no result, state that clearly and rely on the ownership and provider reputation evidence instead.

**4 — Final assessment**
For each IP, provide these fields in the response:
- classification: `Internal`, `External`, or `Unclear`
- owner / network
- observed services and exposure
- notable vulnerabilities or CVEs if present
- contact or ownership details for internal assets when available
- a short risk or reputation assessment

Do not present a provider as malicious based on weak signals alone. Use precise language such as `suspicious`, `abuse-prone`, `no obvious concern found`, or `insufficient evidence`.

Initial Prompt:
I need a background check on the following IP addresses:

Response Template:
## {{ recipe_title }}

{{ response }}
