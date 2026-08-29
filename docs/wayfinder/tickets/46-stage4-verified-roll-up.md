---
title: "Stage 4 Verified Roll-Up - Consolidating the Seven Realized Slices into One Milestone-Verified State"
label: "wayfinder:ticket"
type: HITL
status: closed
resolution: "PASS — Stage 4 Verified roll-up contract frozen (HITL 2026-08-28: Q1 seven realized steps as drafted; Q2 six criteria with fresh gate runs; Q3 confirm pure closure record; Q4 approve C1–C6). See decision 46."
blockedBy: []
---

# Ticket 46 — Stage 4 Verified Roll-Up

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Decision:** [Stage 4 Verified Roll-Up](../decisions/46-stage4-verified-roll-up.md) (opens after HITL grilling)

## Why now

The frozen 7-step Stage-4 sequence (decision 08) is fully consumed: every arrow is
implemented, entry-reviewed, and evidence-backed. R7/R8/R9/R10 each closed with a
milestone-Verified roll-up; Stage 4 has per-slice completion records but no
milestone-level Verified state. This ticket is pure closure — no new behavior, no
corpus change, no capability change.

## Contract draft (C1–C6)

| #   | Clause             | Draft                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| --- | ------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Scope              | Consolidate the seven realized steps (4.1 `process.execute` fail-closed; steps 1–3 runtime-execution/runtime-evidence subjects at v32; 4.3 Godot runtime adapter; 4.4 visual evidence; 4.5 interaction; 4.6 QA workflows; 4.7 profiling) into one milestone-Verified state named from repository evidence at current HEAD.                                                                                                                                                                                                                      |
| C2  | Evidence base      | Six verification criteria each bound to fresh or recorded gate evidence: sequence complete (decisions 41–45 + v32 reconciliation); differential audit 259/259 applicable required at v38/264 with 25 expectation records and the pinned v32 oracle untouched; fail-closed posture (nothing flips `unavailable`, zero spawn paths swept); core domain neutrality (`check:rust`); lean guardrails per ADR 0036 (no taxonomy growth, cap raise to 384 already sanctioned by decision 44); docs atomic (map, AGENTS.md, decisions 41–45 annotated). |
| C3  | No behavior change | The roll-up flips nothing: no code, corpus, fixture, or expectation edits; gates re-run only as verification evidence.                                                                                                                                                                                                                                                                                                                                                                                                                          |
| C4  | Record shape       | Decision 46 carries the HITL answers, the C1–C6 table with per-criterion evidence status, and the single "Stage 4 is Verified at <sha>" marker; map and AGENTS.md Current carry the same marker atomically.                                                                                                                                                                                                                                                                                                                                     |
| C5  | Lean guardrails    | No new ADR, no scope redraw, no Out-of-scope changes; the next frontier remains the map's Not-yet-specified section.                                                                                                                                                                                                                                                                                                                                                                                                                            |
| C6  | Budget             | One coherent pass + up to two repairs; any failed gate is a repair with evidence, never a claim.                                                                                                                                                                                                                                                                                                                                                                                                                                                |

## Open questions for HITL grilling

1. **Q1 Scope** — roll up exactly the seven realized steps as drafted, or a different slice list?
2. **Q2 Evidence base** — C2's six criteria with fresh gate runs as the evidence base?
3. **Q3 Posture** — confirm no behavior change (C3): pure closure record?
4. **Q4 Approval** — approve C1–C6 as drafted?
