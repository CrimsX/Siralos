---
title: "Real Model/Provider Research — Inventory and Secure Credential Requirements"
label: "wayfinder:research"
type: AFK
status: closed
resolution: "decisions/66-real-model-provider-research.md"
blockedBy: []
---

## Question

Survey the current provider boundary so the real model/provider entry review cannot smuggle scope or weaken host authority.

Research inside this repo only (no external API):

- **Current provider surface:** `crates/siralos-core/src/provider` (provider-neutral request/event, `deterministic-fake` at `crates/siralos-adapters/src/provider`, `ModelProvider` trait, `deterministic-fake` echo with 16-code-point chunking, `crates/siralos-cli/src/configuration.rs:18` `DEFAULT_REVIEW_PROVIDER_ID = "deterministic-fake"` and `quality.reviewProvider` validation that rejects any other value, `crates/siralos-cli/src/harness.rs` fake provider for differential).
- **Configuration surface:** `siralos.toml` `[profile]` (permissions/plugins/context/skills) vs `~/.siralos/config.json` (`quality.reviewProvider`, `references`), `siralos_core::composition::ProfileRecord` (no provider/model fields at `crates/siralos-core/src/composition.rs:60`), and `siralos_adapters::config` (user config loading, `credential = "env:OPENAI_API_KEY"` pattern per ADR 0036 §10).
- **Host authority and determinism:** `siralos_core::provider` trust boundary (unknown discriminators fail closed), `siralos_core::determinism` (clock/random/ordering), and `siralos_core::identity` (digests) — how real provider responses would be recorded for replay vs live calls.
- **Out of scope per ADR 0036:** no automatic model-router, no hidden credential storage in `siralos.toml`/`siralos.lock`, no unbounded network access.

Deliver a fact sheet with file:line pointers, not a proposal. This unblocks the entry review for real providers.

Blocked by: none (AFK frontier). Needed by: 67-real-model-provider-entry-review.

## Resolution

Closed — fact sheet recorded in [decisions/66-real-model-provider-research.md](../decisions/66-real-model-provider-research.md). Local-markdown fallback, no hosted `research/real-model-provider` branch created. This unblocks [67 — Entry Review](../tickets/67-real-model-provider-entry-review.md) — frontier now includes 67.
