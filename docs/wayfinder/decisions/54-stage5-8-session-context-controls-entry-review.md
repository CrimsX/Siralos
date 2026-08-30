---
title: "Stage 5.8 Session Context Controls Entry Review — Consumption of the Frozen Visibility Seam"
label: "wayfinder:decision"
type: entry-review
status: accepted
date: 2026-08-29
ticket: "54"
supersedes: []
---

# Decision 54 — Stage 5.8 Session Context Controls Entry Review

**Ticket:** [Ticket 54 — Stage 5.8 Session Context Controls Entry Review](../tickets/54-stage5-8-session-context-controls-entry-review.md)
**Maps to:** [Siralos Roadmap](../siralos-roadmap.md)

## Question

Decision 49 froze the pure Live/Pinned/Frozen control model and deferred
consumption surfaces. With 5.2 (applied profile at startup) and R7.5
(deterministic `/context` snapshots) in place, how does the applied
profile's context control narrow what the session claims about content,
and what is the frozen evidence shape?

## Contract (C1–C6, approved 2026-08-29)

| #   | Clause              | Frozen decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| --- | ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership           | `siralos-core::composition` owns the composition-level seam: the effective policy carries the applied profile's context control (default `Live`), plus pure decision/evidence/rendering helpers over the frozen 5.3 `evaluate_context_policy`. `siralos-adapters::profile_config` parses the additive `[profile.context]` key (kind + optional digest) through the 5.3 constructor; an invalid control makes the profile not applied (5.2 semantics). `siralos-cli` owns the wiring: the session holds the applied control and the `/context` surface evaluates it against the snapshot's content identity. |
| C2  | Authority invariant | Narrowing-only per decision 49 C2: a control can only narrow what Siralos claims about content — `Live` asserts nothing; `Pinned`-stale proceeds but is labelled stale with expected/actual; `Frozen`-stale refuses the claim use (typed refusal, truthful report-safe diagnostics). Absent key = `Live` (transparent, zero-config); an invalid/refused profile contributes no control (5.2 semantics).                                                                                                                                                                                                     |
| C3  | Posture             | Zero new fs write paths and zero spawn; evaluation is pure over in-memory digests. The snapshot identity is a deterministic digest over existing render state — no new I/O, no wall clock, nothing silently used under `Blocked`.                                                                                                                                                                                                                                                                                                                                                                           |
| C4  | Evidence            | Frozen differential subject `composition-context-control` ×4 at v46 over the composition seam: (1) absent/unset → live, transparent fresh-unbound; (2) pinned current → fresh, digest-bound; (3) pinned stale → stale but usable with expected/actual; (4) frozen stale → blocked with expected/actual. Expectations mechanism covers the new scenarios (decision 40 C7).                                                                                                                                                                                                                                   |
| C5  | Corpus mechanics    | Schema stays 3; corpus bumps v45 → v46 (292 → 296 files, inside the 384 cap); all four contract.mjs sites, protocol validator, and the strict-loader count assert (292 → 296; run `cargo test -p siralos-cli --lib --all-features` before commit per the map Notes reminder) move together.                                                                                                                                                                                                                                                                                                                 |
| C6  | Lean guardrails     | Additive profile schema change limited to `[profile.context]` (mirrors decision 51's `plugins` precedent); no new CLI commands (the control rides the existing `/context`); no lockfile change, no plugin-consumption change, no skill consumption surface; no multi-agent machinery (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                                                      |

## HITL answers (2026-08-29, recorded verbatim)

- **Q1 Scope** — approved: _"Full scope as drafted (Recommended)"_ —
  "Core seam + profile parse + `/context` gating + v46 subject —
  completes the 5.3 consumption loop the way 5.7 completed 5.5's."
- **Q2 Binding target** — approved: _"The /context snapshot identity
  (Recommended)"_ — "The control binds the deterministic digest of the
  rendered `/context` snapshot state; Pinned-stale renders labelled,
  Frozen-stale refuses the render with a typed refusal."
- **Q3 Subject matrix** — approved: _"×4 as drafted (Recommended)"_ —
  "(1) absent/unset → live transparent; (2) pinned current → fresh
  digest-bound; (3) pinned stale → stale but usable, labelled;
  (4) frozen stale → blocked with expected/actual."
- **Q4 Contract** — approved: _"Approve C1–C6 (Recommended)"_ — "Freezes
  decision 54; implementation authorized for this arc (gates, v46, docs
  atomic)."

**The Stage 5.8 implementation slice is authorized as the next implementation slice**
against this frozen contract and the 4 HITL answers above. Acceptance: gates
green, `composition-context-control` ×4 at required parity at v46, the
narrowing-only property unit-proven, `check:rust` green, docs atomic.

**Implemented at `6dc830e`** (2026-08-29): `siralos-core::composition` owns
`decide_context_control` over the frozen 5.3 evaluation with absent-control
transparency and digest-bound `ContextControlDecisionEvidence`;
`siralos-adapters::profile_config` parses the additive `[profile.context]`
key through the 5.3 constructor (malformed controls leave the profile
unapplied); `siralos-cli` holds the applied control and `/context` labels
Pinned-stale claims, refuses Frozen-stale ones, and stays byte-transparent
otherwise; corpus v46/296 files, expectations 57 records, audit 291/291
applicable required; docs at the follow-up docs commit.

## Self-loop verification

- Criterion: the frozen contract is complete and internally consistent
  with decisions 47–53 and ADR 0036's narrowing-only composition unit.
- Evidence: this document; ticket 54 with the C1–C6 draft; the HITL
  answers above recorded verbatim.
- Verdict: **PASS** — the Stage 5.8 contract is frozen; implementation
  authorized.
