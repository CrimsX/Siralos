# Decision — Stage 4.7 Profiling Entry Review — Bounded Run-Profiling Sessions over the Generic Runtime Boundary

**Wayfinder ticket:** [Stage 4.7 Profiling Entry Review](../tickets/45-stage4-7-profiling-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [Stage 4.6 QA Workflows Entry Review](44-stage4-6-qa-workflows-entry-review.md) (complete at `a83c2a4`, corpus v37) + [Stage 4.5 Controlled Interaction Entry Review](43-stage4-5-controlled-interaction-entry-review.md) (complete at `42ee5ab`)
**Decided:** 2026-08-28 (resolver session; HITL grilling over ticket 45's C1–C6 draft and the 4 open frontier questions)
**Status:** **PASS — Stage 4.7 contract frozen; authorized as next implementation slice**
**Self-loop ledger:** 4 criteria, one decision pass (verification below)

> Mirrors `decisions/44-stage4-6-qa-workflows-entry-review.md` (QA workflow contracts over the generic boundary, complete) and `decisions/08-stage4-entry-sequence.md` (frozen 7-step sequence, step 7 — the final arrow). No implementation lands here.

---

## Summary

HITL confirmed the **Stage 4.7 Profiling slice** with the bounded run-profiling-sessions framing: the fifth and final consumer of the identity-bound primitive gate. `siralos-core::runtime` gains a bounded profiling-session model (typed sampler sessions attached to a supervised run) routed through the existing generic decision order over the unchanged 13-kind taxonomy and 6-disposition table; a session with zero declared samples is a typed pairing refusal, mirroring the zero-step-workflow, one-shot-interaction, and headless-capture refusals. The profiling primitive is identity-bound and absent on this platform, so every otherwise-valid session reports typed `UNAVAILABLE`. The extend-the-Godot-profiler alternative was considered and **rejected**: the R8 Godot engine-profiler is a verified fail-closed domain surface that stays exactly where it is; step 7 is the generic boundary's final consumer. One frozen differential subject `run-profile` ×4 at corpus v38 (260 → 264 files, inside the 384 cap); nothing flips `unavailable`.

## 1. HITL answers (2026-08-28)

| #   | Frontier question   | Human answer                                                      |
| --- | ------------------- | ----------------------------------------------------------------- |
| 1   | Framing/slice       | **Run-profiling sessions** (sequence step 7)                      |
| 2   | Subject structure   | **`run-profile` ×4** at v38                                       |
| 3   | Fail-closed posture | **Confirm** C3 — nothing flips `unavailable`; no live process I/O |
| 4   | Contract approval   | **Approve C1–C6 as drafted**                                      |

## 2. Frozen contract (C1–C6, confirmed)

| #   | Clause                | Contract                                                                                                                                                                                                                                                                                                                                                                               |
| --- | --------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership             | The run-profiling model is generic: `siralos-core::runtime` gains the bounded profiling-session record; any domain-specific profiler (such as the R8 Godot engine-profiler) stays in its owning optional domain crate and is unchanged. `siralos-core` gains no domain code (decision 37 neutrality; core guardrails unchanged).                                                       |
| C2  | Boundary consumption  | Profiling sessions route through the existing generic decision order (validation → capability → staleness → budget → cancellation → primitive) over the closed 13-kind taxonomy and 6-disposition table — neither is extended; a session with zero declared samples is a typed pairing refusal, mirroring the zero-step-workflow, one-shot-interaction, and headless-capture refusals. |
| C3  | Fail-closed unchanged | The profiling primitive is identity-bound and absent on this platform: every otherwise-valid session reports typed `UNAVAILABLE`; no predicate reports available; zero spawn paths (grep-swept at promotion); no live process I/O — all sample content is injected request data.                                                                                                       |
| C4  | Evidence              | Profiling sessions produce bounded structured evidence: sample count, per-sample digests, and byte totals — digests and counts only, never raw sample payloads; budgets admit session bytes through the existing `RuntimeBudget` admission.                                                                                                                                            |
| C5  | Corpus mechanics      | Schema stays `3`; frozen subject `run-profile` ×4 (unavailable, resource-exceeded, cancelled, pairing-refusal — mirrors the qa-workflow ×4 shape); corpus bumps `v38` at the reconciliation commit (260 → 264 files, inside the 384 cap); new scenarios covered by the post-freeze expectations mechanism per decision 40 C7; injected clock/ports only; no network.                   |
| C6  | Lean guardrails       | No new failure kinds, no cap growth, no GUI/TUI runtime ownership, no approval-authority changes (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                     |

**The Stage 4.7 implementation slice is authorized as the next implementation slice** against this frozen contract and the 4 HITL answers above. **Implemented at `b206a4a`** (corpus v38/264 files, expectations 25 records, audit 259/259 applicable required; zero spawn paths preserved). The slice requires: the generic profiling-session model in `siralos-core::runtime` (C1–C2), fail-closed profiling dispositions (C3), bounded evidence with sample digests (C4), the frozen differential subject at corpus v38 (C5), and the lean boundaries (C6). Acceptance criteria A1–A5 per ticket 45 (gates green, subject at required parity, zero-spawn sweep, `check:rust` green, docs atomic). With this slice the frozen 7-step sequence (decision 08) is fully consumed; the record-complete update names the Stage-4 completion state and the next frontier per the map.

## Self-loop verification

| Criterion                                      | Direct evidence                                                                                                                                    | Status |
| ---------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| All 4 frontier questions answered by human     | §1 Q1–Q4 verbatim answers (2026-08-28)                                                                                                             | pass   |
| Grounded in the frozen sequence, not new scope | decision 08 step 7; gap grep-confirmed at the generic level; Godot-profiler alternative evaluated and rejected as an already-landed domain surface | pass   |
| Fail-closed posture explicitly preserved       | §2 C3; Q3 answer "Confirm"; profiling primitive absent; no live process I/O                                                                        | pass   |
| Lean guardrails explicit                       | §2 C6; ADR 0036 out-of-scope list respected; no taxonomy/limit growth; approval authority untouched                                                | pass   |
