---
title: "Stage 5 Verified Roll-Up - Consolidating the Six Realized Slices into One Milestone-Verified State"
label: "wayfinder:ticket"
type: HITL
status: open
resolution: ""
blockedBy: []
---

# Ticket 57 — Stage 5 Verified Roll-Up

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Decision:** [Stage 5 Verified Roll-Up](../decisions/57-stage5-verified-roll-up.md) (opens after HITL grilling)

## Why now

Stage 5 — Composition (decisions 47–56) is fully consumed: 5.1 profiles
(`be030e3`), 5.2 profile composition (`4c562c8`), 5.3 context controls
(`ce3e7dc`), 5.4 siralos.lock resolution (`0a6d592`), 5.5 plugin selection
(`5e1b3e0`), 5.6 skills (`fcf61c5`), 5.7 session plugin activation gate
(`926ac71`), 5.8 session context controls (`6dc830e`), 5.9 session lock
verification (`6e38804`), and 5.10 session skill consumption (`579f1e9`).
Every frozen seam now has a session-boundary consumer; Stage 5 has
per-slice completion records but no milestone-level Verified state. This
ticket is pure closure — no new behavior, no corpus change, no capability
change.

## Contract draft (C1–C6)

| #   | Clause             | Draft                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| --- | ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| C1  | Scope              | Consolidate the ten realized slices (5.1 profiles; 5.2 profile composition; 5.3 context controls; 5.4 lock resolution; 5.5 plugin selection; 5.6 skills; 5.7 session plugin activation gate; 5.8 session context controls; 5.9 session lock verification; 5.10 session skill consumption) into one milestone-Verified state named from repository evidence at current HEAD.                                                                                                                                        |
| C2  | Evidence base      | Verification criteria each bound to fresh or recorded gate evidence: sequence complete (decisions 47–56 annotated); differential audit 299/299 applicable required at v48/304 with 65 expectation records and the pinned v32 oracle untouched; fail-closed posture (nothing flips `unavailable`, zero spawn paths swept across the Stage 5 modules); core domain neutrality (`check:rust`); lean guardrails per ADR 0036 (no taxonomy growth, corpus cap 384 unchanged); docs atomic (map, AGENTS.md, decisions 47–56 annotated).                                                        |
| C3  | No behavior change | The roll-up flips nothing: no code, corpus, fixture, or expectation edits; gates re-run only as verification evidence.                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| C4  | Record shape       | Decision 57 carries the HITL answers, the C1–C6 table with per-criterion evidence status, and the single "Stage 5 is Verified at <sha>" marker; map and AGENTS.md Current carry the same marker atomically.                                                                                                                                                                                                                                                                                                                                                                    |
| C5  | Lean guardrails    | No new ADR, no scope redraw, no Out-of-scope changes; the next frontier is named from the map's fog-free state (all Not-yet-specified items decided).                                                                                                                                                                                                                                                                                                                                                                                                                           |
| C6  | Budget             | One coherent pass + up to two repairs; any failed gate is a repair with evidence, never a claim.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |

## HITL grilling (pending)

- **Q1 Scope** — full roll-up as drafted vs. narrower evidence base?
- **Q2 Criteria** — the six-verification-criterion ledger from decision 46, adapted to Stage 5?
- **Q3 Record shape** — single marker + atomic docs as drafted?
- **Q4 Contract** — approve C1–C6 as drafted?