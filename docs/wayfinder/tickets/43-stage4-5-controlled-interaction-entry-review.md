---
title: "Stage 4.5 Controlled Interaction Entry Review - Bounded Interaction Rounds over the Generic Runtime Boundary"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: "PASS — Stage 4.5 contract frozen (HITL 2026-08-28: Q1 run-interaction rounds step 5; Q2 run-interaction ×4 at v36; Q3 confirm nothing flips unavailable; Q4 approve C1–C6). Authorized as next implementation slice; see decision 43."
blockedBy: []
---

## Question

Stage 4.3 (Godot Runtime Adapter, `5bedf57`) and Stage 4.4 (Visual Evidence,
`4a250d8`) are complete — corpus v35/252 files, audit 247/247 applicable
required. The frozen Stage-4 sequence (decision 08; `RUST_MIGRATION.md`
4.1→4.7) names **Controlled Interaction** as the next arrow (step 5).

The term is deliberately underspecified in the frozen docs: ADR 0035 names it
only as a future layer ("the generic boundary before the Godot adapter, visual
evidence, controlled interaction, QA, or profiling layers"), and the archived
TypeScript tree has no interaction concept. `siralos-core::runtime` has no
interaction surface today.

The sequence-consistent reading (recommended): after a host can supervise a run
(step 1) and capture evidence of it (steps 2/4), the next host capability is
**bounded interaction rounds with a supervised run** — typed request/response
exchanges (write a bounded stdin line, read a bounded stdout response) under
the same decision order, closed taxonomy, budgets, and the same identity-bound
primitive gate that launches and captures already respect. Everything stays
typed `unavailable` on this platform.

An alternative reading — "controlled interaction" as the human-approval
protocol — already landed: one-time digest-bound approvals are at parity
(security-permissions ×10, R13.1) and `ask` handling is refused where no
approval protocol exists.

Decide and freeze the Stage 4.5 slice: the interaction-round model's home, its
boundary consumption, the frozen differential subject, and what stays
`unavailable`.

## Why this is a slice, not a cleanup

- Decision 08's frozen sequence names it as its own arrow; nothing in the tree
  owns it (grep-confirmed: no interaction/stdin concept in
  `siralos-core::runtime`).
- It is the third consumer of the same identity-bound primitive gate — the
  gate generalizes without weakening (no new failure kinds, no new
  predicates that report available).
- The harness lesson (PROJECT_CONTEXT §17) applies: "typed control operations
  are shared across surfaces" — interaction rounds reuse the decision table
  rather than adding a second one.
- The differential pipeline (expectations mechanism per decision 40 C7) is
  proven at v34/v35; one more subject is a mechanical reconciliation.

## Frozen contract (draft for HITL confirmation)

| #   | Clause                | Contract                                                                                                                                                                                                                                                                                                                                                |
| --- | --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership             | The interaction-round model is generic: `siralos-core::runtime` gains the bounded interaction round record; any domain-specific interaction detail would live in the owning optional domain crate consuming the generic model. `siralos-core` gains no domain code (decision 37 neutrality; core guardrails unchanged).                                 |
| C2  | Boundary consumption  | Interaction rounds route through the existing generic decision order (validation → capability → staleness → budget → cancellation → primitive) over the closed 13-kind taxonomy and 6-disposition table — neither is extended; interaction against a non-interactive (one-shot) run is a typed pairing refusal, mirroring the headless-capture refusal. |
| C3  | Fail-closed unchanged | The interactive primitive is identity-bound and absent on this platform: every otherwise-valid interaction reports typed `UNAVAILABLE`; no predicate reports available; zero spawn paths (grep-swept at promotion); no live process I/O — all round content is injected request data.                                                                   |
| C4  | Evidence              | Interaction runs produce bounded structured evidence: round count, per-round request/response digests, and byte totals — digests and counts only, never raw transcript streams; budgets admit round bytes through the existing `RuntimeBudget` admission.                                                                                               |
| C5  | Corpus mechanics      | Schema stays `3`; frozen subject `run-interaction` ×4 (unavailable, resource-exceeded, cancelled, pairing-refusal — mirrors the visual-evidence ×4 shape); corpus bumps `v36` at the reconciliation commit; new scenarios covered by the post-freeze expectations mechanism per decision 40 C7; injected clock/ports only; no network.                  |
| C6  | Lean guardrails       | No new failure kinds, no cap growth, no GUI/TUI runtime ownership, no approval-authority changes (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                      |

## HITL questions

1. **Framing/slice**: freeze this as **Stage 4.5 — Controlled Interaction**
   (sequence step 5) with the run-interaction-rounds framing above?
   Alternatives: approval-protocol framing instead, or defer Stage 4.
2. **Subjects**: `run-interaction` ×4 at corpus v36 as drafted? Alternatives:
   different counts, or skip the differential subject.
3. **Posture**: confirm C3 — nothing flips `unavailable`; the interactive
   primitive stays absent; no live process I/O.
4. **Contract**: approve C1–C6 as drafted, or amend.
