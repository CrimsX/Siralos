# Decision — Stage 4.5 Controlled Interaction Entry Review — Bounded Interaction Rounds over the Generic Runtime Boundary

**Wayfinder ticket:** [Stage 4.5 Controlled Interaction Entry Review](../tickets/43-stage4-5-controlled-interaction-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [Stage 4.4 Visual Evidence Entry Review](42-stage4-4-visual-evidence-entry-review.md) (complete at `4a250d8`, corpus v35) + [Godot Runtime Adapter Entry Review](41-godot-runtime-adapter-entry-review.md) (complete at `5bedf57`)
**Decided:** 2026-08-28 (resolver session; HITL grilling over ticket 43's C1–C6 draft and the 4 open frontier questions)
**Status:** **PASS — Stage 4.5 contract frozen; authorized as next implementation slice**
**Self-loop ledger:** 4 criteria, one decision pass (verification below)

> Mirrors `decisions/42-stage4-4-visual-evidence-entry-review.md` (evidence lifecycle over the generic boundary, complete) and `decisions/08-stage4-entry-sequence.md` (frozen 7-step sequence, step 5). No implementation lands here.

---

## Summary

HITL confirmed the **Stage 4.5 Controlled Interaction slice** with the run-interaction-rounds framing: the third consumer of the same identity-bound primitive gate. `siralos-core::runtime` gains a bounded interaction-round model (typed request/response exchange rounds with a supervised run) routed through the existing generic decision order over the unchanged 13-kind taxonomy and 6-disposition table; interaction against a non-interactive (one-shot) run is a typed pairing refusal mirroring the headless-capture refusal. The interactive primitive is identity-bound and absent on this platform, so every otherwise-valid interaction reports typed `UNAVAILABLE`. The approval-protocol alternative was considered and **rejected as already landed** (one-time digest-bound approvals at parity, security-permissions ×10, R13.1). One frozen differential subject `run-interaction` ×4 at corpus v36; nothing flips `unavailable`.

## 1. HITL answers (2026-08-28)

| #   | Frontier question   | Human answer                                                      |
| --- | ------------------- | ----------------------------------------------------------------- |
| 1   | Framing/slice       | **Run-interaction rounds** (sequence step 5)                      |
| 2   | Subject structure   | **`run-interaction` ×4** at v36                                   |
| 3   | Fail-closed posture | **Confirm** C3 — nothing flips `unavailable`; no live process I/O |
| 4   | Contract approval   | **Approve C1–C6 as drafted**                                      |

## 2. Frozen contract (C1–C6, confirmed)

| #   | Clause                | Contract                                                                                                                                                                                                                                                                                                                                                |
| --- | --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership             | The interaction-round model is generic: `siralos-core::runtime` gains the bounded interaction round record; any domain-specific interaction detail would live in the owning optional domain crate consuming the generic model. `siralos-core` gains no domain code (decision 37 neutrality; core guardrails unchanged).                                 |
| C2  | Boundary consumption  | Interaction rounds route through the existing generic decision order (validation → capability → staleness → budget → cancellation → primitive) over the closed 13-kind taxonomy and 6-disposition table — neither is extended; interaction against a non-interactive (one-shot) run is a typed pairing refusal, mirroring the headless-capture refusal. |
| C3  | Fail-closed unchanged | The interactive primitive is identity-bound and absent on this platform: every otherwise-valid interaction reports typed `UNAVAILABLE`; no predicate reports available; zero spawn paths (grep-swept at promotion); no live process I/O — all round content is injected request data.                                                                   |
| C4  | Evidence              | Interaction runs produce bounded structured evidence: round count, per-round request digests, and byte totals — digests and counts only, never raw transcript streams; budgets admit round bytes through the existing `RuntimeBudget` admission.                                                                                                        |
| C5  | Corpus mechanics      | Schema stays `3`; frozen subject `run-interaction` ×4 (unavailable, resource-exceeded, cancelled, pairing-refusal — mirrors the visual-evidence ×4 shape); corpus bumps `v36` at the reconciliation commit; new scenarios covered by the post-freeze expectations mechanism per decision 40 C7; injected clock/ports only; no network.                  |
| C6  | Lean guardrails       | No new failure kinds, no cap growth, no GUI/TUI runtime ownership, no approval-authority changes (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                      |

**The Stage 4.5 implementation slice is authorized as the next implementation slice** against this frozen contract and the 4 HITL answers above. **Implemented at `42ee5ab`** (corpus v36/256 files, expectations 17 records, audit 251/251 applicable required; zero spawn paths preserved). The slice requires: the generic interaction-round model in `siralos-core::runtime` (C1–C2), fail-closed interaction dispositions (C3), bounded evidence with round digests (C4), the frozen differential subject at corpus v36 (C5), and the lean boundaries (C6). Acceptance criteria A1–A5 per ticket 43 (gates green, subject at required parity, zero-spawn sweep, `check:rust` green, docs atomic).

## Self-loop verification

| Criterion                                      | Direct evidence                                                                                                                                                     | Status |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| All 4 frontier questions answered by human     | §1 Q1–Q4 verbatim answers (2026-08-28)                                                                                                                              | pass   |
| Grounded in the frozen sequence, not new scope | decision 08 step 5; ADR 0035 names the layer; gap grep-confirmed in `siralos-core::runtime`; approval-protocol alternative evaluated and rejected as already landed | pass   |
| Fail-closed posture explicitly preserved       | §2 C3; Q3 answer "Confirm"; interactive primitive absent; no live process I/O                                                                                       | pass   |
| Lean guardrails explicit                       | §2 C6; ADR 0036 out-of-scope list respected; no taxonomy/limit growth; approval authority untouched                                                                 | pass   |
