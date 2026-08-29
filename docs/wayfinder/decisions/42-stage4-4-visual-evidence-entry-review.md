# Decision — Stage 4.4 Visual Evidence Entry Review — Visual-Run Evidence Lifecycle over the Generic Runtime Boundary

**Wayfinder ticket:** [Stage 4.4 Visual Evidence Entry Review](../tickets/42-stage4-4-visual-evidence-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [Godot Runtime Adapter Entry Review](41-godot-runtime-adapter-entry-review.md) (complete at `5bedf57`, corpus v34) + [TypeScript Archive Removal](40-typescript-archive-removal.md) (complete, expectations mechanism landed)
**Decided:** 2026-08-28 (resolver session; HITL grilling over ticket 42's C1–C6 draft and the 4 open frontier questions)
**Status:** **PASS — Stage 4.4 contract frozen; authorized as next implementation slice**
**Implemented:** `4a250d8` (corpus v35/252 files, expectations 13 records, audit 247/247 applicable required; zero spawn preserved)
**Self-loop ledger:** 4 criteria, one decision pass (verification below)

> Mirrors `decisions/41-godot-runtime-adapter-entry-review.md` (first specialization, complete) and `decisions/08-stage4-entry-sequence.md` (frozen 7-step sequence, step 4). No implementation lands here.

---

## Summary

HITL confirmed the **Stage 4.4 Visual Evidence slice**: the second arrow of the frozen Stage-4 sequence after the Godot adapter. Visual-mode **readiness** is already at parity (R10c: `RuntimeMode` headless|visual, injected display availability, blocked/degraded/available, doctor manifests); the slice adds the missing **visual-evidence lifecycle** — a bounded, digest-bound capture-evidence record admitted through the existing generic decision order, with the capture primitive identity-bound and absent on this platform so every capture reports typed `UNAVAILABLE`. Generic model lives in `siralos-core::runtime`; one frozen differential subject `visual-evidence` ×4 at corpus v35; nothing flips `unavailable`.

## 1. HITL answers (2026-08-28)

| #   | Frontier question   | Human answer                                                                                      |
| --- | ------------------- | ------------------------------------------------------------------------------------------------- |
| 1   | Slice number/step   | **Stage 4.4 Visual Evidence** (sequence step 4)                                                   |
| 2   | Subject structure   | **`visual-evidence` ×4** generic-only at v35                                                      |
| 3   | Fail-closed posture | **Confirm** C3 — nothing flips `unavailable`; no live display probing; zero spawn paths preserved |
| 4   | Contract approval   | **Approve C1–C6 as drafted**                                                                      |

## 2. Frozen contract (C1–C6, confirmed)

| #   | Clause                | Contract                                                                                                                                                                                                                                                                                                                                                     |
| --- | --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| C1  | Ownership             | The visual-evidence model is generic: `siralos-core::runtime` gains the capture-evidence record and mode dimension; any Godot-specific visual detail (engine/movie-writer profile) would live in `crates/siralos-godot` consuming the generic model. `siralos-core` gains no new Godot code (decision 37 neutrality; core guardrails unchanged).             |
| C2  | Boundary consumption  | Capture requests route through the existing generic decision order (validation → capability → staleness → budget → cancellation → primitive) over the closed 13-kind taxonomy and 6-disposition table — neither is extended; visual mode rides the existing `RuntimeMode` and injected display-availability inputs.                                          |
| C3  | Fail-closed unchanged | The capture primitive is identity-bound and absent on this platform: every capture reports typed `UNAVAILABLE`; `is_identity_bound_launch_primitive_available()` stays `false` and no new predicate reports available; zero spawn paths (grep-swept at promotion); no live display probing — readiness semantics (blocked/degraded/available) stay injected. |
| C4  | Evidence              | Visual runs produce the generic bounded evidence projection plus visual-specific structured detail (mode, frame count, per-frame digests, total captured bytes) — digests and counts only, never raw pixel/frame bytes in evidence; budgets admit capture bytes through the existing `RuntimeBudget` admission.                                              |
| C5  | Corpus mechanics      | Schema stays `3`; frozen subject `visual-evidence` ×4 (generic; fixture counts owned by the implementation, mirrors decision 35's `runtime-evidence` ×4); corpus bumps `v35` at the reconciliation commit; new scenarios covered by the post-freeze expectations mechanism per decision 40 C7; injected clock/ports only; no network.                        |
| C6  | Lean guardrails       | No new failure kinds, no caps growth beyond existing limits, no GUI/TUI runtime ownership, no plugin/marketplace machinery (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                 |

**The Stage 4.4 implementation slice is authorized as the next implementation slice** against this frozen contract and the 4 HITL answers above. The slice requires: generic capture-evidence model in `siralos-core::runtime` (C1–C2), fail-closed capture dispositions (C3), bounded evidence with visual-structured detail (C4), the frozen differential subject at corpus v35 (C5), and the lean boundaries (C6). Acceptance criteria A1–A5 per ticket 42 (gates green, subject at required parity, zero-spawn sweep, `check:rust` green, docs atomic).

## Self-loop verification

| Criterion                                      | Direct evidence                                                                                                                                | Status |
| ---------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| All 4 frontier questions answered by human     | §1 Q1–Q4 verbatim answers (2026-08-28)                                                                                                         | pass   |
| Grounded in the frozen sequence, not new scope | decision 08 step 4; `RUST_MIGRATION.md` sequence; visual readiness already at parity since R10c; gap named (evidence lifecycle, not readiness) | pass   |
| Fail-closed posture explicitly preserved       | §2 C3; Q3 answer "Confirm"; identity-bound capture primitive absent; no display probing                                                        | pass   |
| Lean guardrails explicit                       | §2 C6; ADR 0036 out-of-scope list respected; no taxonomy/limit growth                                                                          | pass   |
