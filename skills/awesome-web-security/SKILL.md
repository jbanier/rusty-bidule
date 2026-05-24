---
name: awesome-web-security
description: Looks up curated defensive web security references from qazbnm456/awesome-web-security for authorized testing, training, and research.
metadata:
  keywords: web, security, references, xss, ssrf, csrf, sql injection, oauth, jwt, saml, csp, recon, osint, payloads
---

# Awesome Web Security

Use this skill when an operator needs current web security learning resources,
tools, cheatsheets, payload lists, CTF writeups, bug bounty methodology, or
defensive references for authorized work.

Source:
  - Repository: https://github.com/qazbnm456/awesome-web-security
  - Live index: https://raw.githubusercontent.com/qazbnm456/awesome-web-security/master/data/index.json

Constraints:

- Keep guidance defensive, educational, or explicitly scoped to owned systems,
  labs, CTFs, academic research, or authorized testing.
- Refuse help attacking real targets without clear authorization.
- Do not assemble a step-by-step attack playbook for a named real target.
- Do not provide malware authoring, mass scanning of unowned infrastructure,
  credential theft, or detection-evasion guidance for offensive misuse.
- For payload-list resources, include this reminder: "Test payloads only against
  systems you own or have written authorization to test."

Workflow:

1. Fetch the live index JSON from the source URL above.
2. Parse `categories` and `entries`; do not invent entries or metadata.
3. Filter entries by the user's topic, language, difficulty, and requested
   resource type.
4. Rank results according to intent:
   - latest or new: newest `date_added` first
   - deep dive: advanced resources and long-form references first
   - tools: `type: tool` first
   - payloads: `type: payload-list` first, with the safety reminder
5. Return 5-7 results unless the user asks for a different count.

Filtering hints:

- Prefer the user's language, then fall back to English.
- Match direct category keys first, then adjacent category titles, parents, and
  anchors.
- For XSS, include adjacent XSS tools, tricks, practices, and CSP-evasion
  categories.
- For SQL injection, SSRF, and CSRF, include adjacent tools and tricks
  categories.
- For OAuth, JWT, and SAML, include adjacent authentication categories.
- For recon, OSINT, and subdomains, include adjacent discovery and tooling
  categories.

Output format:

```markdown
**[Title](url)** - *author* - *difficulty* - *type*
One sentence on what it teaches and why it matters.
*(Archive fallback: archive_url)*  <!-- only if status is not active and an archive exists -->
```

End with:

```markdown
Cited from [qazbnm456/awesome-web-security](https://github.com/qazbnm456/awesome-web-security).
```

Failure handling:

- If the index cannot be fetched or parsed, say it is temporarily unreachable
  and link to the repository.
- If no entries match, say so explicitly and suggest adjacent categories from
  the fetched `categories` list.
