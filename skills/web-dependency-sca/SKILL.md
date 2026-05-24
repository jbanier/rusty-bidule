---
name: web-dependency-sca
description: Inventories local dependency manifests, lightweight package metadata, supplied SCA output, and browser third-party assets for passive software component and supply-chain review.
metadata:
  keywords: web, dependencies, sca, supply-chain, package, sri, cdn, javascript
---

# Web Dependency SCA

Use this skill to inventory package manifests and browser supply-chain exposure. It plans SCA commands but does not run network scanners by default.

Inputs may include local manifests, lockfiles, supplied scanner JSON, HTML assets, browser evidence, or client-side audit output. Treat scanner output as leads until validated and deduplicated.

Tools:
  - name: Dependency SCA
    slug: dependency-sca
    description: Inventory dependency files and third-party browser assets, normalize supplied SCA leads, and generate safe follow-up command plans.
    script: scripts/dependency_sca.py
    network: false
    filesystem: read_only
    safety_profile: passive
    requires_active_authorization: false
    methodology:
      - OWASP Top 10 A06
      - OWASP Top 10 A08
      - OWASP SCVS
      - NIST SP 800-53 SI

