---
title: "siralos-godot Distribution & Migration Plan — Pin, Shim, and Cutover"
label: "wayfinder:task"
type: HITL
status: closed
resolution: "decisions/62-siralos-godot-distribution-and-migration-plan.md"
blockedBy: ["61-siralos-godot-externalization-entry-review.md"]
---

# Ticket 62 — siralos-godot Distribution & Migration Plan

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Depends on:** [61-siralos-godot Externalization Entry Review](./61-siralos-godot-externalization-entry-review.md) (C1–C6 frozen, PASS)

## Question

After the C1–C6 contract is frozen in 61, how is the **cutover** sequenced so the standalone repo and the monorepo never diverge and no executor has to invent the pin/shim/docs steps?

Decide and record (no code in this ticket):

- **Pin mechanism detail:** exact `Cargo.toml` snippet (`git` + `rev`), `Cargo.lock` update flow, `cargo deny` (`advisories`/`licenses`/`bans`/`sources`) configuration, and the `siralos.toml` `[plugins.godot]` → `siralos.lock` digest flow that keeps `DomainHost::install` SHA-256 verification as the authority gate. Why `branch = "main"` and committed `path =` overrides are rejected (determinism, supply-chain).
- **Shim strategy:** thin `crates/siralos-godot` re-export vs immediate deletion — which files remain during verification, how `scripts/check-rust-architecture.mjs` (`EXPECTED_CRATES`, `ALLOWED_DEPENDENCIES`, `FORBIDDEN_CORE_SYMBOL_PATTERN`) and `crates/siralos-cli` / `crates/siralos-adapters` consumers are rewired, and when the shim is removed (gate: `cargo test --workspace --all-features` + `npm run check:differential` 315/315+ retained).
- **Bootstrap checklist:** standalone repo initialization — `cargo init` workspace, `rust-toolchain.toml`/`rustfmt.toml`, `README.md`/`ARCHITECTURE.md` pointer, `LICENSE`, `wit/` reference or pointer, CI (`.github/workflows/rust.yml` subset), and retention of `72` Godot tests + `cargo clippy -- -D warnings`.
- **Doc & link sweep:** which seven surfaces advance atomically at the cutover (`PROJECT_CONTEXT.md`, `RUST_MIGRATION.md`, `ROADMAP.md`, `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/architecture/README.md`, `scripts/check-project-context.mjs` expectations) — mirroring decision 33/40 seven-surface pattern.
- **Rollback:** what stays in git history if the pin fails (revert to shim, no silent `path` fallback).

Output: a one-page cutover plan with the pin snippet, shim file list, and doc-rewiring table. Do not execute the move — implementation follows only after HIТL PASS on 61 and on this plan.

Blocked by: 61-siralos-godot-externalization-entry-review.md (contract not frozen until then). This ticket is the last planning slice before implementation; the actual `git` move + `Cargo.toml` pin lands as the next implementation ticket after both 60/61 PASS.

## Resolution

Closed — cutover plan frozen in [decisions/62-siralos-godot-distribution-and-migration-plan.md](../decisions/62-siralos-godot-distribution-and-migration-plan.md) (pin `git rev + Cargo.lock + deny`, shim A→B→C, bootstrap checklist, 7-surface doc sweep, rollback) — **PASS; planning for distribution & migration is decision-ready, no code.** Planning phase 60→61→62 is now complete; implementation (repo bootstrap + verbatim move) is authorized per decision 61 C6 and now frontier.
