---
title: "Session Replay-Run Composition — In-Process Record-Then-Replay"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 77 — Session Replay-Run Composition

**Governing plan:** [68 — Real Provider Credentials and Registry](68-real-provider-credentials-and-registry.md) · **Prior roll-up:** [76](76-real-provider-followthrough-verified-rollup.md) · **Map:** [Siralos Roadmap](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** Entry review and slice in one: the user chose Option A — in-process record-then-replay only; recordings are never persisted (decision 73's invariant stays frozen); cross-session replay via a persisted store is explicitly deferred to a future entry review.

## 1. The composition

- Core: SessionReplayEvidence { providerId, model, recordedCount, recorderSnapshotCount } + compute_session_replay_evidence_digest over digest_artifact_payload("SessionReplayEvidence", 1) — in-process run evidence, nothing persisted.
- Adapters: SessionReplayComposer::new(provider_id, model) creates the retaining recorder; recorder() hands the Rc to attach to the live provider for the record phase; compose() serves the detached snapshot via the decision 74 seam (copy semantics); evidence() reports the run.
- Differential: the hermetic session-replay subject pins the full two-phase contract at corpus v55/323 files: record two OpenAI-shaped responses, replay them in insertion order, typed exhausted failure on the third turn, recorder snapshot intact after playback, digest-bound evidence.

## 2. Criteria → evidence

| Criterion                                                 | Evidence                                                                                                                                                                                                                                                     | Status |
| --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------ |
| The composer binds the frozen seams without new authority | SessionReplayComposer over RetainingReplayRecorder + replay_provider_from_recorder; 2 new adapter tests (round trip with typed exhausted failure + digest consistency); recorder snapshot intact after playback                                              | pass   |
| Nothing is persisted                                      | bodies stay in memory only; no filesystem surface in the composer; harness diff adds zero network or process references                                                                                                                                      | pass   |
| The contract is pinned in the differential audit          | session-replay subject at corpus v55/323 files; audit 318/318 applicable required, 4 explicit platform skips, 0 accepted informational deviations; expectations 84 records via canonicalRecordDocument (surgical 1-record diff); pinned v32 oracle untouched | pass   |
| Focused gates                                             | cargo fmt --all --check exit 0; clippy --workspace --all-targets --all-features -D warnings exit 0; cargo test --workspace exit 0 (core 516, adapters 153, conformance 25, cli 37, 0 failed)                                                                 | pass   |
| Cross-session replay deferred                             | no persisted recordings store in this slice; a persisted store would amend decision 73 and requires its own entry review                                                                                                                                     | pass   |

## 3. Result

**Session replay-run composition is complete and pinned at corpus v55. Cross-session replay (persisted recordings) remains explicitly deferred and unticketed.**
