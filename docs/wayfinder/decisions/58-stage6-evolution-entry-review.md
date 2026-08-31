---
title: "Stage 6 Evolution & Stabilization Entry Review — Freeze the Bounded Evaluation Workflow Contract"
label: "wayfinder:decision"
type: entry-review
status: draft
date: 2026-08-30
ticket: "58"
supersedes: []
---

# Decision 58 — Stage 6 Evolution & Stabilization Entry Review

**Ticket:** [Ticket 58 — Stage 6 Evolution & Stabilization Entry Review](../tickets/58-stage6-evolution-entry-review.md)
**Maps to:** [Siralos Roadmap](../siralos-roadmap.md)

## Question

Stage 5 — Composition is Verified at `c2c30f0` (ten slices, decisions 47–57). How is **Stage 6 — Evolution & Stabilization** (ADR 0036 §§44–48) frozen as a bounded, measurement-driven `/evolve` workflow so an executor can implement it without rediscovering scope, authority, or evaluation boundaries?

> Draft — awaits HITL grilling. This file satisfies the Wayfinder link contract; C1–C6 are frozen only after HITL PASS.

## Contract (C1–C6, draft — pending HITL)

| #   | Clause              | Draft                                                                                                                                                                                                                                                                                                                                                                                                                    |
| --- | ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| C1  | Scope               | Freeze Stage 6 as **bounded, measurement-driven `/evolve` only** — slices 6.1 Evaluation Corpus & Baselines, 6.2 Evolve Workflow Core (`baseline → candidate → evaluation → comparison → reject\|propose` with `Profile → Context → Skill → Plugin → Host` escalation), 6.3 Skill / Plugin / Host Proposals, 6.4 Packaging & Release Stabilization. Exact slice boundaries advisory until each slice's own entry review. |
| C2  | Authority invariant | `/evolve` never grants authority, never mutates Host state silently, never claims stronger reproducibility than the provider permits. Proposals are bounded, redacted, Host-observed; acceptance requires explicit human/host gate. Lower-cost configurable layer preferred over Host code by construction.                                                                                                              |
| C3  | Posture             | Zero new `std::process`/`Command::new().spawn()` paths beyond typed `UNAVAILABLE`; corpus cap stays 384; no general Hooks, TaskGraph, Fleet, marketplaces, plugin dependency graphs, auto-acquisition, or GUI/TUI ownership (ADR 0036 §51). Fail-closed and core neutrality preserved.                                                                                                                                   |
| C4  | Evidence            | Each Stage 6 slice contributes a frozen differential subject over pure seams at successive corpus bumps `v49`→ within cap; audit `299/299 + new slices` at required parity with expectations coverage per decision 40 C7; unit- and session-proven invariants.                                                                                                                                                           |
| C5  | Corpus mechanics    | Schema stays 3; each slice bumps `CORPUS_VERSION` and strict-loader count asserts in `crates/siralos-cli/src/harness.rs` together with manifest version (all contract.mjs sites + protocol validator). Entry review itself bumps nothing.                                                                                                                                                                                |
| C6  | Lean guardrails     | No new ADR beyond 0036, no scope redraw, no Out-of-scope growth; `docs/wayfinder/siralos-roadmap.md: Out of scope` stays closed. One ticket per session; budget one coherent pass + up to two repairs.                                                                                                                                                                                                                   |

## HITL answers (pending)

- **Q1 Scope** — bounded `/evolve` with 6.1–6.4 as drafted vs narrower?
- **Q2 Escalation** — `Profile → Context → Skill → Plugin → Host` with deletion preference as frozen order?
- **Q3 Evidence** — differential subjects over pure seams + expectation coverage vs alternative harness?
- **Q4 Contract** — approve C1–C6 as drafted for implementation?

## Self-loop verification

- Criterion: draft contract is complete and internally consistent with ADR 0036 and the Verified Stage 5 map.
- Evidence: this document; ticket 58 with the C1–C6 draft.
- Verdict: **DRAFT** — awaits HITL grilling.

## Implementation record

_Draft — no implementation._
