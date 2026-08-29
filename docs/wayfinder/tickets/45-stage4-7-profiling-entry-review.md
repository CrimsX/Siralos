---
title: "Stage 4.7 Profiling Entry Review - Bounded Run-Profiling Sessions over the Generic Runtime Boundary"
label: "wayfinder:grilling"
type: HITL
status: open
resolution: ""
blockedBy: []
---

## Question

Stage 4.6 (QA Workflows, `a83c2a4`) is complete — corpus v37/**260 files**
under the raised cap, audit 255/255 applicable required. The frozen Stage-4
sequence (decision 08; `RUST_MIGRATION.md` 4.1→4.7) names **Profiling** as
the final arrow (step 7).

The term is frozen-docs-only at the generic level: the sequence lists it
verbatim, and the archived TypeScript tree's only profiling surface is the
**Godot engine profiler** (`packages/adapters/src/godot/profile/
engine-profiler.ts`) — a domain-specific discovery/profiling probe already
ported and verified in R8 as part of the fail-closed Godot surfaces. No
generic profiling concept exists in the runtime; `siralos-core` has none
today.

The sequence-consistent reading (recommended): after the host can supervise a
run (step 1), capture evidence (steps 2/4), exchange interaction rounds
(step 5), and compose QA workflows (step 6), the final host capability is
**bounded run-profiling sessions** — typed sampler sessions attached to a
supervised run (bounded sample declarations, typed metric-bucket digests),
decided by the same decision order, with session evidence digest-bound and
execution fail-closed on this platform. The Godot engine profiler stays
exactly where R8 put it; nothing is rehomed.

Decide and freeze the Stage 4.7 slice: the run-profiling model's home, its
boundary consumption, the frozen differential subject, and what stays
`unavailable`. Corpus headroom is ample (260 of 384).

## Why this is a slice, not a cleanup

- Decision 08's frozen sequence names it as its own arrow; nothing in the tree
  owns a generic profiling surface (grep-confirmed; the only profiler is the
  R8 Godot engine-profiler, deliberately untouched).
- It is the fifth consumer of the same identity-bound primitive gate — the
  gate generalizes without weakening (no new failure kinds, no new
  predicates that report available).
- The harness lesson (PROJECT_CONTEXT §17) applies: measurement sessions are
  typed control operations shared across surfaces — profiling reuses the
  decision table rather than adding a second one.
- The differential pipeline is proven at v34→v37; one more subject is a
  mechanical reconciliation.

## Frozen contract (draft for HITL confirmation)

| #   | Clause                | Contract                                                                                                                                                                                                                                                                                                                                                                               |
| --- | --------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership             | The run-profiling model is generic: `siralos-core::runtime` gains the bounded profiling-session record; any domain-specific profiler (such as the R8 Godot engine-profiler) stays in its owning optional domain crate and is unchanged. `siralos-core` gains no domain code (decision 37 neutrality; core guardrails unchanged).                                                       |
| C2  | Boundary consumption  | Profiling sessions route through the existing generic decision order (validation → capability → staleness → budget → cancellation → primitive) over the closed 13-kind taxonomy and 6-disposition table — neither is extended; a session with zero declared samples is a typed pairing refusal, mirroring the zero-step-workflow, one-shot-interaction, and headless-capture refusals. |
| C3  | Fail-closed unchanged | The profiling primitive is identity-bound and absent on this platform: every otherwise-valid session reports typed `UNAVAILABLE`; no predicate reports available; zero spawn paths (grep-swept at promotion); no live process I/O — all sample content is injected request data.                                                                                                       |
| C4  | Evidence              | Profiling sessions produce bounded structured evidence: sample count, per-sample digests, and byte totals — digests and counts only, never raw sample payloads; budgets admit session bytes through the existing `RuntimeBudget` admission.                                                                                                                                            |
| C5  | Corpus mechanics      | Schema stays `3`; frozen subject `run-profile` ×4 (unavailable, resource-exceeded, cancelled, pairing-refusal — mirrors the qa-workflow ×4 shape); corpus bumps `v38` at the reconciliation commit (260 → 264 files, inside the 384 cap); new scenarios covered by the post-freeze expectations mechanism per decision 40 C7; injected clock/ports only; no network.                   |
| C6  | Lean guardrails       | No new failure kinds, no cap growth, no GUI/TUI runtime ownership, no approval-authority changes (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                     |

## HITL questions

1. **Framing/slice**: freeze this as **Stage 4.7 — Profiling** (sequence
   step 7) with the bounded run-profiling-sessions framing above (Godot
   engine-profiler untouched)? Alternatives: extend the Godot profiler
   instead, or defer Stage 4.
2. **Subjects**: `run-profile` ×4 at corpus v38 as drafted? Alternatives:
   different counts, or skip the differential subject.
3. **Posture**: confirm C3 — nothing flips `unavailable`; the profiling
   primitive stays absent; no live process I/O.
4. **Contract**: approve C1–C6 as drafted, or amend.
