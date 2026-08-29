---
title: "Stage 5.3 Context Controls Entry Review - Live/Pinned/Frozen Visibility over Content Identity"
label: "wayfinder:decision"
date: 2026-08-28
status: accepted
ticket: "Stage 5.3 Context Controls Entry Review"
adr: "ADR 0036"
---

# Decision 49 — Stage 5.3 Context Controls Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Ticket:** [Stage 5.3 Context Controls Entry Review](../tickets/49-stage5-3-context-controls-entry-review.md)

## Question on the table

Does Stage 5.3 land the declarative per-content control that states how
tightly Siralos binds what it gives the model — Live / Pinned / Frozen over
content identity — as a core-only `siralos-core::context::controls` module,
and what is the frozen evidence shape?

## Contract (C1–C6, approved 2026-08-28)

| #   | Clause              | Frozen decision                                                                                                                                                                                                                                                                                                                                                                     |
| --- | ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership           | `siralos-core::context::controls` owns `ContextPolicy` (`Live` / `Pinned {digest}` / `Frozen {digest}`) and `evaluate_context_policy(policy, actual_digest)` → `ContextControlOutcome` (`Fresh`, `Stale {expected, actual}`, `Blocked {expected, actual}`). Pure declarative data over the artifact-digest primitive; no new adapters, no CLI command, no filesystem.               |
| C2  | Authority invariant | A control can only narrow what Siralos claims about content: `Live` asserts nothing and binds nothing; `Pinned`/`Frozen` bind a digest, and a mismatch is reported truthfully. `Frozen` + stale is a typed `Blocked` — the content is never silently used; `Pinned` + stale proceeds but is labelled stale in every render. A policy can never claim freshness it has not verified. |
| C3  | Posture             | Zero configuration stays valid: an absent control behaves as `Live` (typed, no binding). Staleness never mutates or deletes anything; `Blocked` refuses use and says why. No process launch, no network, no wall clock.                                                                                                                                                             |
| C4  | Evidence            | Frozen differential subject `context-controls` ×4 at v41: (1) live → fresh, unbound; (2) pinned current → fresh, digest-bound; (3) pinned stale → stale but usable with expected/actual; (4) frozen stale → blocked with expected/actual. Expectations mechanism covers the new scenarios (decision 40 C7).                                                                         |
| C5  | Corpus mechanics    | Schema stays 3; corpus bumps v40 → v41 (272 → 276 files, inside the 384 cap); all four contract.mjs sites, protocol validator, strict-loader asserts move together per the checklist.                                                                                                                                                                                               |
| C6  | Lean guardrails     | No lockfile, no Skills/Plugin consumption, no multi-agent machinery, no provider integration, no approval-authority change (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                        |

## HITL answers (2026-08-28, recorded verbatim)

- **Q1 Slice scope** — approved: _"Core-only controls module (Recommended)"_ —
  "The pure declarative policy model and its mechanical outcomes first;
  consumption surfaces wait for their own slices."
- **Q2 Subject matrix** — approved: _"context-controls x4 as drafted
  (Recommended)"_ — "Each case pins a control invariant: live asserts nothing,
  pinned reports stale truthfully, frozen refuses stale use."
- **Q3 Posture** — approved: _"Confirm (Recommended)"_ — "The control declares
  how tightly Siralos binds content; only Frozen claims immutability, so only
  Frozen blocks."
- **Q4 Contract** — approved: _"Approve C1-C6 as drafted (Recommended)"_ —
  "Freeze decision 49 and proceed to implement the controls module and v41
  reconciliation end to end."

## Resolution

**The Stage 5.3 implementation slice is authorized as the next implementation slice**
against this frozen contract and the 4 HITL answers above. **Implemented at
`ce3e7dc`** (corpus v41/276 files, expectations 37 records, audit 271/271
applicable required; zero spawn/fs paths in the module). Acceptance: gates
green, `context-controls` ×4 at required parity at v41, digest-bound evidence
over the single artifact-digest primitive, `check:rust` green, docs atomic.

## Self-loop verification

Ledger (criterion → evidence → verdict) recorded at record-complete in the map
and AGENTS.md; loop budget one pass + two repairs.
