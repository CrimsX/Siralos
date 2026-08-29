---
title: "Godot Runtime Adapter Entry Review - First Specialization of the Generic Runtime Boundary"
label: "wayfinder:grilling"
type: HITL
status: open
resolution: ""
blockedBy: []
---

## Question

Stage 4.1 (generic Controlled Runtime Execution) is Verified (decision 36):
`siralos-core::runtime::execution` hosts the 6-disposition decision table and
`siralos-core::runtime::evidence` the bounded evidence projection, with
`is_identity_bound_launch_primitive_available() → false` so every launch
reports typed `UNAVAILABLE` — 0 spawn. The frozen Stage-4 sequence
(decision 08; `RUST_MIGRATION.md` 4.1→4.7) names the **Godot Runtime Adapter**
as the next arrow: the first specialization that consumes the generic boundary.
Decide and freeze the adapter slice: what it ports, where it lives, which
differential subjects freeze, what stays `unavailable`, and its slice number.

## Why this is a slice, not a cleanup

- Decision 08's generic/Godot table reserves the adapter role: engine selection
  becomes a runtime input, the Godot project launches via the generic
  supervisor, and Godot never defines the process boundary, run-directory
  identity, or failure taxonomy — those are already at parity in
  `siralos-core::runtime` (R10c) and Stage 4.1.
- The 6+3 R8/R9 surfaces live in `crates/siralos-godot` (decision 37) with
  zero spawn paths mechanically preserved; the adapter adds the launch-shaped
  surface that those read-only/prepare-only surfaces deliberately lack.
- `PROJECT_CONTEXT.md` §2 defines the term but no contract yet: "Godot
  Runtime Adapter: optional Godot specialization of the generic runtime
  boundary, not runtime authority itself." This entry review freezes it.
- Numbering conflict to resolve: decision 08 and decision 35 number the
  adapter **4.3** (4.1-4.2 generic), but the landed 4.1 folded both
  `runtime-execution` and `runtime-evidence` subjects (sequence steps 1-2),
  leaving no 4.2 slice. The entry review must pin the number used going
  forward.

## Frozen contract (draft for HITL confirmation)

| #   | Clause                | Contract                                                                                                                                                                                                                                                                                                                                                          |
| --- | --------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership             | The adapter lives in `crates/siralos-godot` and consumes `siralos-core::runtime` + `siralos-adapters::runtime` seams; `siralos-core` gains no new Godot code (domain neutrality per decision 37; core symbol/dependency guardrails unchanged).                                                                                                                    |
| C2  | Boundary consumption  | Engine selection/version profiling is consumed as runtime input; launch requests route through the generic supervisor decision table (`success`/`COMMAND_DENIED`/`STALE`/`RESOURCE_EXCEEDED`/`CANCELLED`/`UNAVAILABLE`); the adapter maps engine-specific failure detail onto the closed 13-kind taxonomy without extending it.                                   |
| C3  | Fail-closed unchanged | On this platform the identity-bound launch primitive is absent, so every launch reports typed `UNAVAILABLE: identity-bound launch primitive not available`; zero spawn paths in any Godot module (grep-swept at promotion); fixed invocation tuples stay inside architecture-owned runner modules; recovery/check-only/LSP-only flags remain structurally paired. |
| C4  | Evidence              | Engine runs produce the generic bounded `RuntimeEvidence` projection (exit code, duration, 1 MiB-bounded stdout/stderr, truncated flag, artifact digest) plus Godot-specific structured detail (engine id/profile, project rel path, diagnostics digest) — never raw engine streams.                                                                              |
| C5  | Corpus mechanics      | Schema stays `3`; proposed frozen subjects `godot-runtime-launch` ×5 + `godot-runtime-evidence` ×4 (fixture counts owned by the implementation, mirrors decision 14); corpus bumps `v34` at the reconciliation commit; injected clock/ports only; no network.                                                                                                     |
| C6  | Lean guardrails       | No visual evidence, no input injection, no QA-workflow or profiling scope (those are sequence steps 4-7); no marketplace/auto-acquisition (ADR 0036); external `siralos-godot` repo stays **FUTURE**.                                                                                                                                                             |

## Open HITL questions

| #   | Question                          | Options                                                                                                                                                     |
| --- | --------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Q1  | Slice number                      | (a) **4.3** as frozen in decisions 08/35, with 4.2 explicitly noted as folded into 4.1 — default; (b) renumber to **4.2** and amend the decision references |
| Q2  | Subject names/counts              | (a) `godot-runtime-launch` ×5 + `godot-runtime-evidence` ×4 as drafted — default; (b) adjust                                                                |
| Q3  | Does anything flip `unavailable`? | (a) **Nothing flips** (default; posture C3 holds); (b) name a surface whose security property is mechanically enforceable and adversarially tested          |
| Q4  | Contract approval                 | (a) Approve C1-C6 as drafted; (b) amend                                                                                                                     |

## Acceptance criteria (implementation slice, after decision)

| #   | Criterion                           | Check                                                                        |
| --- | ----------------------------------- | ---------------------------------------------------------------------------- |
| A1  | Full gate green on the adapter tree | `npm run check` exit 0 observed at the reconciliation commit                 |
| A2  | New subjects at required parity     | differential audit covers the frozen subjects with 0 deviations              |
| A3  | Zero spawn paths preserved          | grep sweep over `crates/siralos-godot` at promotion returns zero spawn paths |
| A4  | Core domain neutrality unchanged    | `check:rust` green; no new core→domain edge                                  |
| A5  | Docs advanced atomically            | seven-surface pattern (`decisions/33` §3) as applicable                      |

## Not due in this ticket

No implementation lands here: no engine launch, no run directory, no
checkpoint, no corpus bump, no decision doc. The decision doc is written at
HITL resolution; the agent never answers for the human (guardrail h, template
[07](../decisions/07-r12-disposition.md)).

---

## Self-loop ledger (draft)

| Criterion                                        | Direct evidence                                                                                                         | Status |
| ------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------- | ------ |
| Grounded in frozen sequence, not new scope       | decision 08 generic/Godot table; decision 35 "the Godot Runtime Adapter (4.3) will later consume this generic boundary" | pass   |
| Existing posture preserved in the draft          | decision 36 (0 spawn, `UNAVAILABLE` const); decision 37 crate ownership; AGENTS.md fail-closed list                     | pass   |
| Numbering conflict surfaced, not silently chosen | "Why this is a slice" bullet 4 + Q1                                                                                     | pass   |
| No implementation authorized by this draft       | "Not due in this ticket" section; status `open`                                                                         | pass   |
