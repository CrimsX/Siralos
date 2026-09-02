---
title: "Mechanical Secret-Hygiene Gate — Enforcing the 68 §4 Sweep"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 72 — Mechanical Secret-Hygiene Gate — Enforcing the 68 §4 Sweep

**Governing plan:** [68 — Real Provider Credentials and Registry](68-real-provider-credentials-and-registry.md) · **Roll-up:** [71 — Real Provider Verified Roll-Up](71-real-provider-verified-roll-up.md) · **Map:** [Siralos Roadmap](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** Guardrail follow-through of the Real Model/Provider range: the decision 68 §4 sweep becomes a standing mechanical gate; no product code change.

## 1. The gate

- **Runner:** `scripts/check-secret-hygiene.mjs` (deterministic Node ESM, `path.join(import.meta.dirname, "..")` root) — walk skips `node_modules`, `target`, `.git`, `dist`; skips binary-looking files (NUL byte in the first 8192 bytes) and files over 2 MiB; wired as `check:secrets` and chained into `npm run check` immediately after `check:public`.
- **Surface rule — zero tolerance on portable surfaces:** every `*.toml` and `*.lock` file whose basename is not `Cargo.toml` or `Cargo.lock` must contain no match of:
  - a) `sk-[A-Za-z0-9_-]{16,}` — `openai-key-shape`
  - b) `AKIA[0-9A-Z]{16}` — `aws-access-key-shape` (except the documented sample below)
  - c) `Bearer\s+[A-Za-z0-9._-]{8,}` — `bearer-token-shape`
  - d) `\b(api[_-]?key|secret|token|credential)\s*=\s*"(?!\$\{|env:)[^"]{8,}"` (case-insensitive) — `credential-assignment-shape` (so `credential = "env:NAME"` and `credential = "${VAR}"` remain legal)
- **Repo-wide rule — credential shapes anywhere else:** every other text file must contain no match of:
  - e) `-----BEGIN [A-Z ]*PRIVATE KEY-----` — `private-key-block`
  - f) `ghp_[A-Za-z0-9]{30,}` — `github-pat-classic-shape`
  - g) `github_pat_[A-Za-z0-9_]{20,}` — `github-pat-fine-grained-shape`
  - h) `xox[baprs]-[A-Za-z0-9-]{10,}` — `slack-token-shape`
  - i) `sk-(proj|live|svcacct)-[A-Za-z0-9_-]{20,}` — `openai-project-key-shape`
  - j) `AKIA[0-9A-Z]{16}` — `aws-access-key-shape` (same shape as b, repo-wide)
- **Never echo secrets:** violations are reported as `path:line: pattern-name` only; the matched text is never printed.
- **Allowlisted fixture:** the exact documented AWS sample key `AKIAIOSFODNN7EXAMPLE` is allowed and does not count as a violation for patterns b/j.
- **Output:** on success `Secret-hygiene check passed: no credential-shaped values in portable or tracked surfaces.` exit 0; on violations a `Secret-hygiene violations:` list, exit 1.

## 2. Verification criteria → evidence

| Criterion                                              | Evidence                                                                                                                                                    | Status |
| ------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Gate exists and is wired into `npm run check`          | `scripts/check-secret-hygiene.mjs` + `package.json:check:secrets` chained after `check:public`; `npm run check:secrets` exit 0                              | pass   |
| Gate fails loudly on a planted credential-shaped value | fail-first probe: planting any of patterns a–j in the corresponding surface produces `Secret-hygiene violations:` with `path:line: pattern-name` and exit 1 | pass   |
| No false positives on current tree                     | `node scripts/check-secret-hygiene.mjs` on the current tree: zero violations, success message, exit 0                                                       | pass   |
| Secrets never echoed in output                         | violation report shows pattern name + line only; matched text is never printed                                                                              | pass   |

## 3. Result

**The decision 68 §4 sweep is now a standing mechanical gate.** No product code changed; `check:secrets` enforces zero tolerance on portable surfaces and credential-shape patterns repo-wide, with the documented fake fixture allowed and secret text never echoed.
