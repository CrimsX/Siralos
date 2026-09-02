---
title: "Recorded-Response Replay Provider — Serving Determinism-Port Recordings"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 73 — Recorded-Response Replay Provider

**Governing plan:** [68 — Real Provider Credentials and Registry](68-real-provider-credentials-and-registry.md) · **Recording side:** [70](70-real-provider-replay-recording.md) · **Map:** [Siralos Roadmap](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** Completes the "for replay" half of decision 68 §3: recordings can now be served back as provider event streams. Design frozen here: retained bodies live in memory only (sanitized, bounded, never persisted to disk, never containing credentials); playback is insertion-ordered; no session composition wiring in this slice.

## 1. The playback side

- Core (`crates/siralos-core/src/determinism/provider_replay.rs`): the `ReplayRecorder` trait gains a DEFAULT no-op third method `record_provider_response_with_body(identity, body)` — additive and object-safe, existing recorders unaffected; new `ReplayRecording { identity, body }` (body = sanitized bounded text, in memory only); new `RetainingReplayRecorder` (`is_recording` true; retains identity + body on the body method; digest computed at retain time; detached snapshot).
- Adapters: `record_outcome` now calls both `record_provider_response` (identity) and `record_provider_response_with_body` (identity + sanitized bounded body) when a recorder is present — Collecting/Noop keep their exact prior behavior via the default.
- Shared parser: the body-to-events conversion tail of `GenericProvider::call_generic` moved verbatim into `crates/siralos-adapters/src/provider/replay.rs::completion_events_from_body` (OpenAI choices shape, Anthropic content/tool_use fallback shape, empty-TextDelta fallback, final Completed) — behavior-identical for the live generic adapter, now shared with playback.
- New `RecordedReplayProvider` (same file): `ModelProvider` serving recordings in insertion order; recordings exhausted -> typed `Failed("no recorded response for replay: recording exhausted")`; cancellation-first with the standard `Cancelled` wording; `id()` = the recorded provider identity; `Display` redacted; bodies never leave memory.
- No session composition wiring: composing a replay run from retained recordings is future work; the differential corpus is untouched.

## 2. Criteria → evidence

| Criterion                                                                                           | Evidence                                                                                                                                                                                     | Status |
| --------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Trait extension is additive (existing recorders unchanged)                                          | default no-op method; all prior Collecting/Noop tests pass unchanged                                                                                                                         | pass   |
| Retaining recorder captures identity + body in memory only                                          | 4 new core tests incl. digest consistency and snapshot detachment                                                                                                                            | pass   |
| Body-to-events extraction is behavior-identical                                                     | verbatim move confirmed by diff; every existing generic adapter test passes unchanged                                                                                                        | pass   |
| Playback semantics (insertion order, exhausted typed failure, cancellation-first, redacted Display) | 4 new adapter tests: replay_serves_recorded_body_as_events, replay_exhausted_is_typed_failure, replay_cancellation_before_start, retaining_recorder_round_trip_through_generic               | pass   |
| Hygiene of retained bodies                                                                          | bodies are the sanitized bounded texts only; RetainingReplayRecorder retains in memory; nothing persisted                                                                                    | pass   |
| Focused gates                                                                                       | cargo fmt --all --check exit 0; clippy --workspace --all-targets --all-features -D warnings exit 0; cargo test --workspace exit 0 (core 515, adapters 150, conformance 25, cli 37, 0 failed) | pass   |
| Corpus untouched                                                                                    | change set has no tests/differential or CLI files                                                                                                                                            | pass   |

## 3. Result

**Recorded responses can now be replayed as provider event streams.** Remaining follow-through (not ticketed here): session-composition wiring that binds a `RetainingReplayRecorder` and swaps in a `RecordedReplayProvider` for replay runs.
