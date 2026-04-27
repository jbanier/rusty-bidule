---
name: shodan
description: Uses the Shodan CLI to investigate public exposure for an IP address or host. Use when the user wants internet-facing service intelligence, recent scan data, exposed ports, banners, or CVE context from Shodan.
metadata:
  keywords: shodan, exposure, intel, services, ip, host, banner, cve, vulnerability
---

# Shodan Exposure Investigation

## Overview

Use this skill when investigating an internet-facing asset with Shodan. Focus on the most recent scan date, exposed ports and services, notable banners, and any vulnerability or CVE context returned by Shodan.

Prefer this skill for:

- public IP addresses
- externally reachable hosts
- quick exposure triage before deeper validation with other tools

## Prerequisites

- `shodan` must be installed and available in `PATH`
- the user must be authenticated with a Shodan API key
- initialize the CLI with:

```bash
shodan init <api-key>
```

## Constraints and guidance

- Use Shodan for external exposure only. It does not replace internal scanning or asset inventory tools.
- Treat Shodan data as point-in-time telemetry. Always note the scan date before drawing conclusions.
- Prefer host lookups for a specific IP instead of broad searches when the user already knows the target.
- If the result is empty, say that Shodan has no current public data for the host instead of assuming the host is safe or offline.

## Common Commands

Look up a specific IP or host:

```bash
shodan host <ip>
```

Look up DNS names associated with an IP:

```bash
shodan host --history <ip>
```

Search for matching internet-facing services when a broader query is needed:

```bash
shodan search <query>
```

Return machine-readable output:

```bash
shodan host --format json <ip>
```

## Output expectations

When summarizing Shodan findings, include:

- whether Shodan has data for the target
- most recent scan or observation date
- open ports and detected services
- notable banners, products, or versions when present
- CVEs or vulnerability indicators returned by Shodan
- any obvious public exposure concerns such as remote admin interfaces or outdated software

If the output is large, provide a concise summary and highlight the ports or services most relevant to the investigation.

## Notes

- `shodan host <ip>` is usually the best starting point for an IP investigation.
- Historical data may help explain stale or intermittent exposure, but it can also surface services that are no longer reachable.
- Product/version detection depends on the banner data Shodan collected and may be incomplete.
