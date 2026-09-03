---
title: "Real Provider Verified Roll-Up II — the Guardrail Gate and the Replay Arc"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 76 — Real Provider Verified Roll-Up II

**Governing plan:** [68 — Real Provider Credentials and Registry](68-real-provider-credentials-and-registry.md) · **Prior roll-up:** [71](71-real-provider-verified-roll-up.md) · **Map:** [Siralos Roadmap](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** Pure closure record over decisions 72–75 — the standing guardrail gate and the replay arc. No behavior change in this decision; the range 66–75 is closed as Verified.

## 1. Scope

- [Mechanical Secret-Hygiene Gate](72-mechanical-secret-hygiene-gate.md) — the 68 §4 sweep became a standing mechanical `check:secrets` gate chained into `npm run check`.
- [Recorded-Response Replay Provider](73-recorded-response-replay-provider.md) — `RetainingReplayRecorder` retains sanitized bodies in memory and `RecordedReplayProvider` serves them as ordered event streams.
- [Replay Composition Seam](74-replay-composition-seam.md) — `replay_provider_from_recorder` composes a replay provider from a retaining recorder's detached snapshot with copy semantics.
- [Provider-Replay Differential Subject](75-provider-replay-differential-subject.md) — the replay contract is pinned at corpus v54/322 files with the `provider-replay` differential subject.

72 added a developer guardrail (no product code); 73–75 added replay recording playback and its corpus pinning (no spawn paths, no runtime behavior beyond the replay types).

## 2. Criteria → evidence

| Criterion                                                                       | Evidence                                                                                                                                                                                                                                                                                                                                                | Status |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| The 68 §4 sweep is a standing mechanical gate                                   | decision 72: check:secrets chained into npm run check after check:public; orchestrator fail-first probe (planted credential-shaped file → exit 1 reporting path:line:pattern-name only, no echoed secret; file removed → exit 0)                                                                                                                        | pass   |
| The replay arc is complete: recording → playback → composition → corpus pinning | decisions 73/74/75: RetainingReplayRecorder + RecordedReplayProvider + replay_provider_from_recorder; provider-replay subject at corpus v54/322 files; differential audit 317/317 applicable required, 4 explicit platform skips, 0 accepted informational deviations; expectations 83 records via canonicalRecordDocument; pinned v32 oracle untouched | pass   |
| Hygiene invariants held across the range                                        | retained bodies live in memory only and are never persisted (73); the secret-hygiene gate is green on the final tree (72); zero spawn paths added in 72–75                                                                                                                                                                                              | pass   |
| Fresh full gate green at the roll-up point                                      | npm run check exit 0 at 04e89f8 (orchestrator-run 2026-08-31, includes check:secrets and check:differential at v54)                                                                                                                                                                                                                                     | pass   |
| Range closure is recorded                                                       | decisions 72–75 indexed in the map; status documents extended in place, not rewritten                                                                                                                                                                                                                                                                   | pass   |

## 3. Result

**The Real Provider range (66–75) is fully Verified. The named next frontier — session replay-run composition (a config surface binding a retaining recorder and swapping in a replay provider for replay runs) — is NOT ticketed here; it requires its own ticket and entry review per ADR 0036.**
