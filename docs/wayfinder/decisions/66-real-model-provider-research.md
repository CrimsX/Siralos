---
title: "Real Model/Provider Research — Inventory and Secure Credential Requirements"
label: "wayfinder:research"
status: accepted
date: 2026-08-31
ticket: "66"
supersedes: []
---

# Decision 66 — Real Model/Provider Research

**Ticket:** [66 — Real Model/Provider Research](../tickets/66-real-model-provider-research.md) · label `wayfinder:research` AFK
**Map:** [Siralos Roadmap](../siralos-roadmap.md)

> Fact sheet only — no proposal, no code, no corpus bump. Provides the frozen file:line evidence that [67 — Entry Review](../tickets/67-real-model-provider-entry-review.md) must consume.

## 1. Current provider surface — only `deterministic-fake`

| Item | Location | Evidence |
|------|----------|----------|
| Provider-neutral types | `crates/siralos-core/src/provider/mod.rs` | `ModelProvider` trait, `ModelRequest`/`ModelEvent`/`Conversation`/`ProviderError`, trust boundary for unknown discriminators |
| Fake provider | `crates/siralos-adapters/src/provider/mod.rs` (`deterministic-fake`) | echo with 16-code-point chunking, `id = "deterministic-fake"` |
| CLI composition — only fake registered | `crates/siralos-cli/src/configuration.rs:18` | `DEFAULT_REVIEW_PROVIDER_ID = "deterministic-fake"` |
| CLI composition — validation | `crates/siralos-cli/src/configuration.rs:93-102` | `quality.reviewProvider` absent → defaults to `deterministic-fake`; any other value → `UnknownReviewProvider` error before any network call |
| Harness fake | `crates/siralos-cli/src/harness.rs` + `crates/siralos-cli/src/harness/tool_loop.rs` | `ToolLoopScriptedProvider` for differential, `ModelProvider` for `provider-turn`/`tool-loop` subjects (corpus `provider-turn` ×18, `tool-loop` ×16) |
| ProfileRecord — no provider/model | `crates/siralos-core/src/composition.rs:60` | `ProfileRecord { name, overlay, plugins, context, skills }` — no `provider`/`model` fields |

## 2. Configuration surface — `siralos.toml` vs `~/.siralos/config.json`

| File | Purpose | Evidence |
|------|---------|----------|
| `siralos.toml` `[profile]` | Workspace profile: `name`, `[profile.permissions]` overlay, `plugins`, `context`, `skills` | `crates/siralos-adapters/src/profile_config.rs:1` (bounded `name`, `permissions`, size-capped, unknown keys rejected) + `crates/siralos-core/src/composition.rs:60` |
| `~/.siralos/config.json` | User config: `quality.reviewProvider`, `references` | `crates/siralos-adapters/src/config.rs` (`load_user_config`), `crates/siralos-cli/src/configuration.rs:85` (`load_user_configuration`) |
| `siralos.lock` | Machine-generated digest-bound lock for `siralos.toml` | `crates/siralos-core/src/composition/lock.rs` + `crates/siralos-adapters/src/lockfile.rs` (deterministic, `siralos.lock` never stores secrets per ADR 0036 §10) |

## 3. Host authority, determinism, and replay

| Invariant | Location | Evidence |
|-----------|----------|----------|
| Trust boundary | `crates/siralos-core/src/provider` | unknown/malformed discriminators fail closed before typed acceptance |
| Determinism ports | `crates/siralos-core/src/determinism` | explicit `clock`/`random`/`ordering` ports — real provider calls must use the `clock` for timeouts and be recorded |
| Identity/digests | `crates/siralos-core/src/identity` | `sha256_hex`, `canonicalize_json` for `determinism-replay` records |
| Tool authority | `crates/siralos-core/src/tool` | `ToolRegistry` allow/ask/deny, `displayInput` UTF-16 truncation, `ToolRound` one-call/one-result — provider never decides authority |
| Replay vs live | `crates/siralos-core/src/determinism` + `crates/siralos-cli/src/harness.rs` `determinism-replay` fixtures (4 files) | recorded observations consumed for replay; live calls without recording are `unavailable` for replay per `determinism-replay.task-plan-digest.json` |

## 4. Out of scope per ADR 0036 — what real providers must NOT introduce

| Item | ADR 0036 | Evidence |
|------|----------|----------|
| No automatic model-router | §51 Removed, `ARCHITECTURE.md: Out of scope` | Profile selects `provider`/`model` explicitly; Host never auto-picks |
| No hidden credential storage | §10 No secrets in portable config | `credential = "env:OPENAI_API_KEY"` pattern, `siralos lock` never captures it |
| No unbounded network | `siralos_core::provider` bounded `tool-loop` budget | `tool-loop` subject enforces round-budget, `provider-turn` enforces 16-code-point chunking |
| No automatic Skill/Plugin acquisition | §35-36 explicit install only | `DomainHost::install` SHA-256 gate per decisions 38/39 |

## 5. What is NOT needed for real providers

- No new `siralos.toml` executable language — `provider`/`model` are bounded strings, not code.
- No `siralos.lock` secret capture — lock records `provider`/`model` identity, not credential values.
- No model-router — user picks `provider`/`model` in `[profile]`.

## Self-loop verification (AFK research)

| Criterion | Evidence | Verdict |
|-----------|----------|---------|
| Current provider is only fake | §1 table `configuration.rs:18` + `ProfileRecord:60` | pass |
| Config surfaces enumerated | §2 table `profile_config.rs:1`, `config.rs`, `composition.rs:60` | pass |
| Host authority/determinism located | §3 table `provider`, `determinism`, `identity`, `tool` | pass |
| Out of scope collected | §4 table ADR 0036 §51, `ARCHITECTURE.md` | pass |

Decision 66 is **fact sheet PASS** — provides frozen evidence for [67 Entry Review](../tickets/67-real-model-provider-entry-review.md). No code, no corpus bump, no behavior change.
