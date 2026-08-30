---
title: "Stage 5.9 Session Lock Verification Entry Review"
label: "wayfinder:ticket"
type: HITL
status: open
resolution: ""
blockedBy: []
---

# Ticket 55 — Stage 5.9 Session Lock Verification Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Decision:** [Stage 5.9 Session Lock Verification Entry Review](../decisions/55-stage5-9-session-lock-verification-entry-review.md) (opens after HITL grilling)

## Why now

Decision 50 froze the `siralos.lock` seam — deterministic idempotent
creation, load that re-derives the lock digest, and verification that
compares recomputed digests never file bytes — but nothing consumes
verification at a session boundary yet. The consumer now exists: the
session composes the applied profile at startup (decision 48) and reads
the installed plugin records (decisions 38/39), so it can recompute the
current lock and hold the on-disk lock to account. This slice completes
the loop; skill binding still waits for a skill consumption surface.

## Contract draft (C1–C6)

| #   | Clause              | Draft                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| --- | ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership           | `siralos-core::composition` owns the pure lock-verification decision: `decide_lock_verification(stored, current)` → typed outcome (`missing` / `current` / `stale` / `invalid`) with digest-bound evidence and deterministic rendering; identity-bounds violations surface as `invalid` with the truthful reason. `siralos-adapters::lockfile` stays the only fs reader (`verify_workspace_lock`, unchanged). `siralos-cli` owns the wiring: at startup the session recomputes the current lock from the applied profile and the installed plugin records and reports the verification outcome. |
| C2  | Authority invariant | The lock never gates authority: verification only narrows what Siralos claims about workspace-state truthfulness. Missing = transparent; Current = verified; Stale/Invalid = a truthful host-side startup diagnostic naming expected/actual, and the session proceeds on live Host state — fail-closed on the lock's claims, never on the session.                                                                                                                                                                                                                                              |
| C3  | Posture             | Zero new fs write paths (the session only reads the existing lock through the unchanged 5.4 adapter) and zero spawn; the decision is pure over in-memory locks. No wall clock, nothing silently trusted.                                                                                                                                                                                                                                                                                                                                                                                        |
| C4  | Evidence            | Frozen differential subject `composition-lock-verify` ×4 at v47 over the pure seam: (1) missing → transparent; (2) current → verified; (3) stale → drift with expected/actual; (4) invalid → identity-bounds refusal. Expectations mechanism covers the new scenarios (decision 40 C7).                                                                                                                                                                                                                                                                                                         |
| C5  | Corpus mechanics    | Schema stays 3; corpus bumps v46 → v47 (296 → 300 files, inside the 384 cap); all four contract.mjs sites, protocol validator, and the strict-loader count assert (296 → 300; run `cargo test -p siralos-cli --lib --all-features` before commit per the map Notes reminder) move together.                                                                                                                                                                                                                                                                                                     |
| C6  | Lean guardrails     | No new CLI commands; no lock schema change; no plugin/profile schema change; no skill consumption surface; no multi-agent machinery (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                                                                                                                                                                                           |

## Open questions for HITL grilling

1. **Q1 Scope** — composition decision seam + session startup verification + differential `composition-lock-verify` ×4 at v47?
2. **Q2 Session semantics** — startup host-side diagnostic only (the lock never gates authority), vs additionally surfacing the outcome in an existing command render?
3. **Q3 Subject matrix** — the four cases as drafted (missing / current / stale / invalid)?
4. **Q4 Approval** — approve C1–C6 as drafted and authorize the implementation slice?
