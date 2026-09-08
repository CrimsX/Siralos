---
title: The Paraphrase-Gap Fixture Amendment
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# The Paraphrase-Gap Fixture Amendment

Governing plan 68 · entry review [79](79-context-management-foundations.md) · Map.

> **User-directed 2026-08-31 (session HITL).** The decision 115 experiment was unpowered — the fixture corpus had no hit-to-key edges AND the paraphrase scenario had zero hits, so the neighbor mechanism had nothing to traverse. This amendment plants the minimal mechanism test: one genuine referrer node (a real 2-token match) linked to the key, stated as planted. The gate scenarios are byte-unchanged and the measurement is now live.

## 2. Design (F1–F6) — as implemented

| Area                 | Value                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F1 The Referrer      | The `paraphrase-gap` scenario gains one new node `pg-referrer` (source, summary 255 chars) whose summary contains both query tokens `commit` and `gating` naturally — “Commit gating for Siralos: the commit gating gate validates the phase-contract before any workspace mutation lands, enforcing host approval when the lock is stale.” A genuine 2-token lexical match that passes search and the rerank threshold of 2. L0 identity + L1 summary only; no deeper levels. Plus one edge `pg-referrer -> pg-01` (`ContextEdgeKind::References`, consistent with sibling edges). |
| F2 The Plant Stated  | This edge and node are an artificial mechanism test, stated as planted in the decision record and in a code comment at the `paraphrase-gap` construction — testing whether the neighbor mechanism traverses graph locality, not a claim that natural corpora have such edges (per the decision 86 lesson that fixture design is the experiment).                                                                                                                                                                                                                                    |
| F3 Gate Unchanged    | The six gate scenarios are byte-unchanged. The gate aggregate re-measured byte-identical to the decision 115 record: recall 14/14, paged V2 3170 vs DeepAll 7609 (58.3% over the bar), dedup share 0%, sensitivity 8/9 (bpt=5/oh=16 fails), GO false, V1 3560, V3 13/14.                                                                                                                                                                                                                                                                                                            |
| F4 The Measurement   | The paraphrase scenario’s informational record re-measures: `neighborReachable` TRUE — the referrer is a passing hit, the key is its 1-hop neighbor, the candidate is admitted and inspected; `neighborCost` 65 candidate tokens (summary-only inspect of `pg-01`); toolCalls 4 for the paraphrase scenario (1 search + 1 hit inspect + 1 candidate inspect + 1 expand, delta +1 candidate inspect vs the unplanted baseline).                                                                                                                                                      |
| F5 The 115 E-Rules   | The decision 115 E-rules stand unchanged: decay (referrer score 21 halved to 10), cap 8 (only 1 candidate admitted), surfaced-exclusion (candidate not in hits_set), summary-only candidate inspects (no deeper expansion, no escalation).                                                                                                                                                                                                                                                                                                                                          |
| F6 Runtime Untouched | Runtime untouched — scheduler, tool runtime, and assembly unchanged. Corpus re-pinned at v79/357 (differential `context-benchmark` record re-pinned in place; `CORPUS_VERSION` 79, manifest and digests refreshed, `strict_loader` 357).                                                                                                                                                                                                                                                                                                                                            |

## 3. Criteria → Evidence

_Measured after run (corpus v79/357, audit 352/352 applicable required, 118 expectation records, pinned v32 oracle untouched)_

| Criterion                             | Evidence                                                                                                                                                                                                                      | Verdict  |
| ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| The referrer is a genuine 2-token hit | `pg-referrer` summary contains `commit` and `gating` as whole-word tokens; search hit `matched_in: summary` score 21; rerank overlap 2 ≥ threshold 2, expanded.                                                               | pass     |
| The planted edge is traversed         | Edge `pg-referrer -> pg-01` present; `neighbor_candidates_for_scenario` on the paraphrase scenario generated 1, admitted 1 (`pg-01`), decayed score 10; `neighborReachable` true.                                             | pass     |
| Gate byte-identity proof              | Gate-only `run_benchmark` → V2 3170 vs baseline 7609, recall 14/14, V1 3560, V3 13/14, sensitivity 8/9, GO false — byte-identical to the decision 115 record (asserted in `d116_gate_byte_identical_to_decision_115_record`). | pass     |
| Reachability outcome & cost           | `informational.paraphraseGap.neighborReachable` true, `neighborCost` 65 (estimate_tokens of `pg-01` summary length 260), `per-scenario` paraphrase `toolCalls` 4.                                                             | measured |
| E-rules (decay, cap, exclusion)       | Decayed score 10 (21/2), cap 8 not triggered (1 < 8), hits_set exclusion respected, candidate inspected as summary only (no structured/detailed/source).                                                                      | pass     |
| Runtime untouched & corpus re-pinned  | No scheduler/tool/assembly change; `context-benchmark` record re-pinned via `canonicalRecordDocument` (surgical `neighborReachable` false→true, `neighborCost` 0→65); corpus v79/357.                                         | pass     |

## 4. Result

With the planted referrer edge, the neighbor mechanism reached the paraphrase key through graph locality at 65 candidate tokens (+1 candidate inspect, toolCalls 4); the gate aggregate re-measured byte-identical (recall 14/14, paged V2 3170 vs DeepAll 7609, 8/9 cells, GO false, V1 3560, V3 13/14); the clause-(d) boundary is narrowed where graph locality exists, and the runtime remains unchanged.
