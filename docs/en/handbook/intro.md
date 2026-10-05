# Handbook

This section collects practical articles on diagnosing and solving real-world problems with RedisME, organized by scenario with concrete steps and reasoning.

How it differs from the [Guide](/guide/intro/about):

- **Guide**: Product features and how to use the UI
- **Handbook**: Scenario-driven walkthroughs for locating, analyzing, and fixing issues with RedisME

## Articles

- [Slowlog Governance](/handbook/slowlog-governance): Production slowlog remediation (baseline → locate → fix → prevent recurrence)
- [SSL Encryption](/handbook/ssl-encryption): New TLS cluster; two checks per app and a full cutover in one iteration; sync the old cluster to the new one while both run, then stop sync and retire the old cluster only after no application connections remain
