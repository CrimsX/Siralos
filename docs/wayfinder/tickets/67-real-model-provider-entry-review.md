---
title: "Real Model/Provider Entry Review — Freeze the Provider Contract"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: "decisions/67-real-model-provider-entry-review.md"
blockedBy: ["66-real-model-provider-research.md"]
---

# Ticket 67 — Real Model/Provider Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Decision:** will open as `docs/wayfinder/decisions/67-real-model-provider-entry-review.md` after HITL PASS

## Question

`siralos` today only has `deterministic-fake` (`crates/siralos-cli/src/configuration.rs:18`). `siralos.toml` `[profile]` has no `provider`/`model` fields, and `siralos_core::composition::ProfileRecord:60` has `name`/`overlay`/`plugins`/`context`/`skills` only. Real providers (OpenAI, Anthropic, etc.) need credential handling (`env:` references per ADR 0036 §10), model selection, streaming, tool calling, and deterministic replay. How is the real provider contract frozen so an executor can implement it without rediscovering scope, authority, or determinism boundaries?

> Wayfinder discipline: Plan, don't do. This ticket freezes the decision contract; implementation follows only after HITL PASS (one ticket per session). No provider code lands here.

## Why now

- **Preconditions met:** Stage 6 Verified (`e2c3540` 315/315) and Stage 7 moved (`87bfd35` 3 members, `1bf2ca3` plugin self-contained) are green; the fake provider is the only registered provider and the `UnknownReviewProvider` gate proves it.
- **No open frontier tickets:** `60–65` are closed; `66` research is the only frontier — without a frozen contract, the next implementation would invent credential storage, model-router, or hidden network access (all Out of scope per ADR 0036 §51).
- **High-consequence:** provider code touches network, credentials, and host authority — HITL C1–C6 required before any `siralos_core::provider` or `siralos_adapters::provider` code.

## Contract draft (C1–C6) — draft for HITL confirmation

| # | Clause | Draft |
|---|--------|-------|
| C1 | Scope — what is provider vs profile | **Profile selects, Host authorizes.** `siralos.toml [profile]` may declare `provider = "openai"` / `model = "gpt-4o"` (bounded strings, validated at the boundary, no code execution). `ProfileRecord` gains `provider`/`model` optional fields (bounded, validated) that feed `EffectiveRunPolicy` but never grant network or credential authority by themselves. The `ModelProvider` trait stays provider-neutral (`crates/siralos-core/src/provider`). No model-router — user picks explicitly. |
| C2 | Credentials — never in portable config | Secrets **never** in `siralos.toml` or `siralos.lock` per ADR 0036 §10. Providers receive credentials only via `credential = "env:OPENAI_API_KEY"` (or equivalent host-mediated reference) that the Host resolves at startup; the resolved value is never written to `siralos.toml`/`siralos.lock`/`Context`/`logs`, and `siralos lock` never captures it. CLI `--help` and `/context` never echo it. |
| C3 | Network and determinism | Real provider calls are **explicit, bounded, and Host-observed**: `siralos_adapters::provider::{openai,anthropic}` (or a registry) implements `ModelProvider` over `reqwest`/`hyper` with the `siralos_core::determinism` clock, strict timeouts, and a bounded transcript. Responses are recorded via `siralos_core::determinism`/`identity` for `determinism-replay` style replay; a live call without recording is a typed `unavailable` for replay. No hidden auto-retry beyond the bounded `tool-loop` budget. |
| C4 | Tool calling | Model-requested tool calls are validated against the Host's `ToolRegistry` (allow/ask/deny) before any `Tool::execute`; the provider never decides authority. `displayInput` truncation stays UTF-16 code-unit exact. Provider output remains untrusted data until Host validation. |
| C5 | Verification | Each provider slice contributes a frozen differential subject over pure seams (e.g., `provider-openai` ×4, `provider-anthropic` ×4) or `check:differential` expectations per decision 40 C7 (pinned v32 oracle untouched); `cargo test --workspace --all-features` + `npm run check` stay green; no new `unsafe`, no spawn beyond the typed `ModelProvider` call, no secret in logs. Corpus bump `v52 → v53+` with strict-loader asserts together (map Notes). |
| C6 | Lean guardrails & ordering | No model-router, no automatic plugin acquisition, no generic Memory, no GUI/TUI, no marketplace (ADR 0036 §51). Ordered slices after PASS: (1) research (66) → (2) this entry review (67) → (3) `ProfileRecord` provider/model fields + `siralos.toml` parsing → (4) credential resolution + `siralos_adapters::provider` registry + `ModelProvider` HTTP adapters (one provider at a time). Each slice entry-reviewed, one per session, budget one coherent pass + up to two repairs. |

## HITL grilling — frontier questions (answer in session, then freeze C1–C6)

1. **Scope** — add `provider`/`model` to `ProfileRecord` as bounded optional fields that feed `EffectiveRunPolicy`, no model-router?
2. **Credentials** — approve `env:` references only, never in `siralos.toml`/`siralos.lock`/`Context`/`logs`?
3. **Network** — approve Host-observed, bounded provider calls with `determinism` clock and replay recording, no hidden retry?
4. **Tools** — approve Host `ToolRegistry` gate before any tool execute, provider output stays untrusted?
5. **Ordering** — approve the 4-step ordered slice sequence, lean guardrails held?

## Acceptance

PASS when 5 answers recorded verbatim and C1–C6 frozen in `decisions/67-*`. That **authorizes only** the `ProfileRecord` provider/model fields + `siralos.toml` parsing slice; the credential + HTTP registry slice remains frozen but not authorized until that slice is Verified (gates green, no secret in portable config). `Out of scope` stays closed.

## Resolution

Closed — HITL 2026-08-31: Q1 Add to ProfileRecord / Q2 Env only / Q3 Bounded Host-observed / Q4 Host gate / Q5 Approve ordering — C1–C6 frozen in [decisions/67-real-model-provider-entry-review.md](../decisions/67-real-model-provider-entry-review.md) — **PASS; ProfileRecord provider/model fields + siralos.toml parsing authorized as next implementation slice.** This unblocks [68 — Credentials & Registry](../tickets/68-real-provider-credentials-and-registry.md) — frontier now includes 68.

Blocked by: 66-real-model-provider-research.md
