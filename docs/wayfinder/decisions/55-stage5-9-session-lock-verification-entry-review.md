---
title: "Stage 5.9 Session Lock Verification Entry Review — Consumption of the Frozen Lock Seam"
label: "wayfinder:decision"
type: entry-review
status: accepted
date: 2026-08-29
ticket: "55"
supersedes: []
---

# Decision 55 — Stage 5.9 Session Lock Verification Entry Review

**Ticket:** [Ticket 55 — Stage 5.9 Session Lock Verification Entry Review](../tickets/55-stage5-9-session-lock-verification-entry-review.md)
**Maps to:** [Siralos Roadmap](../siralos-roadmap.md)

## Question

Decision 50 froze the `siralos.lock` seam (create, load, verify over
recomputed digests) but nothing consumes verification at a session
boundary. How does the session hold the on-disk lock to account at
startup, and what is the frozen evidence shape?

## Contract (C1–C6, approved 2026-08-29)

| #   | Clause              | Frozen decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| --- | ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership           | `siralos-core::composition` owns the pure lock-verification decision: `decide_lock_verification(stored, current)` → typed outcome (`missing` / `current` / `stale` / `invalid`) with digest-bound evidence and deterministic rendering; identity-bounds violations surface as `invalid` with the truthful reason. `siralos-adapters::lockfile` stays the only fs reader (`verify_workspace_lock`, unchanged). `siralos-cli` owns the wiring: at startup the session recomputes the current lock from the applied profile and the installed plugin records and reports the verification outcome. |
| C2  | Authority invariant | The lock never gates authority: verification only narrows what Siralos claims about workspace-state truthfulness. Missing = transparent; Current = verified; Stale/Invalid = a truthful host-side startup diagnostic naming expected/actual, and the session proceeds on live Host state — fail-closed on the lock's claims, never on the session.                                                                                                                                                                                                                                              |
| C3  | Posture             | Zero new fs write paths (the session only reads the existing lock through the unchanged 5.4 adapter) and zero spawn; the decision is pure over in-memory locks. No wall clock, nothing silently trusted.                                                                                                                                                                                                                                                                                                                                                                                        |
| C4  | Evidence            | Frozen differential subject `composition-lock-verify` ×4 at v47 over the pure seam: (1) missing → transparent; (2) current → verified; (3) stale → drift with expected/actual; (4) invalid → identity-bounds refusal. Expectations mechanism covers the new scenarios (decision 40 C7).                                                                                                                                                                                                                                                                                                         |
| C5  | Corpus mechanics    | Schema stays 3; corpus bumps v46 → v47 (296 → 300 files, inside the 384 cap); all four contract.mjs sites, protocol validator, and the strict-loader count assert (296 → 300; run `cargo test -p siralos-cli --lib --all-features` before commit per the map Notes reminder) move together.                                                                                                                                                                                                                                                                                                     |
| C6  | Lean guardrails     | No new CLI commands; no lock schema change; no plugin/profile schema change; no skill consumption surface; no multi-agent machinery (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                                                                                                                                                                                           |

## HITL answers (2026-08-29, recorded verbatim)

- **Q1 Scope** — approved: _"Full scope as drafted (Recommended)"_ —
  "Core decision seam + startup wiring + v47 subject — completes the 5.4
  consumption loop."
- **Q2 Session semantics** — approved: _"Host-side startup diagnostic
  (Recommended)"_ — "Missing/Current transparent; Stale/Invalid →
  truthful host-side diagnostic naming expected/actual; the session
  always proceeds on live Host state — the lock never gates authority."
- **Q3 Subject matrix** — approved: _"×4 as drafted (Recommended)"_ —
  "(1) missing → transparent; (2) current → verified; (3) stale → drift
  with expected/actual; (4) invalid → identity-bounds refusal."
- **Q4 Contract** — approved: _"Approve C1–C6 (Recommended)"_ — "Freezes
  decision 55; implementation authorized for this arc (gates, v47, docs
  atomic)."

**The Stage 5.9 implementation slice is authorized as the next implementation slice**
against this frozen contract and the 4 HITL answers above. Acceptance: gates
green, `composition-lock-verify` ×4 at required parity at v47, the
never-gates-authority property unit-proven, `check:rust` green, docs atomic.
**Implemented at `6e38804`** (reflow `c1c4081`; corpus v47/300 files,
expectations 61 records, audit 295/295 applicable required; zero spawn/fs-write
paths; the lock never gates authority — unit- and session-proven).

## Self-loop verification

- Criterion: the frozen contract is complete and internally consistent
  with decisions 47–54 and ADR 0036's lean composition model.
- Evidence: this document; ticket 55 with the C1–C6 draft; the HITL
  answers above recorded verbatim.
- Verdict: **PASS** — the Stage 5.9 contract is frozen; implementation
  authorized.
