---
name: web-crypto-posture
description: Reviews supplied TLS, certificate, HTTP header, CSP, and related-host evidence for passive cryptographic posture assessment and authorized follow-up command planning.
metadata:
  keywords: web, tls, certificate, crypto, hsts, csp, ct, headers
---

# Web Crypto Posture

Use this skill to summarize cryptographic posture from existing evidence. It separates confirmed in-scope targets from related host candidates derived from certificate transparency, CSP sources, or supplied observations.

Do not test CT-discovered or CSP-derived hosts unless they match the authorized scope. Generated TLS commands are follow-up plans only and must be run separately with explicit authorization.

Tools:
  - name: Crypto Posture
    slug: crypto-posture
    description: Normalize TLS/certificate/header evidence, identify related host candidates, and produce authorized TLS follow-up plans.
    script: scripts/crypto_posture.py
    network: false
    filesystem: read_only
    safety_profile: passive
    requires_active_authorization: false
    methodology:
      - OWASP WSTG-CRYP
      - OWASP WSTG-CONF
      - OWASP ASVS V9
      - NIST SP 800-53 SC

