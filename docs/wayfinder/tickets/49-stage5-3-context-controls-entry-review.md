---
title: "Stage 5.3 Context Controls Entry Review - Live/Pinned/Frozen Visibility over Content Identity"
label: "wayfinder:ticket"
type: HITL
status: closed
resolution: "PASS per decision 49 (2026-08-28): C1-C6 approved; implemented as the Stage 5.3 slice."
blockedBy: []
---

# Ticket 49 — Stage 5.3 Context Controls Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Decision:** [Stage 5.3 Context Controls Entry Review](../decisions/49-stage5-3-context-controls-entry-review.md) (opens after HITL grilling)

## Why now

Stage 5.2 completed the profile resolution chain; the model-side complement is
context visibility (ADR 0036: profiles define how the model works — context
shows what Siralos gives the model). Existing seams: `siralos-core::context`
(R10b PhaseContract with narrowing-only authority, digest binding, provenance
refs, targeted staleness) and R10a content identity. The missing piece is a
declarative per-content control that states how tightly Siralos binds what it
gives the model: Live (re-read from source), Pinned (bound to a digest, stale
reported but usable), Frozen (bound to a digest, stale refuses use).

## Contract draft (C1–C6)

| #   | Clause              | Draft                                                                                                                                                                                                                                                                                                                                                                               |
| --- | ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership           | `siralos-core::context::controls` owns `ContextPolicy` (`Live` / `Pinned {digest}` / `Frozen {digest}`) and `evaluate_context_policy(policy, actual_digest)` → `ContextControlOutcome` (`Fresh`, `Stale {expected, actual}`, `Blocked {expected, actual}`). Pure declarative data over the artifact-digest primitive; no new adapters, no CLI command, no filesystem.               |
| C2  | Authority invariant | A control can only narrow what Siralos claims about content: `Live` asserts nothing and binds nothing; `Pinned`/`Frozen` bind a digest, and a mismatch is reported truthfully. `Frozen` + stale is a typed `Blocked` — the content is never silently used; `Pinned` + stale proceeds but is labelled stale in every render. A policy can never claim freshness it has not verified. |
| C3  | Posture             | Zero configuration stays valid: an absent control behaves as `Live` (typed, no binding). Staleness never mutates or deletes anything; `Blocked` refuses use and says why. No process launch, no network, no wall clock.                                                                                                                                                             |
| C4  | Evidence            | Frozen differential subject `context-controls` ×4 at v41: (1) live → fresh, unbound; (2) pinned current → fresh, digest-bound; (3) pinned stale → stale but usable with expected/actual; (4) frozen stale → blocked with expected/actual. Expectations mechanism covers the new scenarios (decision 40 C7).                                                                         |
| C5  | Corpus mechanics    | Schema stays 3; corpus bumps v40 → v41 (272 → 276 files, inside the 384 cap); all four contract.mjs sites, protocol validator, strict-loader asserts move together per the checklist.                                                                                                                                                                                               |
| C6  | Lean guardrails     | No lockfile, no Skills/Plugin consumption, no multi-agent machinery, no provider integration, no approval-authority change (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                        |

## Open questions for HITL grilling

1. **Q1 Scope** — core-only controls module, no adapters/CLI surface this slice?
2. **Q2 Subject matrix** — `context-controls` ×4 as drafted (live / pinned-current / pinned-stale / frozen-stale)?
3. **Q3 Posture** — confirm C3: absent control = Live, and Pinned-stale proceeds while Frozen-stale blocks (rather than both blocking)?
4. **Q4 Approval** — approve C1–C6 as drafted?
