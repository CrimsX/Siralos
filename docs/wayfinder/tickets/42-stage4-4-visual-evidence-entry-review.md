---
title: "Stage 4.4 Visual Evidence Entry Review - Visual-Run Evidence Lifecycle over the Generic Runtime Boundary"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: "PASS — Stage 4.4 contract frozen (HITL 2026-08-28: Q1 Stage 4.4 Visual Evidence step 4; Q2 visual-evidence ×4 at v35; Q3 confirm nothing flips unavailable; Q4 approve C1–C6). Authorized as next implementation slice; see decision 42."
blockedBy: []
---

## Question

Stage 4.1 (generic Controlled Runtime Execution + Runtime Evidence, sequence
steps 1–2 folded) is Verified (decision 36) and the Godot Runtime Adapter —
sequence step 3, the first specialization consuming the generic boundary — is
**complete** at `5bedf57` (decision 41; corpus v34, audit 243/243). The frozen
Stage-4 sequence (decision 08; `RUST_MIGRATION.md` 4.1→4.7,
`PROJECT_CONTEXT.md` §15) names **Visual Evidence** as the next arrow.

What already exists at parity (R10c, mirrored from the TypeScript H3 layer at
`5da5cde:packages/core/src/runtime/{modes,readiness,doctor}.ts`):

- `RuntimeMode` = headless | visual (`siralos-core::runtime`); Godot
  availability never implies visual mode;
- deterministic visual-mode capability evaluation — blocked ("no display
  available"), degraded ("display availability unknown"), available — from
  injected display availability, never a live probe;
- the readiness doctor emits headless **and** visual manifests with digests.

What does **not** exist: a **visual-evidence lifecycle** — bounded,
digest-bound capture evidence for visual runs (frames/screenshots), admitted
by budgets, redacted, reconciled, and typed `unavailable` when the capture
primitive is absent. Decide and freeze the Stage 4.4 slice: where the
visual-evidence model lives, how it consumes the generic boundary, which
differential subjects freeze, and what stays `unavailable`.

## Why this is a slice, not a cleanup

- Decision 08's frozen sequence names Visual Evidence as its own arrow (step 4)
  after the Godot adapter; nothing in the tree owns it yet — the runtime
  evidence projection is stdout/stderr/exit-code shaped only (1 MiB bounded,
  artifact digest) and has no frame/capture concept.
- Visual capture is the second consumer of the same identity-bound primitive
  gate: the platform has no display/capture primitive, so every capture
  reports typed `UNAVAILABLE` exactly like launches do — the slice must
  generalize that gate without weakening it (no new failure kinds, no live
  display probing, no GUI ownership per ADR 0036's out-of-scope list).
- The differential harness now has an expectations mechanism (decision 40 C7,
  landed at `5bedf57`), so new post-freeze scenarios are addable with
  disclosed provenance; the corpus pipeline is proven at v34.
- `PROJECT_CONTEXT.md` §17 harness lesson applies directly: "read-only
  observation never manufactures observed state" — visual evidence records
  what a (future) visual run captured; they are never synthesized from
  readiness.

## Frozen contract (draft for HITL confirmation)

| #   | Clause                | Contract                                                                                                                                                                                                                                                                                                                                                     |
| --- | --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| C1  | Ownership             | The visual-evidence model is generic: `siralos-core::runtime` gains the capture-evidence record and mode dimension; any Godot-specific visual detail (engine/movie-writer profile) would live in `crates/siralos-godot` consuming the generic model. `siralos-core` gains no new Godot code (decision 37 neutrality; core guardrails unchanged).             |
| C2  | Boundary consumption  | Capture requests route through the existing generic decision order (validation → capability → staleness → budget → cancellation → primitive) over the closed 13-kind taxonomy and 6-disposition table — neither is extended; visual mode rides the existing `RuntimeMode` and injected display-availability inputs.                                          |
| C3  | Fail-closed unchanged | The capture primitive is identity-bound and absent on this platform: every capture reports typed `UNAVAILABLE`; `is_identity_bound_launch_primitive_available()` stays `false` and no new predicate reports available; zero spawn paths (grep-swept at promotion); no live display probing — readiness semantics (blocked/degraded/available) stay injected. |
| C4  | Evidence              | Visual runs produce the generic bounded evidence projection plus visual-specific structured detail (mode, frame count, per-frame digests, total captured bytes) — digests and counts only, never raw pixel/frame bytes in evidence; budgets admit capture bytes through the existing `RuntimeBudget` admission.                                              |
| C5  | Corpus mechanics      | Schema stays `3`; proposed frozen subject `visual-evidence` ×4 (generic; fixture counts owned by the implementation, mirrors decision 35's `runtime-evidence` ×4); corpus bumps `v35` at the reconciliation commit; new scenarios covered by the post-freeze expectations mechanism per decision 40 C7; injected clock/ports only; no network.               |
| C6  | Lean guardrails       | No new failure kinds, no caps growth beyond existing limits, no GUI/TUI runtime ownership, no plugin/marketplace machinery (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                 |

## HITL questions

1. **Slice number/step**: freeze this as **Stage 4.4 — Visual Evidence**
   (sequence step 4, after the 4.3 adapter)? Alternatives: defer Stage 4
   further, or jump to a different arrow (Controlled Interaction, QA
   Workflows, Profiling) first.
2. **Subjects**: `visual-evidence` ×4 generic-only at corpus v35 as drafted?
   Alternatives: also a `godot-visual-evidence` specialization subject now;
   or different counts.
3. **Posture**: confirm C3 — nothing flips `unavailable`; capture stays typed
   `UNAVAILABLE`; no display probing.
4. **Contract**: approve C1–C6 as drafted, or amend.
