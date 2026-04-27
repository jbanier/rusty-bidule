---
name: local-analysis
description: Performs local-only IOC extraction, defang/refang, hashing, decoding, URL parsing, IP math, timestamp normalization, file reads, and file metadata inspection without sending artifacts to external services.
metadata:
  keywords: local, ioc, extract, defang, refang, hash, base64, hex, url, cidr, timestamp, file, metadata, entropy
---

# Local Analysis Utilities

Use this skill when an analyst needs deterministic local parsing or inspection of sensitive material.

These tools are intended to stay local by default and should be preferred over remote services for:

- pasted text containing possible indicators
- local files and malware-adjacent artifacts
- encoding and timestamp normalization
- subnet and IP calculations

Constraints:

- Keep artifact handling local unless the user explicitly asks for a remote lookup.
- Prefer the local parsers here before sending indicators to VirusTotal, Shodan, or MCP servers.
- When a tool returns structured JSON, summarize it instead of reformatting it loosely.
- `hash-file`, `read-file`, and `file-metadata` require filesystem read permission.

Tools:
  - name: Extract IOCs
    slug: extract-iocs
    description: Extract IPs, domains, URLs, hashes, CVEs, emails, and ATT&CK IDs from pasted text.
    script: scripts/extract_iocs.py
  - name: Defang
    slug: defang
    description: Defang URLs, domains, IPs, and email addresses in pasted text.
    script: scripts/defang.py
  - name: Refang
    slug: refang
    description: Refang previously defanged URLs, domains, IPs, and email addresses.
    script: scripts/refang.py
  - name: Hash File
    slug: hash-file
    description: Compute MD5, SHA1, SHA256, SHA512, and SSDEEP when available for a local file.
    script: scripts/hash_file.py
    filesystem: read_only
  - name: Decode Blob
    slug: decode-blob
    description: Attempt layered base64 and hex decoding for pasted data.
    script: scripts/decode_blob.py
  - name: Parse URL
    slug: parse-url
    description: Parse a URL locally into scheme, host, path, query params, and fragment.
    script: scripts/parse_url.py
  - name: IP Math
    slug: ip-math
    description: Normalize IPs/CIDRs, classify RFC1918 ranges, and compute subnet details.
    script: scripts/ip_math.py
  - name: Normalize Timestamp
    slug: normalize-timestamp
    description: Convert Unix epoch, ISO8601, Windows FILETIME, and common log timestamps into normalized UTC output.
    script: scripts/normalize_timestamp.py
  - name: Read File
    slug: read-file
    description: Read a bounded portion of a local file for analysis.
    script: scripts/read_file.py
    filesystem: read_only
  - name: File Metadata
    slug: file-metadata
    description: Inspect size, timestamps, MIME/magic hints, and entropy for a local file.
    script: scripts/file_metadata.py
    filesystem: read_only
