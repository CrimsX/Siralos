---
title: "Provider-Replay Differential Subject — Pinning the Replay Contract at Corpus v54"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 75 — Provider-Replay Differential Subject — Pinning the Replay Contract at Corpus v54

**Governing plan:** [68 — Real Provider Credentials and Registry](68-real-provider-credentials-and-registry.md) · **Follows:** [73 — Recorded-Response Replay Provider](73-recorded-response-replay-provider.md) / [74 — Replay Composition Seam](74-replay-composition-seam.md) · **Map:** [Siralos Roadmap](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** Pins the decision 73/74 replay-playback contract into the differential audit following the provider-generic precedent (decision 40 C7 candidate-authored expectations); hermetic — recordings constructed in memory, no network.

## 1. The subject

- Scenario: `tests/differential/corpus/provider-replay.json` — one required scenario (`id: provider-replay`, `platforms: ["*"]`, `parity: required`, minimal input `{}`) covering the hermetic replay contract.
- Record: `provider_replay_record` constructs two `ReplayRecording` values (`alpha`/`beta` bodies, `ProviderResponseIdentity { provider_id: "replay-subject", model: "replay-model", status: Some(200), body_sha256 = sha256(body), body_bytes = body.len(), observed_at_ms: Some(1000) }`), records them via `RetainingReplayRecorder` (`record_provider_response` then `record_provider_response_with_body` for each), composes `replay_provider_from_recorder("replay-subject","replay-model",&recorder)`, streams three turns with fresh cancellation tokens and canonicalizes events as `provider_generic_record` does. Shape: `providerId`, `model`, `remainingCounts` progression (`[2,1,0]` before each turn), `turn1Events` (text-delta alpha + completed), `turn2Events` (text-delta beta + completed), `turn3Events` (failed "no recorded response for replay: recording exhausted"), and `recorderSnapshotCount: 2` proving copy semantics (recorder untouched by playback).

## 2. Criteria → evidence

| Criterion                                                                   | Evidence                                                                                                                                                                                | Status |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| subject added at v54 (322 files)                                            | `CORPUS_VERSION` 53→54, `provider-replay.json` added, manifest regenerated, strict-loader 321→322                                                                                       | pass   |
| audit 317/317 applicable required, pinned v32 oracle untouched              | `npm run check:differential` parity held 317/317 applicable required, 4 explicit platform skips, 0 informational, pinned oracle `typescript-freeze-v32/oracle.json` untouched           | pass   |
| expectations 83 records via canonicalRecordDocument, surgical 1-record diff | `tests/differential/evidence/post-freeze/expectations.json` 82→83 records, rewritten via `canonicalRecordDocument`, one trailing newline, diff is one appended `provider-replay` record | pass   |
| hermetic (no network, no credentials)                                       | recordings constructed in memory, no provider HTTP, no loopback endpoint, no credential                                                                                                 | pass   |

## 3. Result

**The full replay contract (ordered playback, typed exhausted failure, copy semantics) is pinned at corpus v54/322 files; audit 317/317 applicable required, expectations 83 records via canonicalRecordDocument, pinned v32 oracle untouched.**
