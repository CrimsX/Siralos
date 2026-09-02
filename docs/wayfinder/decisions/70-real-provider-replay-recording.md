---
title: "Real Provider Replay Recording and Secret-Hygiene Audit — Determinism Ports for Provider Responses"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 70 — Real Provider Replay Recording and Secret-Hygiene Audit

**Ticket:** [68 — Real Provider Credentials and Registry](../tickets/68-real-provider-credentials-and-registry.md) · label `wayfinder:task` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md)
**Blocked by:** [68 — Real Provider Credentials and Registry](68-real-provider-credentials-and-registry.md) (68 §3/§4) · [69 — Real Provider Implementation Progress](69-real-provider-implementation-progress.md)

> **User-directed 2026-08-31 (session HITL).** Continuation of the Real Model/Provider range per decision 69; records the replay-recording slice (68 §3) and the recorded secret-hygiene sweep (68 §4). It grants no new scope: the Real Provider Verified roll-up remains the final frontier.

## 1. Replay recording of provider responses (68 §3)

- **Core:** `crates/siralos-core/src/determinism/provider_replay.rs` (new) — `ProviderResponseIdentity` (`provider_id`, `model`, `status: Option<u16>`, `body_sha256`, `body_bytes`, `observed_at_ms: Option<u64>`); `compute_provider_response_identity_digest` framed `siralos:ProviderResponseIdentity:v1\0` + canonical JSON via `digest_artifact_payload` (mirrors `compute_provider_input_identity_digest`); `ProviderReplayAvailability::{Recorded{digest}, Unavailable{reason}}` with `as_diagnostic`; object-safe `ReplayRecorder` port (`&self` methods); `NoopReplayRecorder`; `CollectingReplayRecorder` (insertion-order detached snapshot).
- **Adapters:** `ReplayHooks` (optional `Rc<dyn Clock>` + `Rc<dyn ReplayRecorder>`, manual redacted `Debug`) and `record_outcome`/`response_body_sha256` helpers in `crates/siralos-adapters/src/provider/mod.rs`; OpenAI/Anthropic/Generic providers gain `with_replay_support(clock, recorder)` and `take_last_replay_availability()` (take semantics, resetting to "no provider response observed yet"); **every** terminal observed outcome records exactly once — transport failures (client build / send / body read) as status `None` with empty body, any HTTP response as status + the sanitized bounded text. Records never contain the credential or raw body text (body sha256 only). Default construction is behavior-identical to before and yields the typed `Unavailable` "live call not recorded" — the typed-unavailable property required by 68 §3.
- **Registry:** `HostProvider::with_replay_support` (Fake variant unchanged) and `HostProvider::take_last_replay_availability` (Fake returns "deterministic-fake records no HTTP responses (inherently deterministic echo)").
- The closed `ProviderEvent`/`ModelEvent` set is untouched; the differential corpus is untouched; session composition is unchanged (no recorder bound by default).
- **Tests:** six in-file core tests (digest stability/sensitivity, noop, collecting order, snapshot detachment, diagnostic format) and three hermetic adapter tests (`generic_replay_records_transport_failure_with_typed_availability`, `generic_without_replay_support_is_unavailable_not_recorded`, `replay_record_debug_does_not_contain_credential_bytes`) using `FixedClock` + `CollectingReplayRecorder` over the unreachable loopback endpoint.

## 2. Secret-hygiene 3-surface audit (68 §4) — recorded sweep 2026-08-31

- **Portable config surfaces** (all non-Cargo `*.toml` and `*.lock` files in the repository): zero secret-shaped values (`sk-` prefixes, Bearer tokens, literal `api_key` assignments) — **none** found.
- **Repo-wide informational scan:** every `sk-`/`Bearer` match is either a substring false positive inside identifiers such as "task-binding"/"task-runtime", a doc comment (`crates/siralos-core/src/domain/lifecycle.rs:204`, `crates/siralos-core/src/doctor.rs:295`), or an intentionally fake `#[cfg(test)]` fixture (`sk-test`/`sk-secret`/`sk-super-secret-credential-999`; the sanitizer fixture in `crates/siralos-cli/src/harness.rs:4354`). No real credential exists in the repository.
- **Standing mechanical guards:** `HostCredential` `Debug`/`Display` redaction; replay records embed the body sha256 only; the public-hygiene and identity gates run in `npm run check`.

## 3. Verified evidence (orchestrator-run gates, independent of the implementing worker)

- `cargo fmt --all --check` exit 0; `cargo clippy --workspace --all-targets --all-features -- -D warnings` exit 0; `cargo test --workspace` exit 0 (core 511, adapters 146, domain-conformance 25, cli 37 — 0 failed); full `npm run check` exit 0 including the pinned differential (316/316 applicable required, pinned v32 oracle untouched).

## 4. Remaining frontier

- The Real Provider Verified roll-up only: fresh full gate, decisions 66–70 annotated, and the status flip ("in progress" → "Verified") in README, ROADMAP (which still lacks a Stage 7 / Real Provider section), PROJECT_CONTEXT, and AGENTS.

## 5. Criteria → evidence

| Criterion                                                               | Evidence                                                                  | Status   |
| ----------------------------------------------------------------------- | ------------------------------------------------------------------------- | -------- |
| Replay record exists per 68 §3                                          | `provider_replay.rs` + six core tests                                     | pass     |
| Adapters record every observed outcome exactly once, typed availability | per-adapter diffs + three hermetic adapter tests                          | pass     |
| Non-recorded live call is typed unavailable                             | `generic_without_replay_support_is_unavailable_not_recorded`              | pass     |
| Closed event set and corpus untouched                                   | git change set; `npm run check` exit 0 with pinned differential           | pass     |
| Credential/body hygiene of records                                      | `replay_record_debug_does_not_contain_credential_bytes`; body sha256 only | pass     |
| 68 §4 sweep recorded                                                    | section 2 above                                                           | pass     |
| Real Provider Verified roll-up                                          | pending                                                                   | **open** |
