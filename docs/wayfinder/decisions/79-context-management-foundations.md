---
title: "Context Management Foundations — the Context Graph (Entry Review + Slice 1)"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 79 — Context Management Foundations — the Context Graph (Entry Review + Slice 1)

**Governing plan:** [68 — Real Provider Credentials and Registry](68-real-provider-credentials-and-registry.md) · **Map:** [Siralos Roadmap](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** Adopts the verified context-management handoff's thesis — maximize useful context within capacity, not minimize tokens; memory management over compaction — with the frozen interpretation table below. Implementation is staged with a benchmark decision gate after the demand-paging slice; learned routing and prefetch remain post-evidence only.

## 1. Frozen interpretation table (each clause binds implementation)

- a) Deterministic first: the scheduler (future slice) is synchronous deterministic ticks on in-turn events; no threads, locks, or async runtime.
- b) L2 structured representations are deterministic host extraction; L1 prose summaries are model-derived, provenance-bound, optional, never authoritative.
- c) No new persisted store in v1: the graph is reconstructable from existing sources; only future pins would persist (additive [profile.context] key, decision 54 pattern).
- d) Routing Level 1 is lexical/structural only; embeddings and vector databases stay out (map out-of-scope).
- e) Conversation history is NOT a context source in v1; conversation-derived knowledge enters only via the existing knowledge-proposal path.
- f) The working-context budget is a deterministic constant with deterministic demotion on overflow (compaction is a last-resort ordered step, never the architecture).
- g) Benchmark decision gate: after the demand-paging slice, a minimal gold-set benchmark (baseline A vs graph systems) decides whether audit/metrics/prefetch machinery is built; success criteria per the handoff's section 37.

## 2. Slice 1 scope

ContextNode/ContextEdge/ContextGraph in siralos-core with validation, canonical ordering, digest over the artifact-digest primitive, and targeted staleness over node source bindings; pinned by the hermetic `context-graph` subject.

## 3. Criteria → evidence

| Criterion                                                      | Evidence                                                                                                                                                                                                                                                              | Status |
| -------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Graph construction, validation, digest, and targeted staleness | 5 new core tests (canonical order, duplicate/dangling/malformed refusals, bounds, token heuristic, staleness targeting incl. absent-not-stale); cargo test --workspace --all-targets --all-features exit 0 (core 529, adapters 170, conformance 25, cli 71, 0 failed) | pass   |
| Existing seams are reused, not duplicated                      | digest over digest_artifact_payload("ContextGraph", 1); staleness semantics mirror derive_artifact_staleness with a documented deviation: an absent binding is not stale — absent evidence never fabricates staleness; targeting pinned by tests                      | pass   |
| The contract is pinned in the differential audit               | context-graph subject at corpus v57/325 files; audit 320/320 applicable required, 4 explicit platform skips, 0 accepted informational deviations; expectations 86 records via canonicalRecordDocument (surgical 1-record diff); pinned v32 oracle untouched           | pass   |
| The frozen interpretation table binds implementation           | module doc + build() enforce reconstruction-only (no persisted store), canonical ordering, bounded summaries/bindings; the scheduler/tools/benchmark clauses bind the pending slices                                                                                  | pass   |

## 4. Result

**Slice 1 — the Context Graph — is complete and pinned at corpus v57. Slices 2–5 (representations, the tiered scheduler, the context tools, audit + metrics) and the benchmark decision gate are pending their own slices.**
