---
title: "Real Provider Credentials and Registry — Env Resolution and Host-Mediated HTTP Adapters"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 68 — Real Provider Credentials and Registry

**Ticket:** [68 — Real Provider Credentials and Registry](../tickets/68-real-provider-credentials-and-registry.md) · label `wayfinder:task` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md)
**Blocked by:** [67 — Entry Review](../decisions/67-real-model-provider-entry-review.md) (C1–C6 frozen PASS 2026-08-31)

> **HITL 2026-08-31 — credential + registry + HTTP adapter plan frozen, no code in this decision.** The next implementation slice (`ProfileRecord` provider/model fields + `siralos.toml` parsing) is authorized per decision 67 C6; the HTTP registry slice remains frozen but not authorized until that slice is Verified.

## 1. Credential shape — `env:` references only, never in portable config

**`siralos.toml` syntax (bounded, validated at the boundary):**

```toml
[profile]
name = "default"
provider = "openai"          # bounded 1..64, [a-z0-9_-], validated in ProfileRecord
model = "gpt-4o"             # bounded 1..128, [a-zA-Z0-9._-], validated
credential = "env:OPENAI_API_KEY"  # literal prefix "env:" + env name 1..64, [A-Z0-9_], validated; value is never the secret itself
```

**Validation (pure, bounded, deterministic):**

- `siralos.toml` parsing in `crates/siralos-adapters/src/profile_config.rs` (bounded size, unknown keys rejected) validates `provider`/`model`/`credential` strings at the boundary — NUL bytes, oversize, or bad charset → `ProfileValidationError` (profile not applied, truthful diagnostic, per `composition.rs:118`).
- `credential` value must match `^env:[A-Z0-9_]{1,64}$`; any other form (e.g., `sk-...`, `file:...`) is rejected before any network call.
- `~/.siralos/config.json` keeps its existing shape (`quality.reviewProvider` already validated at `configuration.rs:93`); no credential field is added there — `siralos.toml` is the single `credential` source for providers.

**Host resolution (in-memory only, never written):**

- At `load_workspace_profile` / `load_user_configuration` time, the Host does `std::env::var(env_name)` for the `env:` reference; the resolved bytes are held in a `HostCredential` newtype that is `!Serialize`, `!Debug` (redacted), and lives only for the `ModelProvider` call.
- The resolved bytes are **never** written to `siralos.toml`, `siralos.lock` (which records only `provider`/`model` identity per `composition/lock.rs`), `Context` projection, `siralos lock` output, `Cargo.lock`, or logs. `Debug`/`Display` for `HostCredential` prints `[REDACTED]`.
- The `missing_docs`/`clippy` lints plus a `grep -r "env:"` sweep over `siralos.toml`/`siralos.lock`/`Context` would fail if a secret value ever appears (the sweep checks for `sk-` prefix or `Bearer` in those files).

## 2. Registry — `provider` string → `ModelProvider`

**`siralos_adapters::provider::registry` (or `siralos_core::provider::registry`):**

```rust
pub enum ProviderKind { DeterministicFake, OpenAi, Anthropic }

pub fn provider_kind_from_str(s: &str) -> Result<ProviderKind, UnknownProvider> {
    match s {
        "deterministic-fake" => Ok(ProviderKind::DeterministicFake),
        "openai" => Ok(ProviderKind::OpenAi),
        "anthropic" => Ok(ProviderKind::Anthropic),
        other => Err(UnknownProvider { provider_id: other.to_owned() }),
    }
}
```

- The bounded `provider` string from `ProfileRecord`/`EffectiveRunPolicy` is mapped via `provider_kind_from_str`; unknown → typed `UnknownProvider` refusal **before any network call**, with a truthful diagnostic that never echoes the credential (`"unknown provider \"{provider_id}\" — configure it or remove the setting"`).
- The registry constructs the concrete `ModelProvider` (`DeterministicFakeProvider`, `OpenAiProvider`, `AnthropicProvider`) with the `HostCredential` and the `siralos_core::determinism::Clock` injected.

## 3. HTTP adapter — `siralos_adapters::provider::openai` (and `anthropic`)

**Transport:** `reqwest` or `hyper` (choose one at implementation, not in this decision) with the `siralos_core::determinism::Clock` for connect/read timeouts, strict timeouts (e.g., 10s connect, 60s read), and a bounded transcript (16-code-point chunking preserved from `deterministic-fake`).

**Host-observed and replay-recordable:**

- Every request/response byte count and timing is observed via the `Clock` and digested via `siralos_core::identity::{sha256_hex, canonicalize_json}` for the `determinism-replay` record (like `determinism-replay` fixtures).
- Responses are recorded via the determinism ports for replay; a live call without recording is a typed `unavailable` for replay (per `determinism-replay.task-plan-digest.json`).

**No hidden unbounded retry:** the `tool-loop` budget (16 rounds) is the only retry; the HTTP adapter itself does not retry beyond one bounded attempt.

## 4. Secret hygiene — 3-surface audit

| Surface                                             | Check                                                                                                                             | Evidence                                                     |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| `siralos.toml`                                      | `grep -r "sk-"` and `grep -r "Bearer"` over the file must be empty; `credential` must match `^env:`                               | `profile_config.rs` validation                               |
| `siralos.lock`                                      | `siralos.lock` must contain only `provider`/`model` identity, never credential value; `lock.rs` never serializes `HostCredential` | `composition/lock.rs`                                        |
| `Context` projection + `siralos lock` output + logs | `Context` never includes `HostCredential`; `siralos lock` output never echoes it; `missing_docs`/`clippy` lints                   | `crates/siralos-cli/src/interactive.rs` + `configuration.rs` |

## Self-loop verification (task planning)

| Criterion                                                             | Evidence                                                                            | Verdict |
| --------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ------- |
| Credential shape is `env:` only, never in portable config             | §1 `siralos.toml` syntax, `profile_config.rs` validation, `HostCredential` redacted | pass    |
| Registry maps bounded `provider` → `ModelProvider` with typed refusal | §2 `provider_kind_from_str`, `UnknownProvider`                                      | pass    |
| HTTP adapter is bounded, Host-observed, replay-recordable             | §3 `determinism::Clock`, `identity` digests, `tool-loop` budget                     | pass    |
| Secret hygiene is auditable                                           | §4 3-surface `grep` sweep, `missing_docs`/`clippy`                                  | pass    |
| No new authority/spawn beyond `ModelProvider`                         | `ModelProvider` trait bound, `ToolRegistry` gate per decision 67 C4                 | pass    |

**Planning for credentials and registry is PASS — the `ProfileRecord` provider/model fields + `siralos.toml` parsing slice is now fully planned and authorized per decision 67 C6; the HTTP registry slice remains frozen but not authorized until that slice is Verified.**
