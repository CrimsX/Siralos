---
title: The Neighbor-Expansion Experiment
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: 68
supersedes: []
---

# The Neighbor-Expansion Experiment

Governing plan 68 · entry review [79](79-context-management-foundations.md) · Map.

> **User-directed 2026-08-31 (session HITL).** The externally-endorsed paraphrase-gap experiment, pre-committed: 1-hop neighbors of matched nodes become decayed-score inspect candidates (never deeper, never escalated, capped at 8), the gate rules are unchanged (the decision 94 sweep stays gated), and the measurement targets one question — does graph locality narrow the clause-(d) boundary, and at what cost? The measured outcome is a null result: the fixture corpus contains no graph edges from hits toward the paraphrase key, so the mechanism admitted zero candidates and the boundary remains unmeasured.

## 2. Design (E1–E6) — as implemented

| Area                 | Value                                                                                                                                                                                                                                                                                                                              |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| E1 Candidates        | ProgressiveV2 only: the union of 1-hop graph neighbors of all rerank-passing hits, excluding surfaced nodes; each inherits its referrer hit's decision 92 score HALVED; ordered decayed-score desc, node_id asc; capped at 8 total per scenario; candidates are INSPECTED (summary) but never expanded deeper and never escalated. |
| E2 Unchanged rules   | The decision 87 GO rule (recall parity, paged*2 < DeepAll, dedup guard, 9-cell sweep) and the decision 88 depth-aware recall + adoption comparison re-run unchanged; V1 and V3 untouched with invariance guards (V1 paged 3560, V3 13/14).                                                                                         |
| E3 Paraphrase record | Excluded from the aggregate; its informational record gains neighborReachable and neighborCost.                                                                                                                                                                                                                                    |
| E4 Determinism       | Candidates are pure functions of the graph + hits + scores; run-twice byte-equal.                                                                                                                                                                                                                                                  |
| E5 Runtime untouched | Benchmark strategy only; no scheduler/tool/assembly change.                                                                                                                                                                                                                                                                        |
| E6 Tool calls        | Candidate inspects count like other inspects (1 + inspects(hits + candidates) + expands(passing hits)).                                                                                                                                                                                                                            |

## 3. Criteria → Evidence

_Measured after run (corpus v78/357, audit 352/352 applicable required, expectations 118 records, pinned v32 oracle untouched)_

| Criterion                               | Evidence                                                                                                                                                                                                              | Verdict  |
| --------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| The mechanism ran and was deterministic | NeighborCandidateMetrics recorded per scenario; the full record re-pinned at v78                                                                                                                                      | pass     |
| Candidates generated/admitted           | **ZERO across all six scenarios (generated 0, admitted 0, tokens 0 everywhere)** — the fixture corpus contains no graph edges from hits toward the paraphrase key, so the candidate set was empty by construction     | measured |
| Paraphrase reachability                 | neighborReachable false, neighborCost 0 — NOT because graph locality failed to correlate, but because there were no candidate edges to traverse; the boundary remains UNMEASURED on this corpus                       | measured |
| Gate re-measured under unchanged rules  | All aggregates byte-identical to the decision 92 record: recall 14/14, paged V2 3170 vs DeepAll 7609 (58.3% over the bar), dedup share 0%, sensitivity 8/9 (bpt=5/oh=16 fails), GO false — the NO-GO stands unchanged | measured |
| V1/V3 invariance guards                 | V1 paged 3560 byte-identical; V3 depth-aware 13/14 with the Retained outcome intact (policyAdopted false)                                                                                                             | pass     |

## 4. Result

The experiment measured a NULL result: the fixture corpus contains no graph edges from hits toward the paraphrase key, so the candidate mechanism admitted zero candidates and the clause-(d) boundary remains unmeasured — not measured-failed, unpowered. The gate re-measured NO-GO byte-identical to the decision 92 record (recall 14/14, paged 3170 vs DeepAll 7609, 8/9 sensitivity cells). The honest next step, if this boundary is to be measured at all, is a pre-committed fixture amendment planting one hit→key edge (stated as a mechanism test, per the decision 86 lesson that fixture design is the experiment). The runtime remains unchanged.
