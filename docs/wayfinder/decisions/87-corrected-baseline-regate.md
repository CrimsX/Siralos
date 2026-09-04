---
title: "The Corrected-Baseline Re-Gate"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 87 — The Corrected-Baseline Re-Gate

**Governing plan:** [68 — Real Provider Credentials and Registry](68-real-provider-credentials-and-registry.md) · **Entry review:** [79](79-context-management-foundations.md) · **Map:** [Siralos Roadmap](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** The corrected baseline was signed off externally before the run (the challenger of decisions 85/86 approved the amendments): DeepAll as the sole gated reference, SummariesAll and retired Identity as informational, strategy unchanged — one variable at a time. The claim this gate can make is deliberately weak and honest: selective structured retrieval dominates maximal source dumping at recall parity; the depth-premium ratio vs SummariesAll carries the non-trivial information.

## 2. Methodology — signed-off rule + amendments (verbatim pins)

**Signed-off rule (verbatim):** `GO = recall_parity && aggregate(paged*2 < DeepAll) && dedup_guard && all 9 cells pass. Only DeepAll gates. Pre-commit this sentence in the decision: "Paged is expected to beat DeepAll and lose to SummariesAll; neither informational outcome affects the verdict."`

**Pre-run caveat (verbatim):** `The strategy still carries the known Fact-2 structured-first over-pay headwind; a GO under DeepAll does not endorse the expansion policy.`

**Sequencing pins:** strategies are UNCHANGED (V1 and V2 flows byte-identical to decision 86's implementation); decomposition is REPORTED (re-based) so the over-pay is visible; any post-hoc strategy tweak is forbidden (decision 88 starts from a fresh pre-commit).

| #   | Amendment                               | Verbatim pin                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| --- | --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | DeepAll definition (frozen)             | `DeepAll = sum of estimate(deepest_available(node)) over ALL nodes, deepest ordering Source > Detailed > Structured > Summary > Identity (the existing deepest_level_for ordering); ZERO tool-call overhead on DeepAll (a dump, not a tool flow); same estimator (ceil(bytes/4), primary bpt=4/oh=4, 9-cell sweep {3,4,5}x{0,8,16}); NO dedup on reference dumps — DeepAll counts every node's bytes even across shared digests (the maximal dump; the asymmetry vs paged's surfaced-digest dedup is intentional and must be stated). Document the other asymmetry: paged's expansion priority (best_level_for: Structured > Detailed > Summary > Identity, Source EXCLUDED) is shallower than DeepAll's deepest (Source included) — paged wins partly by delivering structured depth instead of source depth; state it, do NOT fix it.` |
| 2   | SummariesAll (informational, non-gated) | `SummariesAll = sum of estimate(node L1 summary bytes) over all nodes (the same bytes the paged inspect step counts); zero overhead, no dedup; report the depth-premium ratio paged/SummariesAll (integer basis points); losing to SummariesAll is NOT a failure.`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| 3   | Identity retired but PRINTED            | `Identity retired but PRINTED: one diagnostic row (non-gated) per scenario and aggregate, so the audit trail shows the Fact-1 inversion.`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| 4   | Re-based formulas                       | `total_saved = DeepAll - actual (saturating); decomposition (dedup_saved, rerank_saved, level_saved) re-based to DeepAll with the sum identity test; dedup guard same form (dedup*2 <= total_saved); recall_baseline =                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | key | numerically unchanged (DeepAll surfaces every key node) — comment-only change.` |
| 5   | Mandatory per-scenario table            | `for each of the 6 gated scenarios: deepAll, summariesAll, identityDiag, pagedV1, pagedV2, recallV1, recallV2, toolCallsV1, toolCallsV2; plus aggregate rows. broad-foxtrot stays included; paraphrase-gap stays excluded with its informational record.`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| 6   | Sequencing pins                         | `the strategies are UNCHANGED (V1 and V2 flows byte-identical to decision 86's implementation); the decision record carries the pre-run caveat "The strategy still carries the known Fact-2 structured-first over-pay headwind; a GO under DeepAll does not endorse the expansion policy."; the decomposition is REPORTED (re-based) so the over-pay is visible; any post-hoc strategy tweak is forbidden (decision 88 starts from a fresh pre-commit).`                                                                                                                                                                                                                                                                                                                                                                                 |
| 7   | Deepest-availability audit              | `record per-scenario level counts (how many nodes hold Source/Detailed/Structured/Summary/Identity) so DeepAll is reproducible; include in the record as levelCensus.`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| 8   | Restated rules                          | `inspect double-count (paged counts inspect summary bytes + expansion bytes — a deliberate headwind vs DeepAll); dedup posture per flow (references none, paged surfaced-digest); tool-call counting on paged unchanged (1 + inspects + expands); zero overhead on all references.`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |

Additional guard rail: `V1's paged-flow token numbers MUST be byte-identical to the decision 86 run (the flow code is untouched; only reference accounting and report shape change) — asserted in a test against the recorded v3 aggregate (3402 V1, 3012 V2)`.

## 3. Criteria → evidence (measured)

### 3.1 Per-scenario table (6 gated scenarios + aggregate)

| Scenario      | deepAll  | summariesAll | identityDiag | pagedV1  | pagedV2  | recallV1  | recallV2  | toolCallsV1 | toolCallsV2 |
| ------------- | -------- | ------------ | ------------ | -------- | -------- | --------- | --------- | ----------- | ----------- |
| narrow-alpha  | 1148     | 316          | 325          | 488      | 379      | 2/2       | 2/2       | 7           | 6           |
| narrow-beta   | 1148     | 303          | 330          | 345      | 236      | 1/1       | 1/1       | 5           | 4           |
| narrow-gamma  | 1218     | 366          | 389          | 488      | 379      | 2/2       | 2/2       | 7           | 6           |
| narrow-delta  | 1491     | 400          | 447          | 350      | 350      | 1/1       | 1/1       | 5           | 5           |
| medium-echo   | 1456     | 431          | 448          | 800      | 800      | 3/3       | 3/3       | 9           | 9           |
| broad-foxtrot | 1148     | 357          | 347          | 931      | 868      | 5/5       | 5/5       | 11          | 11          |
| **Aggregate** | **7609** | **2173**     | **2286**     | **3402** | **3012** | **14/14** | **14/14** | **44**      | **41**      |

`paraphrase-gap` excluded from aggregate; informational record: `queryTokens ["commit","gating"]`, `keyOverlapCount 0`, `inHits false`.

Paged is expected to beat DeepAll and lose to SummariesAll; neither informational outcome affects the verdict.

- **DeepAll check:** `pagedV2 3012 *2 = 6024 < DeepAll 7609` ⇒ **pass** (reduction 60.4%).
- **SummariesAll (informational):** `pagedV2 3012 > SummariesAll 2173` ⇒ **lose to SummariesAll as expected**; depth-premium `13861 bps` (138.61%, paged 38.6% larger than summaries). Losing is NOT a failure.
- **Identity diagnostic (retired):** `pagedV2 3012 > Identity 2286` would still lose vs old baseline — printed for Fact-1 audit; non-gated.

### 3.2 Aggregates

| Metric                         | Value                                                                                                                                                                    |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `totalBaseline` (DeepAll)      | 7609                                                                                                                                                                     |
| `summariesAll` (informational) | 2173                                                                                                                                                                     |
| `identityDiag` (retired)       | 2286                                                                                                                                                                     |
| `totalPaged V1`                | 3402 (byte-identical to decision 86)                                                                                                                                     |
| `totalPaged V2`                | 3012 (byte-identical to decision 86)                                                                                                                                     |
| `totalRecall`                  | 14/14 (recall parity holds)                                                                                                                                              |
| `totalToolCalls V1/V2`         | 44 / 41                                                                                                                                                                  |
| `go`                           | `true` (recall parity && paged*2<DeepAll && dedupGuard && 9/9 cells)                                                                                                     |
| `reason`                       | `total_recall_paged 14 == total_recall_baseline 14 is true, total_paged 3012 *2 < total_baseline 7609 is true, dedup guard ok (0*2 <= 4597), sensitivity 9/9 pass => GO` |

### 3.3 Decomposition (aggregate, re-based to DeepAll, bpt=4/oh=4)

`dedupSaved 0`, `rerankSaved 327`, `levelSaved 4270`, `totalSaved 4597`, `dedupSharePct 0 bps`, `dedupGuardOk true` (`0*2 <= 4597`). Sum identity holds: `0 + 327 + 4270 == 4597 == DeepAll 7609 − paged 3012`.

Per-scenario sums identical via residual level; dedup is zero because realistic fixtures have only one duplicate pair and it is not among paged hits with dedup disabled vs enabled delta.

### 3.4 Depth-premium ratio vs SummariesAll

`depthPremiumBps = pagedV2 *10000 / summariesAll = 3012*10000/2173 = 13861 bps` (138.61%). Paged loses to SummariesAll as expected; the non-trivial information is that selective structured retrieval still dominates maximal dumping while paying a 38.6% depth premium over bare summaries.

### 3.5 9-cell estimator sweep (bytesPerToken × overhead, vs DeepAll)

| bpt | overhead | recallOk | marginOk | dedupOk |
| --- | -------- | -------- | -------- | ------- |
| 3   | 0        | true     | true     | true    |
| 3   | 8        | true     | true     | true    |
| 3   | 16       | true     | true     | true    |
| 4   | 0        | true     | true     | true    |
| 4   | 8        | true     | true     | true    |
| 4   | 16       | true     | true     | true    |
| 5   | 0        | true     | true     | true    |
| 5   | 8        | true     | true     | true    |
| 5   | 16       | true     | true     | true    |

Result: **9/9 cells pass** (`allOk true`). The corrected baseline passes margin under every estimator; the previous 0/9 failure under Identity is overturned — expected because DeepAll maximal dump is the honest challenger.

### 3.6 Level census (deepest-availability audit)

| Scenario      | Source | Detailed | Structured | Summary | Identity |
| ------------- | ------ | -------- | ---------- | ------- | -------- |
| narrow-alpha  | 1      | 1        | 3          | 1       | 6        |
| narrow-beta   | 1      | 1        | 3          | 1       | 6        |
| narrow-gamma  | 1      | 1        | 3          | 2       | 7        |
| narrow-delta  | 1      | 2        | 4          | 1       | 8        |
| medium-echo   | 1      | 2        | 3          | 2       | 8        |
| broad-foxtrot | 1      | 1        | 3          | 1       | 6        |

Counts are nodes that hold the level; DeepAll picks the deepest available per node (Source if present, else Detailed, etc.). Zero overhead on all references; paged inspect double-count is intentional headwind.

### 3.7 Informational

`paraphrase-gap`: `queryTokens ["commit","gating"]`, `keyOverlap 0`, `inHits false` — excluded, demonstrates lexical-only routing limit.

## 4. Result

Corrected-baseline re-gate measured **GO** (paged V2 3012 vs DeepAll 7609: recall 14/14, reduction 60.4%, dedup share 0%, 9/9 cells pass; depth premium vs SummariesAll 13861 bps). The weak-but-honest claim stands: selective retrieval dominates maximal dumping at recall parity; the escalation-policy question (Fact 2) moves to decision 88 with a fresh pre-commit.
