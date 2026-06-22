---
name: ip-reputation-analysis
description: Determine IP ownership (internal/external), gather exposure context, and assess reputation for security incidents or suspicious hosting
metadata:
  keywords: ip, reputation, whois, shodan, runzero, intel, investigation
---

# IP Reputation Analysis

Assess IP addresses to determine ownership, exposure, and reputation context for security investigations.

## Purpose

For each IP address:
1. Determine if internal or external to organization
2. Gather service/vulnerability context
3. Assess hosting reputation
4. Provide security-focused summary

**Use when**: Investigating suspicious IPs, triaging alerts, assessing asset exposure

## Analysis Approach

Work **IP-by-IP**. Keep findings for each address separate.

### Step 1: Ownership Check

Use `whois` command to determine:
- Owning network/ASN
- Registered organization
- Network block

**Classification**:
- **Internal**: Belongs to organization or subsidiary
- **External**: Third-party owned
- **Ambiguous**: Unclear ownership (document uncertainty)

**Example**:
```bash
whois 198.51.100.42

Result:
  NetRange: 198.51.100.0 - 198.51.100.255
  OrgName: Example Corp
  → Classification: Internal (matches organization)
```

### Step 2a: Internal IP Analysis

For internal/related assets:

1. **Query asset management**:
   - Use RunZero MCP (if available)
   - Query: Asset services, vulnerabilities, last scan
   
2. **Identify ownership**:
   - Contact team (from DCE/asset DB if available)
   - System purpose (from discovered services)

3. **Assess exposure**:
   - What services are running?
   - Any known vulnerabilities?
   - Appropriate for network zone?

**Output**:
```
IP: 10.0.5.42 (Internal)
Owner: Engineering team
Services: SSH (22), HTTPS (443)
Purpose: Build server (CI/CD)
Vulnerabilities: None critical
Assessment: Normal exposure for build infrastructure
```

### Step 2b: External IP Analysis

For external assets:

1. **Hosting provider evaluation**:
   - Who hosts it? (from whois)
   - Provider reputation:
     - Known for abuse?
     - Bulletproof hosting?
     - Legitimate datacenter?
   
   Use web search: `"<provider> bulletproof hosting"` or `"<provider> abuse reputation"`

2. **Public exposure scan**:
   - Activate `shodan` skill
   - Query: Services, open ports, CVEs
   - Recent scan date (freshness)

3. **Threat intelligence** (if available):
   - Check abuse databases
   - Known malicious activity?

**Output**:
```
IP: 45.142.120.10 (External)
Hosting: HostingProvider LLC (Country: XX)
Provider reputation: Known for lax abuse policies
Shodan results:
  - Last scanned: 2026-06-20
  - Open ports: 22 (SSH), 80 (HTTP), 445 (SMB)
  - CVEs: CVE-2023-12345 (OpenSSH vuln)
Assessment: ⚠️ Suspicious - lax provider + SMB exposed + outdated SSH
```

### Step 3: Final Assessment

Synthesize findings:

**For Internal IPs**:
- Is exposure appropriate for purpose?
- Any unexpected services?
- Vulnerabilities requiring patching?
- Ownership clear?

**For External IPs**:
- Hosting provider trustworthy?
- Services expected for context?
- Known malicious activity?
- Related to organization business?

**Risk levels**:
- 🟢 **Normal**: Expected ownership, appropriate services
- 🟡 **Suspicious**: Unexpected hosting, unusual services, outdated software
- 🔴 **Malicious**: Known bad provider, C2 indicators, active threats

## Tools and Skills

**Required tools**:
- `whois` command (via bash)
- Web search (for provider reputation)

**Optional integrations**:
- `shodan` skill: Public exposure data
- RunZero MCP: Internal asset management
- DCE/asset DB: Team ownership

## Example Execution

```
User: "Analyze these IPs: 10.0.5.42, 198.51.100.10, 45.142.120.5"

LLM (activates this skill):

[IP 1: 10.0.5.42]
Running whois 10.0.5.42...
Result: RFC1918 private address
Classification: Internal (private IP space)

Querying internal asset DB...
Owner: Engineering - Build Team
Services: SSH, Docker API
Purpose: CI/CD build server
Assessment: 🟢 Normal - appropriate for build infrastructure

---

[IP 2: 198.51.100.10]
Running whois 198.51.100.10...
Result: Registered to Example Corp
Classification: Internal (organization owned)

Checking exposure...
Services: HTTPS (443), SSH (22)
Purpose: Public web server
Vulnerabilities: None critical
Assessment: 🟢 Normal - expected public web presence

---

[IP 3: 45.142.120.5]
Running whois 45.142.120.5...
Result: BulletProofHost LLC, Country XX
Classification: External (third-party)

Evaluating provider reputation...
Web search: "BulletProofHost abuse"
Results: Known for hosting malicious content, lax abuse policies

Activating shodan skill...
Query: 45.142.120.5
Results:
  - Open ports: 22 (SSH), 445 (SMB), 3389 (RDP)
  - Last scan: 2026-06-21
  - CVEs: 3 critical (outdated Windows Server)

Assessment: 🔴 Suspicious/Malicious
- Known bad hosting provider
- SMB + RDP exposed (rare for legit servers)
- Multiple critical CVEs
- Likely compromised server or attacker infrastructure

Recommendation: Block at firewall, add to threat intel
```

## Output Format

```markdown
## IP Reputation Analysis

### IP: 10.0.5.42
- **Classification**: Internal
- **Owner**: Engineering - Build Team
- **Services**: SSH (22), Docker (2376)
- **Purpose**: CI/CD build server
- **Vulnerabilities**: None critical
- **Assessment**: 🟢 Normal

### IP: 45.142.120.5
- **Classification**: External
- **Hosting**: BulletProofHost LLC
- **Provider Reputation**: ⚠️ Known for abuse/malicious hosting
- **Shodan Results**:
  - SMB (445), RDP (3389) exposed
  - CVE-2023-XXXX (critical)
- **Assessment**: 🔴 Suspicious - likely attacker infrastructure
- **Recommendation**: Block, add to threat intel feed

---

**Summary**: 1 internal (normal), 1 external (high risk - block recommended)
```

## Tips

1. **Document ambiguity**: If ownership unclear, say so
2. **Context matters**: Exposed SMB on internal build server = normal; on external unknown = suspicious
3. **Provider reputation key**: Bulletproof hosting is major red flag
4. **Freshness check**: Shodan data can be stale - note scan date
5. **Multiple sources**: Don't rely on single data source

---

**Use this skill for rapid IP triage during security investigations.**
