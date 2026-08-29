---
title: "Stage 5.4 siralos.lock Resolution Entry Review - Machine-Generated Portable Identities"
label: "wayfinder:decision"
date: 2026-08-28
status: accepted
ticket: "Stage 5.4 siralos.lock Resolution Entry Review"
adr: "ADR 0036"
---
# Decision 50 — Stage 5.4 siralos.lock Resolution Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Ticket:** [Stage 5.4 siralos.lock Resolution Entry Review](../tickets/50-stage5-4-lock-resolution-entry-review.md)

## Question on the table

Does Stage 5.4 complete the ADR 0036 §8-9 pair — human-authored
`siralos.toml` plus machine-generated `siralos.lock` — by resolving the
workspace's declarations (profile digest, plugin records) into deterministic,
digest-bound lock identities with a verify-vs-regenerate flow, and what is the
frozen evidence shape?

## Contract (C1–C6, approved 2026-08-28)

| # | Clause | Frozen decision |
| --- | --- | --- |
| C1 | Ownership | `siralos-core::composition::lock` owns the lock model: `LockIdentity` (profile digest, sorted plugin records, workspace declarations digest) + `create_workspace_lock` + deterministic canonical serialization. `siralos-adapters` owns `load_workspace_lock` / `write_workspace_lock` via the established atomic-write pattern, and `verify_workspace_lock` (current / stale / missing). No new write primitives. |
| C2 | Authority invariant | The lock is derived data: it binds digests of declarations the workspace already holds and grants nothing. Regeneration is deterministic and idempotent (same declarations → byte-identical lock). A lock can never include an identity the workspace did not declare, and verification compares the recomputed lock digest, not file bytes alone. |
| C3 | Posture | Missing lock = typed missing (generation is always available). Stale lock = truthful stale report with expected/actual lock digests; nothing is silently rewritten. The lock write uses the same lstat-verified regular-file + rename pattern as the plugin record writer (symlink at target replaced by rename, never followed; any failure removes the temp file). |
| C4 | Evidence | Frozen differential subject `composition-lock` ×4 at v42 over the pure resolution seam: (1) profile-only workspace → lock with profile identity; (2) workspace with plugin records → sorted identities included; (3) verify current → match; (4) verify stale → expected/actual digests. Expectations mechanism covers the new scenarios (decision 40 C7); the fs write/load paths are covered by adapters unit tests. |
| C5 | Corpus mechanics | Schema stays 3; corpus bumps v41 → v42 (276 → 280 files, inside the 384 cap); all four contract.mjs sites, protocol validator, and the strict-loader count assert (now 276 → 280 — see the map Notes reminder: run `cargo test -p siralos-cli --lib --all-features` before commit) move together per the checklist. |
| C6 | Lean guardrails | No acquisition/installation machinery, no network, no multi-agent machinery, no approval-authority change (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete. |

## HITL answers (2026-08-28, recorded verbatim)

- **Q1 Slice scope** — approved: *"Core model + adapters + atomic write
  (Recommended)"* — "Completes the ADR 0036 sec 8-9 pair: deterministic
  regeneration, verify-vs-regenerate flow, one proven write pattern."
- **Q2 Subject matrix** — approved: *"composition-lock x4 as drafted
  (Recommended)"* — "Covers identity resolution, sorted plugin inclusion, and
  truthful verification outcomes."
- **Q3 Posture** — approved: *"Confirm (Recommended)"* — "The lock is derived
  data with one proven write pattern; verification compares recomputed digests,
  never file bytes alone."
- **Q4 Contract** — approved: *"Approve C1-C6 as drafted (Recommended)"* —
  "Freeze decision 50 and proceed to implement the lock model, adapters, and
  v42 reconciliation end to end."

## Resolution

**The Stage 5.4 implementation slice is authorized as the next implementation slice**
against this frozen contract and the 4 HITL answers above. Acceptance: gates
green, `composition-lock` ×4 at required parity at v42, deterministic
idempotent regeneration with unit-tested atomic writes, `check:rust` green,
docs atomic.

## Self-loop verification

Ledger (criterion → evidence → verdict) recorded at record-complete in the map
and AGENTS.md; loop budget one pass + two repairs.
