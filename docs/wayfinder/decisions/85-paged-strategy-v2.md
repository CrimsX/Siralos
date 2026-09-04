---
title: "Paged Strategy V2 — Progressive Disclosure Re-Benchmark"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 85 — Paged Strategy V2 — Progressive Disclosure Re-Benchmark

**Governing plan:** [68 — Real Provider Credentials and Registry](68-real-provider-credentials-and-registry.md) · **Entry review:** [79](79-context-management-foundations.md) · **Map:** [Siralos Roadmap](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** Re-benchmark with a smarter deterministic paged strategy: lexical rerank, digest dedup, progressive disclosure. The 50% bar, recall rule, and gold fixtures are re-committed unchanged before the run — only the strategy varies. The verdict is measured.

## 2. Methodology — v2 rules

| #   | Rule                    | Verbatim                                                                                                                                                                                                                                                                                   |
| --- | ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1   | Lexical rerank          | for each search hit, overlap = count of DISTINCT query tokens present in the summary's token set (summary text from the node's summary representation). expand_threshold = min(2, distinct query token count). Expand the hit IFF matched_in == "summary" AND overlap >= expand_threshold. |
| 2   | Fallback                | if NO hit passes the threshold, expand exactly ONE hit — the summary-matched hit with the highest overlap; tie-break node_id ascending. (Prevents recall collapse on degenerate queries.)                                                                                                  |
| 3   | Digest dedup            | maintain a surfaced-digest set across the whole flow; a summary or expansion whose content digest is already surfaced contributes zero additional tokens (the content is not re-surfaced). Summaries surfaced by inspect seed the set.                                                     |
| 4   | Deep-expansion priority | unchanged from v1 (structured > detailed > summary > identity); a summary-level expansion of an already-inspected node naturally costs zero via rule 3.                                                                                                                                    |

Tokenizer: `fn tokenize(text: &str) -> Vec<String>` — lowercase, split on non-alphanumeric, DROP tokens shorter than 3 chars, dedup preserving first-occurrence order.

The 50% bar, the recall rule (`total_recall_paged == total_recall_baseline`), and the gold fixtures are UNCHANGED and re-committed before the run — only the strategy is the variable. `TOOL_CALL_OVERHEAD_TOKENS`, `tool_calls`, the baseline strategy, and the recall definition are ALL UNCHANGED from v1.

## 3. Criteria → evidence

| Criterion                                  | Evidence                                                                                                                                                                                                                                                                                                                                                                                                                                          | Status |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Gold fixtures unchanged (6 scenarios)      | `gold_set()` — narrow-alpha (2/6, key 2), narrow-beta (2/6, key 1), narrow-gamma (2/7, key 2), narrow-delta (2/8, key 1), medium-echo (3/8, key 3), broad-foxtrot (5/6, key 5); v1 aggregates 1230 baseline vs 650 paged (47.2% reduction) recall 14/14 tool_calls 38; v2 aggregates 1230 baseline vs 510 paged (58.5% reduction) recall 14/14 tool_calls 38 — broad-foxtrot still loses on v1 (206 vs 180) and v2 reduces via dedup (144 vs 180) | pass   |
| V1 byte-identical                          | `run_strategy(..., ExhaustiveV1)` 14/14 recall, 650 paged, per-scenario narrow-alpha 74, narrow-beta 86, narrow-gamma 74, narrow-delta 96, medium-echo 114, broad-foxtrot 206 — byte-identical to pre-v2 module                                                                                                                                                                                                                                   | pass   |
| V2 preserves recall on gold_set            | `run_strategy(..., ProgressiveV2)` 14/14 recall (total_recall_paged 14 == total_recall_baseline 14) — honesty pin holds, fixtures not weakened                                                                                                                                                                                                                                                                                                    | pass   |
| V2 lexical rerank + fallback deterministic | tokenizer + threshold + fallback + digest dedup + key-blindness covered by 7 new tests (tokenizer, rerank threshold, single-token, fallback, dedup, key-blindness, no-mutation)                                                                                                                                                                                                                                                                   | pass   |
| Decision rule unchanged (V2)               | `go = total_recall_paged == total_recall_baseline && total_paged *2 < total_baseline` — measured on V2: 14==14 true, 510*2=1020 <1230 true => GO                                                                                                                                                                                                                                                                                                  | pass   |
| Contract pinned at v62                     | `context-benchmark` subject at corpus v62/329 files; audit 324/324 applicable required, 4 skips, 0 informational, pinned v32 oracle untouched; `go true` + `reason` citing `total_recall_paged 14 == total_recall_baseline 14 is true, total_paged 510 *2 < total_baseline 1230 is true => GO`                                                                                                                                                    | pass   |

Per-scenario v1 → v2 deltas (baseline unchanged):

| Scenario      | Baseline | Paged v1        | Paged v2        | Δ paged  | Recall    | Tool calls |
| ------------- | -------- | --------------- | --------------- | -------- | --------- | ---------- |
| broad-foxtrot | 180      | 206             | 144             | −62      | 5/5       | 11         |
| medium-echo   | 240      | 114             | 98              | −16      | 3/3       | 7          |
| narrow-alpha  | 180      | 74              | 66              | −8       | 2/2       | 5          |
| narrow-beta   | 180      | 86              | 78              | −8       | 1/1       | 5          |
| narrow-delta  | 240      | 96              | 58              | −38      | 1/1       | 5          |
| narrow-gamma  | 210      | 74              | 66              | −8       | 2/2       | 5          |
| **Total**     | **1230** | **650 (47.2%)** | **510 (58.5%)** | **−140** | **14/14** | **38**     |

## 4. Result

Strategy v2 measured GO (1230 baseline vs 510 paged: recall 14/14, reduction 58.5% vs the 50% bar). The decision 83 NO-GO is superseded by measured evidence; the audit and hygiene-metrics seam is un-gated as a candidate next slice. Savings composition on these fixtures is dedup-dominated (−140 total: broad-foxtrot −62, narrow-delta −38); the lexical-rerank threshold was 1 for every scenario (single-token queries), so rerank was neutral here — its discrimination is covered by unit tests and becomes active on multi-token queries.
