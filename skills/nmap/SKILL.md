---
name: nmap
description: Performs basic non-root network reconnaissance with nmap. Use when the user asks to scan a host, check whether a TCP port is open, identify exposed services, map open ports on an IP or CIDR range, or determine what is running on a server with nmap.
metadata:
  keywords: nmap, scan, port, network, host, reconnaissance, service, tcp, cidr
---

# Nmap Basic Network Reconnaissance

## Overview

Use this skill for non-privileged nmap reconnaissance. Focus on active hosts, open TCP ports, and service identification without requiring sudo or raw-packet features.

## Constraints

- Do not use sudo or root-only scan modes.
- Use TCP connect scans with `-sT` when scan type must be explicit.
- Do not use `-sS`, `-O`, `-sU`, or raw packet injection flags.
- Prefer targeted hosts, CIDR ranges, and port sets over unnecessarily broad scans.
- Verify the target scope matches the user request before scanning.

## Common Commands

Check whether a specific port is open:

```bash
nmap -sT -p <port> <target>
```

Identify exposed services on a host:

```bash
nmap -sT -sV <target>
```

Run default safe scripts against selected ports:

```bash
nmap -sT -sV -sC -p <ports> <target>
```

Perform a fast scan of a subnet:

```bash
nmap -sT -F <target-cidr>
```

Scan a specific TCP port range:

```bash
nmap -sT -p <start-end> <target>
```

## Output Expectations

Return the relevant nmap output: host reachability, open ports, detected services, versions when available, and safe script output when `-sC` is used.

## Notes

- TCP connect scans are more visible in logs than SYN scans.
- Large ranges can be slow without root privileges.
- If the user asks what is running on a server, prefer `nmap -sT -sV <target>`.
