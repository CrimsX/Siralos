---
title: "Real Provider Credentials and Registry — Env Resolution and Host-Mediated HTTP Adapters"
label: "wayfinder:task"
type: HITL
status: open
blockedBy: ["67-real-model-provider-entry-review.md"]
---

# Ticket 68 — Real Provider Credentials and Registry

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [67 — Entry Review](../tickets/67-real-model-provider-entry-review.md) (C1–C6 frozen, PASS)

## Question

After C1–C6 are frozen in 67, how are credentials and the provider registry implemented so a profile-declared `provider = "openai"` actually results in a bounded, Host-observed, replay-recordable `ModelProvider` call without ever storing a secret in `siralos.toml`/`siralos.lock`/`Context`?

Decide and record (no code in this ticket):

- **Credential shape:** exact `siralos.toml` syntax for `credential = "env:OPENAI_API_KEY"` (and the equivalent `~/.siralos/config.json` shape if kept), bounded key validation, `env` name allowlist, and the Host resolution that happens at `load_workspace_profile` / `load_user_configuration` time — the resolved bytes are held in memory only for the `ModelProvider` call and never written to `siralos.toml`/`siralos.lock`/`Context`/`logs`/`Cargo.lock`.
- **Registry:** `siralos_adapters::provider::registry` (or `siralos_core::provider::registry`) that maps the bounded `provider` string from `ProfileRecord`/`EffectiveRunPolicy` to a concrete `ModelProvider` (`deterministic-fake`, `openai`, `anthropic`) — unknown provider is a typed `UnknownProvider` refusal before any network call, with a truthful diagnostic that never echoes the credential.
- **HTTP adapter:** `siralos_adapters::provider::openai` (and `anthropic`) — `reqwest`/`hyper` with the `siralos_core::determinism` clock, strict connect/read timeouts, bounded transcript, and `siralos_core::identity` digest for the `determinism-replay` record. Responses are recorded via the determinism ports for replay; a non-recorded live call is a typed `unavailable` for replay. No hidden unbounded retry — the `tool-loop` budget is the only retry.
- **Secret hygiene:** which 3 surfaces must be audited to prove no secret in portable config (`siralos.toml`, `siralos.lock`, `Context` projection, `siralos lock` output, `Cargo.lock`, logs) — the `missing_docs`/`clippy` lints plus a `grep -r "env:"` sweep that would fail if a secret value ever appears.

Output: a one-page credential + registry + HTTP adapter plan with the exact `siralos.toml` snippet, the `ModelProvider` trait bound, and the 3-surface secret audit. Do not implement the adapter — implementation follows only after HITL PASS on 67 and on this plan.

Blocked by: 67-real-model-provider-entry-review.md (contract not frozen until then). This ticket is the last planning slice before the `ProfileRecord` provider/model fields implementation; the HTTP adapter lands as the next implementation ticket after both 66/67 PASS.
