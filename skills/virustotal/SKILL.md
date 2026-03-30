---
name: virustotal
description: Uses the VirusTotal CLI (`vt`) to investigate suspicious file hashes, URLs, domains, and IP addresses. Use when the user wants quick threat intelligence, reputation checks, relationship pivots, or VirusTotal context for likely indicators of compromise.
keywords: virustotal, vt, hash, sha256, sha1, md5, url, domain, ip, indicator, ioc, malware, reputation
---

# VirusTotal CLI Investigation

## Overview

Use this skill when an investigator wants to query VirusTotal from the terminal with the `vt` command-line client.

Prefer this skill for:

- known file hashes (`SHA-256`, `SHA-1`, or `MD5`)
- suspicious URLs
- domains and subdomains
- IP addresses
- quick relationship pivots such as downloaded files, resolutions, contacted infrastructure, or related reports

## Prerequisites

- `vt` must be installed and available in `PATH`
- the user must have a VirusTotal API key
- initialize the CLI with:

```bash
vt init
```

You can also pass an API key explicitly with `--apikey`, but `vt init` is usually simpler for repeated use.

## Constraints and safety guidance

- Prefer lookups over uploads when the user already has a hash, URL, domain, or IP.
- Treat `vt scan file` as **explicit opt-in** only. Uploading a file may share samples or metadata with VirusTotal and can be inappropriate for sensitive/internal artifacts.
- When possible, hash a file locally and query the hash instead of uploading the full binary.
- Use `--format json` when the output needs to be parsed or summarized carefully.
- Use `--silent` in scripts or automation to avoid noisy progress output.

## Common commands

### File hash lookups

Look up a known file hash:

```bash
vt file <sha256-or-sha1-or-md5>
```

Look up several hashes:

```bash
vt file <hash1> <hash2> <hash3>
```

Read hashes from standard input:

```bash
cat hashes.txt | vt file -
```

Return structured output:

```bash
vt file --format json <sha256>
```

Useful pivots from a file:

```bash
vt file contacted_domains <sha256>
vt file contacted_ips <sha256>
vt file contacted_urls <sha256>
vt file dropped_files <sha256>
vt file embedded_urls <sha256>
vt file related_reports <sha256>
vt file threat_actors <sha256>
```

### URLs

Look up a URL:

```bash
vt url https://example.com/path
```

Look up multiple URLs or read them from a file:

```bash
vt url https://example.com https://evil.example
cat urls.txt | vt url -
```

Useful pivots from a URL:

```bash
vt url downloaded_files https://example.com/path
vt url redirects_to https://example.com/path
vt url redirecting_urls https://example.com/path
vt url related_reports https://example.com/path
```

Submit a URL for scanning:

```bash
vt scan url https://example.com/path
```

### Domains

Look up a domain:

```bash
vt domain example.com
```

Useful pivots from a domain:

```bash
vt domain resolutions example.com
vt domain subdomains example.com
vt domain downloaded_files example.com
vt domain communicating_files example.com
vt domain urls example.com
vt domain historical_whois example.com
```

### IP addresses

Look up an IP:

```bash
vt ip 203.0.113.10
```

Useful pivots from an IP:

```bash
vt ip resolutions 203.0.113.10
vt ip downloaded_files 203.0.113.10
vt ip communicating_files 203.0.113.10
vt ip urls 203.0.113.10
```

### Analyses and scans

Retrieve an analysis by ID:

```bash
vt analysis <analysis-id>
```

Scan a file only when the user explicitly wants to upload it:

```bash
vt scan file /path/to/sample.bin
```

## Output expectations

When summarizing `vt` output, focus on the fields most useful to investigators:

- detection / reputation signals
- object identifiers such as hash, URL, domain, or IP
- last analysis stats or malicious/suspicious verdict counts
- notable tags, families, or classifications
- relationships worth pivoting into next
- supporting context such as downloaded files, resolutions, related reports, or threat actors

If the output is large, prefer a concise summary plus the next-best pivots instead of dumping the entire response.

## Notes

- `vt` supports `yaml`, `json`, and `csv` output via `--format`.
- Hash lookups are usually safer than uploads for sensitive environments.
- Some advanced commands depend on the VirusTotal subscription tier and API privileges.
- For batch work, standard input is often supported by passing `-` as the target.
