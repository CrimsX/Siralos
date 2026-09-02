---
title: "Replay Composition Seam — From Retaining Recorder to Replay Provider"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 74 — Replay Composition Seam — From Retaining Recorder to Replay Provider

**Governing plan:** [68 — Real Provider Credentials and Registry](68-real-provider-credentials-and-registry.md) · **Follows:** [73 — Recorded-Response Replay Provider](73-recorded-response-replay-provider.md) · **Map:** [Siralos Roadmap](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** First concrete step of session replay-run composition: a pure composition function over the existing seams; no session/CLI wiring, no config surface, no corpus change.

## 1. The seam

- One function over the existing seams: `RetainingReplayRecorder::records_snapshot` -> `RecordedReplayProvider` — `replay_provider_from_recorder(provider_id, model, recorder)` composes a `RecordedReplayProvider` from the recorder's detached snapshot (`records_snapshot()`); the recorder keeps its recordings, the provider serves a copy — the copy semantics are the contract.
- Pure composition: no new storage, no config surface, no session wiring; the seam is the only public addition in this slice.

## 2. Criteria → evidence

| Criterion                                                 | Evidence                                                                                                                                                                                     | Status |
| --------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Seam exists and composes from a live recorder             | round-trip test: record 2, replay 2, exhausted typed failure                                                                                                                                 | pass   |
| Copy semantics (recorder snapshot unaffected by playback) | asserted in the test                                                                                                                                                                         | pass   |
| Hygiene (bodies in memory only, digest-bound identities)  | recordings carry sanitized bodies; nothing persisted                                                                                                                                         | pass   |
| Focused gates                                             | cargo fmt --all --check exit 0; clippy --workspace --all-targets --all-features -D warnings exit 0; cargo test --workspace exit 0 (core 515, adapters 151, conformance 25, cli 37, 0 failed) | pass   |
| Corpus untouched                                          | change set has no tests/differential or CLI files                                                                                                                                            | pass   |

## 3. Result

**The composition seam is public; actual session/CLI wiring (config surface for replay runs) remains future work needing its own entry review.**
