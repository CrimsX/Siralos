# Decision — Stage 4 Verified Roll-Up — One Milestone-Verified State for the Seven Realized Slices

**Wayfinder ticket:** [Stage 4 Verified Roll-Up](../tickets/46-stage4-verified-roll-up.md) · label `wayfinder:ticket` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [Stage 4.7 Profiling Entry Review](45-stage4-7-profiling-entry-review.md) (complete at `b206a4a`; the frozen sequence fully consumed)
**Decided:** 2026-08-28 (resolver session; HITL grilling over ticket 46's C1–C6 draft and the 4 open roll-up questions)
**Status:** **PASS — Stage 4 Verified roll-up contract frozen; evidence gathering authorized**
**Self-loop ledger:** 4 criteria, one decision pass (verification below)

> Pure closure record: no behavior change, no corpus change, no capability change (C3). Mirrors the R7/R8/R9/R10 milestone roll-ups: each closed with a single "Verified at <sha>" marker named from repository evidence.

---

## Summary

HITL confirmed rolling the seven realized Stage-4 steps into one milestone-Verified state: 4.1 `process.execute` fail-closed (`168a769`/`05c075c`); steps 1–3 of the frozen sequence (the `runtime-execution` + `runtime-evidence` subjects landed at the v32 reconciliation, 4.2 folded into 4.1 per decision 41); 4.3 Godot runtime adapter (`5bedf57`); 4.4 visual evidence (`4a250d8`); 4.5 controlled interaction (`42ee5ab`); 4.6 QA workflows (`a83c2a4`); 4.7 profiling (`b206a4a`). The evidence base is fresh gate runs plus the recorded decisions; the roll-up flips nothing.

## 1. HITL answers (2026-08-28)

| #   | Roll-up question  | Human answer                         |
| --- | ----------------- | ------------------------------------ |
| 1   | Scope             | **Seven realized steps as drafted**  |
| 2   | Evidence base     | **Six criteria, fresh gate runs**    |
| 3   | Posture           | **Confirm** C3 — pure closure record |
| 4   | Contract approval | **Approve C1–C6 as drafted**         |

## 2. Frozen contract (C1–C6, confirmed)

| #   | Clause             | Contract                                                                                                                                                                   |
| --- | ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Scope              | Consolidate the seven realized steps listed above into one milestone-Verified state named from repository evidence at current HEAD.                                        |
| C2  | Evidence base      | Six verification criteria, each bound to fresh or recorded gate evidence (see §3).                                                                                         |
| C3  | No behavior change | The roll-up flips nothing: no code, corpus, fixture, or expectation edits; gates re-run only as verification evidence.                                                     |
| C4  | Record shape       | This decision carries the HITL answers, the C1–C6 table, and the single "Stage 4 is Verified at <sha>" marker; map and AGENTS.md Current carry the same marker atomically. |
| C5  | Lean guardrails    | No new ADR, no scope redraw, no Out-of-scope changes; the next frontier remains the map's Not-yet-specified section.                                                       |
| C6  | Budget             | One coherent pass + up to two repairs; any failed gate is a repair with evidence, never a claim.                                                                           |

## 3. Verification ledger (self-loop verification protocol)

| Criterion (C2)                                                                                                                                              | Direct evidence                                                                                                                                                                                          | Status |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Sequence complete — all seven steps implemented, entry-reviewed, evidence-backed                                                                            | decisions 41–45 frozen and annotated; v32 reconciliation recorded in the map and AGENTS.md Current (steps 1–3)                                                                                           | pass   |
| Differential parity at v38 — audit 259/259 applicable required, 4 skips, 1 accepted informational; expectations 25 records; pinned v32 oracle untouched     | fresh `npm run check` run (differential gate included) at the freeze commit; expectations file digest-bound, pinned oracle directory unchanged                                                           | pass   |
| Fail-closed posture — nothing flips `unavailable`; zero spawn paths                                                                                         | grep sweep over `crates/siralos-core/src/runtime` and `crates/siralos-godot/src`: zero `std::process`/`Command::new`/`spawn` code paths; every Stage-4 decision (41–45) explicitly preserves the posture | pass   |
| Core domain neutrality — `check:rust` green                                                                                                                 | fresh `npm run check` run (architecture check + fmt + clippy + tests included)                                                                                                                           | pass   |
| Lean guardrails per ADR 0036 — no taxonomy growth, no GUI/TUI ownership, no approval-authority change; corpus cap raise 256 → 384 sanctioned by decision 44 | C1–C6 of decisions 42–45 each carry the guardrail clause; no new limits beyond the sanctioned cap raise                                                                                                  | pass   |
| Docs atomic — map, AGENTS.md, decisions 41–45 annotated with Implemented-at shas                                                                            | record-complete commits `d673d22`, `ac1266b`, `8002e69`; decision index lines 41–45 in the map                                                                                                           | pass   |

**Stage 4 is Verified** — the marker and its sha are recorded in the map and AGENTS.md Current at the record-complete commit.
