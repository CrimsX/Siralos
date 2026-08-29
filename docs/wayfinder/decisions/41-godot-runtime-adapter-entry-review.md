# Decision — Godot Runtime Adapter Entry Review — First Specialization of the Generic Runtime Boundary

**Wayfinder ticket:** [Godot Runtime Adapter Entry Review](../tickets/41-godot-runtime-adapter-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [Stage 4.1 Verified Promotion](36-stage4-1-verified-promotion.md) (PASS — generic boundary Verified, 0 spawn) + [TypeScript Archive Removal](40-typescript-archive-removal.md) (complete at `e6c49f1`, corpus v33)
**Decided:** 2026-08-28 (resolver session; HITL grilling over ticket 41's C1–C6 draft and the 4 open frontier questions)
**Status:** **PASS — Stage 4.3 contract frozen; authorized as next implementation slice**
**Self-loop ledger:** 4 criteria, one decision pass (verification below)

> Mirrors `decisions/35-stage4-1-entry-review.md` (generic boundary, Verified) and `decisions/08-stage4-entry-sequence.md` (frozen 7-step sequence, step 3). No implementation lands here.

---

## Summary

HITL confirmed the **Godot Runtime Adapter slice (Stage 4.3)**: the first specialization consuming the generic runtime boundary. The adapter lives in `crates/siralos-godot`, consumes the `siralos-core::runtime` + `siralos-adapters::runtime` seams, keeps every effect fail-closed (typed `UNAVAILABLE`, zero spawn), and freezes two differential subjects at corpus v34. Slice numbering stays **4.3** as frozen in decisions 08/35, with 4.2 explicitly recorded as folded into 4.1.

## 1. HITL answers (2026-08-28)

| #   | Frontier question   | Human answer                                                                                                                                                                            |
| --- | ------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Slice number        | **Keep frozen 4.3** (4.2 folded into 4.1, noted)                                                                                                                                        |
| 2   | Subject structure   | Approved as drafted — verbatim answer: "`godot-runtime-evidence ×4 at v34`"; C5's full structure (`godot-runtime-launch` ×5 + `godot-runtime-evidence` ×4) stands per Q4 draft approval |
| 3   | Fail-closed posture | **Nothing flips** `unavailable`                                                                                                                                                         |
| 4   | Contract approval   | **Approve C1–C6 as drafted**                                                                                                                                                            |

## 2. Frozen contract (C1–C6, confirmed)

| #   | Clause               | Contract                                                                                                                                                                                                                                                                                                                                                          |
| --- | -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership            | The adapter lives in `crates/siralos-godot` and consumes `siralos-core::runtime` + `siralos-adapters::runtime` seams; `siralos-core` gains no new Godot code (domain neutrality per decision 37; core symbol/dependency guardrails unchanged).                                                                                                                    |
| C2  | Boundary consumption | Engine selection/version profiling is consumed as runtime input; launch requests route through the generic supervisor decision table (`success`/`COMMAND_DENIED`/`STALE`/`RESOURCE_EXCEEDED`/`CANCELLED`/`UNAVAILABLE`); the adapter maps engine-specific failure detail onto the closed 13-kind taxonomy without extending it.                                   |
| C3  | Fail-closed posture  | On this platform the identity-bound launch primitive is absent, so every launch reports typed `UNAVAILABLE: identity-bound launch primitive not available`; zero spawn paths in any Godot module (grep-swept at promotion); fixed invocation tuples stay inside architecture-owned runner modules; recovery/check-only/LSP-only flags remain structurally paired. |
| C4  | Evidence             | Engine runs produce the generic bounded `RuntimeEvidence` projection (exit code, duration, 1 MiB-bounded stdout/stderr, truncated flag, artifact digest) plus Godot-specific structured detail (engine id/profile, project rel path, diagnostics digest) — never raw engine streams.                                                                              |
| C5  | Corpus mechanics     | Schema stays `3`; frozen subjects `godot-runtime-launch` ×5 + `godot-runtime-evidence` ×4 (fixture counts owned by the implementation, mirrors decision 14); corpus bumps `v34` at the reconciliation commit; injected clock/ports only; no network.                                                                                                              |
| C6  | Lean guardrails      | No visual evidence, no input injection, no QA-workflow or profiling scope (those are sequence steps 4-7); no marketplace/auto-acquisition (ADR 0036); external `siralos-godot` repo stays **FUTURE**.                                                                                                                                                             |

## 3. Authorization

**The Stage 4.3 implementation slice is authorized as the next implementation slice** against this frozen contract and the 4 HITL answers above. The slice requires: adapter seam consumption (C1–C2), fail-closed launch dispositions (C3), bounded evidence projection with Godot-structured detail (C4), the two frozen differential subjects at corpus v34 (C5), and the lean boundaries (C6). Acceptance criteria A1–A5 per ticket 41; one Verified promotion closes the slice.

## Self-loop verification

| Criterion                                        | Direct evidence                                                                                                                                   | Status |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| All 4 frontier questions answered by human       | §1 Q1–Q4 verbatim answers (2026-08-28)                                                                                                            | pass   |
| Grounded in the frozen sequence, not new scope   | decision 08 step 3; decision 35 "the Godot Runtime Adapter (4.3) will later consume this generic boundary"; decision 36 generic boundary Verified | pass   |
| Fail-closed posture explicitly preserved         | §2 C3; Q3 answer "nothing flips"; decision 36 `UNAVAILABLE` const + 0 spawn                                                                       | pass   |
| Numbering conflict resolved, not silently chosen | §1 Q1 "keep frozen 4.3"; 4.2 fold recorded in Summary and decision 08/35 references left intact                                                   | pass   |
