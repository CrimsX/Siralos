---
title: "The Context Demand-Paging Tools (Slice 4)"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 82 — The Context Demand-Paging Tools (Slice 4)

**Governing plan:** [68 — Real Provider Credentials and Registry](68-real-provider-credentials-and-registry.md) · **Entry review:** [79](79-context-management-foundations.md) · **Map:** [Siralos Roadmap](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** Slice 4 of decision 79's staged plan: three read-only context tools over the graph, representation store, and scheduler state. The tools never mutate scheduler state — the model cannot promote, pin, or archive; access events are host-observed. Live session registration is deferred to the benchmark slice.

## 2. The three tools

| Tool              | Input                                | Output                                                                                                              | Failure                                                                                                            |
| ----------------- | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `context.inspect` | `{ node_id: String }`                | `{ id, kind, content_digest, summary, tier, pinned, stale, availableLevels, tokenEstimate }`                        | `invalid_input` for malformed `node_id`; `failed` for unknown node with typed not-found message                    |
| `context.search`  | `{ query: String }`                  | `{ query, hits: [{ node_id, kind, tier, matched_in }], truncated, hitCount }` sorted by `node_id`, `MAX_RESULTS=16` | `invalid_input` for empty query; success with empty hits when no match                                             |
| `context.expand`  | `{ node_id: String, level: String }` | `{ node_id, level, origin, content, content_digest, derived_from }`                                                 | `invalid_input` for malformed level; `failed` for unknown node; `unavailable` for absent level with available list |

## 3. Criteria → evidence

| Criterion                                                                | Evidence                                                                                                                                                                                                                                                                                                     | Status |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------ |
| Adapters are read-only demand-paging over graph/representation/scheduler | Three `Tool` adapters (`context.inspect`, `context.search`, `context.expand`) over an immutable `ContextToolState { graph, store, state, current_digests }`; no `&mut` state, no promotion/pin/archive, no absolute-path echo; `cargo clippy --workspace --all-targets --all-features -- -D warnings` exit 0 | pass   |
| Inspect returns full metadata with provenance and stale flag             | Inspect `ctx-a` asserts `id`, `kind=source`, `content_digest`, `summary`, `tier=hot`, `pinned=true`, `stale=false`, `availableLevels=[identity, structured]`, `tokenEstimate`; inspect `ctx-knowledge` asserts `stale=true`; unknown node returns typed `failed` not-found                                   | pass   |
| Search is deterministic lexical with canonical order and bounds          | Case-insensitive substring on `id` and whole-word-ish containment on `summary`; `MAX_RESULTS=16` cap with `node_id` canonical order; hits on `id` and `summary` asserted; empty query returns `invalid_input`; no-hits returns success empty                                                                 | pass   |
| Expand resolves honesty without fabrication                              | Expand `ctx-a` at `structured` resolves `content_digest`, `origin=host_extracted`, `derived_from` provenance; expand `ctx-b` at `structured` returns `unavailable` with `available: [identity]`; unknown node `failed`; malformed level `invalid_input`                                                      | pass   |
| Authority: tools never mutate scheduler state                            | Mutation guard asserts `WorkingSetState` tiers/entries unchanged after all tool calls; adapters take only immutable snapshots; `cargo test --workspace --all-targets --all-features` exit 0 (core 556, adapters 181, cli 71, 0 failed)                                                                       | pass   |
| The contract is pinned in the differential audit                         | `context-tool` subject at corpus v60/328 files; audit 323/323 applicable required, 4 explicit platform skips, 0 accepted informational deviations; expectations 89 records via `canonicalRecordDocument` (surgical 88→89 diff); pinned v32 oracle untouched                                                  | pass   |
| Pinned oracle untouched and gates green                                  | pinned v32 oracle still 234/234; `cargo fmt --all --check` exit 0; `npm run check:differential` exit 0; `npx prettier --check` exit 0; `node scripts/check-doc-links.mjs` exit 0; `npm run check:context` exit 0                                                                                             | pass   |

## 4. Result

Slice 4 — the context demand-paging tools — is complete and pinned at corpus v60; live session registration and the minimal benchmark are the next slices.
