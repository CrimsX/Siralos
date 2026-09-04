---
title: "The Escalation-Policy Test"
label: "wayfinder:decision"
status: "accepted"
date: "2026-08-31"
ticket: "68"
supersedes: []
---

# 88 — The Escalation-Policy Test

Governing plan 68 · entry review [79](79-context-management-foundations.md) · Map.

> **User-directed 2026-08-31 (session HITL).** The expansion-policy question left open by the corrected-baseline GO is tested with a fresh pre-commit, externally signed off before the run: answer-aware scoring (the depth-blind metric cannot measure escalation), key-blind policy, top-1 gate with an informational top-k curve, and pre-committed adoption branches.

## 2. Pre-commit (externally signed off)

The expansion-policy question left open by the corrected-baseline GO is tested with a fresh pre-commit, externally signed off before the run: answer-aware scoring (the depth-blind metric cannot measure escalation), key-blind policy, top-1 gate with an informational top-k curve, and pre-committed adoption branches.

### Amendments

| Area                    | Value                                                                                                                    |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| Level ordering          | Identity < Summary < Structured < Detailed < Source                                                                      |
| Answer level            | Summary \| Structured only (Source banned, Detailed banned unless no Structured)                                         |
| Gold substring          | At answer_level and deeper, not cheaper                                                                                  |
| Summary                 | GRAPH summary                                                                                                            |
| Availability invariant  | Gold at Summary requires graph node; at Structured requires store Structured and reachable via best_level_for            |
| Census floor            | ≥5/14 Structured → UNPOWERED if fails                                                                                    |
| Key-blindness           | Answer map scorer-side only, V3 takes no answer arg                                                                      |
| Depth-aware recall      | surfaced = max(Summary, expanded); dedup-zero counts as surfaced                                                         |
| Dedup-zero              | Counts as surfaced                                                                                                       |
| ProgressiveV3Escalating | Top-1 subset of V2                                                                                                       |
| Gate                    | Adopt iff depth-aware(V3) == depth-aware(V2) && cost(V3) < cost(V2) strict; void if V3 > V2                              |
| Cost machinery          | Frozen from decision 87 (bpt=4/oh=4, DeepAll sole baseline, 9-cell sweep, 50% bar, dedup guard, 3402/3012 byte-identity) |

## 3. Criteria → Evidence

_Measured after run (corpus v65, 329 scenarios, 90 expectation records, 324/324 audit)_

- Census (pre-committed vs as-run, floor): pre 5 Structured / 8 Summary (total 13 + 1 Identity =14), as-run 5/8, floorOk true (≥5/14)
- Deviation note: the pre-committed bins were Summary | Structured, but key `bf-05` (the second half of the one intentional duplicate pair) is scored at Identity with the shared gold — its content is byte-identical to `bf-01`'s by v3 fixture design, so any deeper gold in the shared bytes would violate exclusivity for `bf-01`. Impact: the Identity key is recall-trivial for every strategy; the census floor (>= 5/14 Structured) is evaluated on the remaining keys and holds.
- Per-scenario depth-aware recall V2 vs V3 + cost + tool calls: V2 14/14 at 3012 tokens / 41 calls; V3 13/14 at 1844 tokens / 31 calls (broad-foxtrot 868→381, medium-echo 800→376, narrow-alpha 379→305, narrow-beta 236→236, narrow-delta 350→241, narrow-gamma 379→305)
- Per-level recall split: Summary 8/8 for both V2 and V3; Structured 5/5 for V2, 4/5 for V3 (one structured loss)
- Escalation curve: k=1 1844 cost / 13 recall / 31 calls; k=2 2249 /13 /36; k=3 2731 /13 /38 (curve informational, gate is top-1)
- Cost columns under 9-cell sweep (bpt {3,4,5} × oh {0,8,16} vs DeepAll 7609): all 9 cells pass recallOk, marginOk, dedupOk; GO true (V2 3012 < 7609*0.5)
- V1/V2 byte-identity: V1 3402, V2 3012 (frozen)

## 4. Result

Retained: escalation loses depth-aware recall (V3 13 vs V2 14 cost V3 1844 vs V2 3012); V2's expansion policy stands; the tradeoff is recorded as the measured cost of depth.

PolicyAdopted false. The recommended flow remains ProgressiveV2; runtime semantics remain unchanged until the scheduler-semantics slice re-pins them.
