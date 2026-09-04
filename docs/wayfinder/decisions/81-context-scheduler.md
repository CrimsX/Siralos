---
title: "The Deterministic Tiered Context Scheduler (Slice 3)"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 81 — The Deterministic Tiered Context Scheduler (Slice 3)

**Governing plan:** [68 — Real Provider Credentials and Registry](68-real-provider-credentials-and-registry.md) · **Entry review:** [79](79-context-management-foundations.md) · **Map:** [Siralos Roadmap](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** Slice 3 of decision 79's staged plan: the HOT/WARM/COLD working set under deterministic synchronous ticks with an explicit-pin override and a deterministic constant budget. Demotion reorders; it never deletes. Prefetch, compression, and learned ranking remain post-benchmark.

## 2. Deterministic rules

| Aspect          | Rule                                                                                                                                                                                                                              |
| --------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| State           | `WorkingSetState { entries: Vec<SchedulerEntry> sorted by node_id, tick: u64 }` — `SchedulerEntry { node_id, tier, pinned, relevance 0..=100, last_access_tick, token_estimate }`, tier ∈ {Hot, Warm, Cold, Archive}              |
| Events          | `Access { node_id }`, `Pin { node_id }`, `Unpin { node_id }`, `Relevance { node_id, relevance }`, `Stale { node_id }`, `Archive { node_id }` — all synchronous, in-turn, no threads or async                                      |
| Tick            | `score = relevance*4 + recency_points + pin_bonus` where `recency_points = 30 if ticks_since==0 else 30 - min(ticks_since,30)` and `pin_bonus = 50 if pinned`; Hot if `score>=280`, Warm if `score>=120`, else Cold               |
| Tick re-tiering | Increment `tick` by 1; for each non-Archive entry: `pinned → Hot`, else tier by score; Archive never re-tiers automatically; report `{ tick, promoted: Vec<node_id>, demoted: Vec<node_id> }` in canonical node_id order          |
| Budget rule (f) | `DEFAULT_BUDGET_TOKENS=4096` constant; `enforce_budget` sums Hot tokens from caller estimates and while `hot_total>budget` demotes lowest-scored Hot non-pinned entry to Warm (tie: score asc, node_id asc); pinned never demoted |
| Stale interplay | `Stale` demotes one tier toward Cold (`Hot→Warm`, `Warm→Cold`, `Cold` stays `Cold`) regardless of `pinned` — content-staleness is a fact and outranks preference; documented precedence is deterministic                          |
| Archive         | `Archive` is explicit-only (`Archive { node_id }`); no tick, budget, or staleness path ever produces `Archive`; demotion never deletes authoritative information                                                                  |

## 3. Criteria → evidence

| Criterion                                                    | Evidence                                                                                                                                                                                                                                                                  | Status |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Scheduler construction, scoring, tick, events, stale, budget | 16 core tests (canonical order, duplicate refusal, pin forces Hot, access recency, relevance range, tick promotion/demotion, stale beats pin, archive explicit-only, budget lowest-score tie-break, pinned never demoted, boundary, ordering, config, score)              | pass   |
| Clause (a) deterministic synchronous ticks preserved         | No threads, locks, or async runtime; `tick()` and `apply_event()` are synchronous on in-turn events; ordering is canonical by node_id; score is integer deterministic                                                                                                     | pass   |
| Clause (f) budget is deterministic constant with demotion    | `DEFAULT_BUDGET_TOKENS=4096`; `enforce_budget` demotes lowest-scored Hot non-pinned first, tie-break node_id asc, pinned never demoted; hot_total before/after counted; compaction is not the architecture                                                                | pass   |
| The contract is pinned in the differential audit             | `context-scheduler` subject at corpus v59/327 files; audit 322/322 applicable required, 4 explicit platform skips, 0 accepted informational deviations; expectations 88 records via canonicalRecordDocument (surgical 87→88 diff); pinned v32 oracle untouched            | pass   |
| Pinned oracle untouched and gates green                      | pinned v32 oracle still 234/234; `cargo fmt --all --check` exit 0; `cargo clippy --workspace --all-targets --all-features -- -D warnings` exit 0; `cargo test --workspace --all-targets --all-features` exit 0 (core 556, adapters 170, conformance 25, cli 71, 0 failed) | pass   |

## 4. Result

Slice 3 — the tiered scheduler — is complete and pinned at corpus v59. Slice 4 (the context tools) is pending.
