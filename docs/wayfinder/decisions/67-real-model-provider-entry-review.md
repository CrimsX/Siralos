---
title: "Real Model/Provider Entry Review — Freeze the Provider Contract"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "67"
supersedes: []
---

# Decision 67 — Real Model/Provider Entry Review

**Ticket:** [67 — Real Model/Provider Entry Review](../tickets/67-real-model-provider-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md)
**Blocked by:** [66 — Research](../decisions/66-real-model-provider-research.md)

> **PASS — HITL 2026-08-31: C1–C6 approved** (provider/model in ProfileRecord, env: only, bounded Host-observed, ToolRegistry gate, 4-step ordering). Authorizes `ProfileRecord` provider/model fields + `siralos.toml` parsing as next implementation slice.

## Question

`siralos` today only has `deterministic-fake` (`crates/siralos-cli/src/configuration.rs:18`). `ProfileRecord:60` has no `provider`/`model`. How is the real provider contract frozen so an executor can implement it without rediscovering scope, authority, or determinism boundaries?

## Contract (C1–C6, approved 2026-08-31)

| # | Clause | Frozen contract |
|---|--------|-----------------|
| C1 | Scope — provider/model in ProfileRecord | `ProfileRecord` gains **bounded optional `provider`/`model` fields** (e.g., `provider = "openai"`, `model = "gpt-4o"` — bounded strings, validated at the boundary like `name` at `composition.rs:118`, unknown keys rejected). They feed `EffectiveRunPolicy` but never grant network/credential authority by themselves. `siralos.toml [profile]` may declare them; missing → `None` (transparent, Host defaults to `deterministic-fake` per `configuration.rs:93`). No model-router — user picks explicitly per ADR 0036 §51. |
| C2 | Credentials — never in portable config | Secrets **never** in `siralos.toml`, `siralos.lock`, `Context` projection, `siralos lock` output, `Cargo.lock`, or logs per ADR 0036 §10. Providers receive credentials only via `credential = "env:OPENAI_API_KEY"` (bounded `env` name allowlist, validated at `profile_config.rs` boundary) that the Host resolves at `load_workspace_profile` / `load_user_configuration` time; the resolved bytes are held in memory only for the `ModelProvider` call and never written to `siralos.toml`/`siralos.lock`/`Context`/`logs`. CLI `--help` and `/context` never echo them. |
| C3 | Network and determinism | Real provider calls are **explicit, bounded, and Host-observed**: `siralos_adapters::provider::{openai,anthropic}` implements `ModelProvider` over `reqwest`/`hyper` with the `siralos_core::determinism` clock, strict connect/read timeouts, and a bounded transcript (16-code-point chunking preserved). Responses are recorded via `siralos_core::determinism`/`identity` (`sha256_hex`, `canonicalize_json`) for `determinism-replay` style replay; a live call without recording is a typed `unavailable` for replay. No hidden auto-retry beyond the bounded `tool-loop` budget (`tool-loop` ×16, `provider-turn` ×18). |
| C4 | Tool calling | Model-requested tool calls are validated against the Host's `ToolRegistry` (allow/ask/deny at `crates/siralos-core/src/tool`) before any `Tool::execute`; the provider never decides authority. `displayInput` truncation stays UTF-16 code-unit exact. Provider output remains **untrusted data** until Host validation, then Host-observed evidence. |
| C5 | Verification | Each provider slice contributes a frozen differential subject over pure seams (e.g., `provider-openai` ×4) or `check:differential` expectations per decision 40 C7 (pinned v32 oracle untouched); `cargo test --workspace --all-features` + `npm run check` stay green; no new `unsafe`, no spawn beyond the typed `ModelProvider` call, no secret in logs. Corpus bump `v52 → v53+` with strict-loader asserts together (`crates/siralos-cli/src/harness.rs` + `tests/differential/corpus/manifest.json` + `contract.mjs`) per map Notes. |
| C6 | Lean guardrails & ordering | No model-router, no automatic plugin acquisition, no generic Memory, no GUI/TUI, no marketplace (ADR 0036 §51). Ordered slices after PASS: (1) 66 research (done) → (2) 67 entry review (this) → (3) `ProfileRecord` provider/model fields + `siralos.toml` parsing → (4) credential resolution + `siralos_adapters::provider` registry + `ModelProvider` HTTP adapters (one provider at a time). Each slice entry-reviewed, one per session, budget one coherent pass + up to two repairs. |

## HITL answers (2026-08-31, recorded verbatim)

- **Q1 Scope** — approved: *"Add to ProfileRecord"* — bounded optional `provider`/`model` in `ProfileRecord`, no model-router.
- **Q2 Credentials** — approved: *"Env only"* — `env:` references only, never in portable config.
- **Q3 Network** — approved: *"Bounded Host-observed"* — explicit, bounded, Host-observed with replay.
- **Q4 Tools** — approved: *"Host gate"* — `ToolRegistry` gate before execute, untrusted until validated.
- **Q5 Ordering** — approved: *"Approve ordering"* — 4-step ordered slice sequence.

**Entry review is PASS — provider contract frozen, `ProfileRecord` provider/model fields + `siralos.toml` parsing authorized as next implementation slice** against this frozen C1–C6 and the 5 HITL answers above. Credential + HTTP registry slice remains frozen but not authorized until the `ProfileRecord` slice is Verified.

## Self-loop verification

| Criterion | Evidence | Verdict |
|-----------|----------|---------|
| Research consumed | decision 66 fact sheet (deterministic-fake at `configuration.rs:18`, `ProfileRecord:60`, `siralos.toml`/`config.json`/`siralos.lock`, determinism/replay) | pass |
| C1–C6 frozen without new behavior | table above, ADR 0036 §10 `env:` pattern, `siralos_core::provider` trust boundary | pass |
| Credentials never in portable config | C2 `env:` only, never in `siralos.toml`/`siralos.lock`/`Context`/`logs` | pass |
| Network is bounded and replay-recordable | C3 `determinism` clock, strict timeouts, `identity` digests | pass |
| Human decided 5 material cuts | HITL answers 2026-08-31 verbatim | pass |

## Implementation record

_Authorized at 2026-08-31 — no code in this decision. Next slice `ProfileRecord` provider/model fields + `siralos.toml` parsing is authorized as next implementation slice against frozen C1–C6._
