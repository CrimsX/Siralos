---
title: "Stage 6 Evolution & Stabilization Entry Review — Freeze the Bounded Evaluation Workflow Contract"
label: "wayfinder:decision"
type: entry-review
status: accepted
date: 2026-08-30
ticket: "58"
supersedes: []
---

# Decision 58 — Stage 6 Evolution & Stabilization Entry Review

**Ticket:** [Ticket 58 — Stage 6 Evolution & Stabilization Entry Review](../tickets/58-stage6-evolution-entry-review.md)
**Maps to:** [Siralos Roadmap](../siralos-roadmap.md)

## Question

Stage 5 — Composition is Verified at `c2c30f0` (ten slices, decisions 47–57). How is **Stage 6 — Evolution & Stabilization** (ADR 0036 §§44–48) frozen as a bounded, measurement-driven `/evolve` workflow so an executor can implement it without rediscovering scope, authority, or evaluation boundaries?

> **PASS — HITL 2026-08-30: C1–C6 approved** (scope 6.1–6.4, escalation Profile→Host with deletion preference, evidence over pure seams, contract frozen; authorized as next implementation frontier).

## Contract (C1–C6, approved 2026-08-30)

| #   | Clause              | Draft                                                                                                                                                                                                                                                                                                                                                                                                                    |
| --- | ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| C1  | Scope               | Freeze Stage 6 as **bounded, measurement-driven `/evolve` only** — slices 6.1 Evaluation Corpus & Baselines, 6.2 Evolve Workflow Core (`baseline → candidate → evaluation → comparison → reject\|propose` with `Profile → Context → Skill → Plugin → Host` escalation), 6.3 Skill / Plugin / Host Proposals, 6.4 Packaging & Release Stabilization. Exact slice boundaries advisory until each slice's own entry review. |
| C2  | Authority invariant | `/evolve` never grants authority, never mutates Host state silently, never claims stronger reproducibility than the provider permits. Proposals are bounded, redacted, Host-observed; acceptance requires explicit human/host gate. Lower-cost configurable layer preferred over Host code by construction.                                                                                                              |
| C3  | Posture             | Zero new `std::process`/`Command::new().spawn()` paths beyond typed `UNAVAILABLE`; corpus cap stays 384; no general Hooks, TaskGraph, Fleet, marketplaces, plugin dependency graphs, auto-acquisition, or GUI/TUI ownership (ADR 0036 §51). Fail-closed and core neutrality preserved.                                                                                                                                   |
| C4  | Evidence            | Each Stage 6 slice contributes a frozen differential subject over pure seams at successive corpus bumps `v49`→ within cap; audit `299/299 + new slices` at required parity with expectations coverage per decision 40 C7; unit- and session-proven invariants.                                                                                                                                                           |
| C5  | Corpus mechanics    | Schema stays 3; each slice bumps `CORPUS_VERSION` and strict-loader count asserts in `crates/siralos-cli/src/harness.rs` together with manifest version (all contract.mjs sites + protocol validator). Entry review itself bumps nothing.                                                                                                                                                                                |
| C6  | Lean guardrails     | No new ADR beyond 0036, no scope redraw, no Out-of-scope growth; `docs/wayfinder/siralos-roadmap.md: Out of scope` stays closed. One ticket per session; budget one coherent pass + up to two repairs.                                                                                                                                                                                                                   |

## HITL answers (2026-08-30, recorded verbatim)

- **Q1 Scope** — approved: _"Approve 6.1–6.4 as drafted"_ — bounded `/evolve` with four advisory slices (6.1 Evaluation Corpus & Baselines, 6.2 Evolve Workflow Core, 6.3 Skill/Plugin/Host Proposals, 6.4 Packaging & Release).
- **Q2 Escalation** — approved: _"Approve escalation as drafted"_ — `Profile → Context → Skill → Plugin → Host` with deletion preference (lower-cost configurable layer preferred).
- **Q3 Evidence** — approved: _"Approve evidence as drafted"_ — differential subjects over pure seams with expectations coverage per decision 40 C7 at successive corpus bumps within cap 384.
- **Q4 Contract** — approved: _"Approve C1–C6"_ — freezes decision 58; implementation of 6.1 authorized next.

**Stage 6 entry review is PASS — Stage 6 is authorized as the next implementation frontier** against this frozen contract and the 4 HITL answers above. Acceptance: C1–C6 frozen, lean guardrails held, no code/corpus bump in this decision.

## Self-loop verification

- Criterion: the frozen contract is complete and internally consistent with ADR 0036, the Verified Stage 5 map (c2c30f0), and the lean Host/Plugin/Skill invariants.
- Evidence: this document; ticket 58 with the C1–C6 draft; HITL answers above recorded verbatim.
- Verdict: **PASS** — the Stage 6 contract is frozen; 6.1 implementation authorized.

## Implementation record

_Authorized at 2026-08-30 — no code in this decision. Next slice 6.1 Evaluation Corpus & Baselines is authorized as the next implementation slice against the frozen C1–C6 contract._
