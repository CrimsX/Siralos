---
title: "Stage 4.6 QA Workflows Entry Review - Bounded QA Workflow Contracts over the Generic Runtime Boundary"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: "PASS — Stage 4.6 contract frozen (HITL 2026-08-28: Q1 bounded workflow steps step 6; Q2 raise cap to 384, qa-workflow ×4 at v37; Q3 confirm nothing flips unavailable; Q4 approve C1–C6). Authorized as next implementation slice; see decision 44."
blockedBy: []
---

## Question

Stage 4.5 (Controlled Interaction, `42ee5ab`) is complete — corpus v36/**256
files**, audit 251/251 applicable required. The frozen Stage-4 sequence
(decision 08; `RUST_MIGRATION.md` 4.1→4.7) names **QA Workflows** as the next
arrow (step 6).

The term is frozen-docs-only: ADR 0035 lists it as a future layer, the
sequence lists it verbatim, and the archived TypeScript tree has no QA-workflow
surface (the only `qa` hits are executor milestone-manifest prose: "Runtime
QA, project execution, or runtime impact proof."). `siralos-core` has no QA
workflow concept today.

The sequence-consistent reading (recommended): after the host can supervise a
run (step 1), capture evidence (steps 2/4), and exchange interaction rounds
(step 5), the next host capability is **bounded QA workflow contracts** —
deterministic, ordered sequences of typed QA steps composed over the generic
runtime boundary, decided by the same decision order, so a workflow's planned
steps, budgets, and evidence shape are host-owned and digest-bound while its
execution stays fail-closed on this platform.

**A second decision is forced by arithmetic:** corpus v36 sits exactly at the
corpus scenario cap (`CONTRACT_LIMITS.scenarios = 256` in
`contract.mjs`; `MAX_SCENARIOS = 256` in `harness.rs`). Admitting a
new subject ×4 breaches the cap. The cap is candidate-side corpus integrity —
not the pinned v32 oracle — so raising it is a deliberate, decision-frozen
contract change on both sides, disclosed here.

Decide and freeze the Stage 4.6 slice: the QA workflow model's home, its
boundary consumption, the cap resolution, the frozen differential subject, and
what stays `unavailable`.

## Why this is a slice, not a cleanup

- Decision 08's frozen sequence names it as its own arrow; nothing in the tree
  owns it (grep-confirmed: no QA-workflow concept in `siralos-core`).
- It is the fourth consumer of the same identity-bound primitive gate — the
  gate generalizes without weakening (no new failure kinds, no new
  predicates that report available).
- The harness lesson (PROJECT_CONTEXT §17) applies: workflow composition is a
  typed control operation shared across surfaces — QA workflows reuse the
  decision table rather than adding a second one.
- The differential pipeline is proven at v34/v35/v36; the cap change is a
  mechanical two-constant reconciliation with decision provenance.

## Frozen contract (draft for HITL confirmation)

| #   | Clause                | Contract                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| --- | --------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership             | The QA workflow model is generic: `siralos-core::runtime` gains the bounded QA workflow record (ordered typed steps); any domain-specific QA step vocabulary would live in the owning optional domain crate consuming the generic model. `siralos-core` gains no domain code (decision 37 neutrality; core guardrails unchanged).                                                                                                                                                                                                                                          |
| C2  | Boundary consumption  | QA workflows route through the existing generic decision order (validation → capability → staleness → budget → cancellation → primitive) over the closed 13-kind taxonomy and 6-disposition table — neither is extended; a workflow with zero steps is a typed pairing refusal, mirroring the one-shot-interaction and headless-capture refusals.                                                                                                                                                                                                                          |
| C3  | Fail-closed unchanged | The QA-workflow execution primitive is identity-bound and absent on this platform: every otherwise-valid workflow reports typed `UNAVAILABLE`; no predicate reports available; zero spawn paths (grep-swept at promotion); no live process I/O — all step content is injected request data.                                                                                                                                                                                                                                                                                |
| C4  | Evidence              | QA workflows produce bounded structured evidence: step count, per-step digests, and byte totals — digests and counts only, never raw step payloads; budgets admit workflow bytes through the existing `RuntimeBudget` admission.                                                                                                                                                                                                                                                                                                                                           |
| C5  | Corpus mechanics      | Schema stays `3`; the corpus scenario cap is raised `256 → 384` in both `contract.mjs` `CONTRACT_LIMITS.scenarios` and `harness.rs` `MAX_SCENARIOS` (candidate-side integrity only; the pinned v32 oracle is untouched and the freeze evidence is unchanged); frozen subject `qa-workflow` ×4 (unavailable, resource-exceeded, cancelled, pairing-refusal — mirrors the interaction ×4 shape); corpus bumps `v37` at the reconciliation commit; new scenarios covered by the post-freeze expectations mechanism per decision 40 C7; injected clock/ports only; no network. |
| C6  | Lean guardrails       | No new failure kinds beyond the cap change, no GUI/TUI runtime ownership, no approval-authority changes (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                                                                                                                                                                                                  |

## HITL questions

1. **Framing/slice**: freeze this as **Stage 4.6 — QA Workflows** (sequence
   step 6) with the bounded-workflow-steps framing above? Alternatives:
   reinterpret as task-kernel acceptance QA (already landed), or defer Stage 4.
2. **Corpus cap**: raise the scenario cap `256 → 384` (both constants) to
   admit `qa-workflow` ×4 at v37? Alternatives: skip the differential
   subject (unit tests only), or a different cap value.
3. **Posture**: confirm C3 — nothing flips `unavailable`; the QA-workflow
   execution primitive stays absent; no live process I/O.
4. **Contract**: approve C1–C6 as drafted, or amend.
