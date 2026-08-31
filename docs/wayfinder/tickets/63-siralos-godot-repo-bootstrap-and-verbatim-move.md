---
title: "siralos-godot Repo Bootstrap + Verbatim Move"
label: "wayfinder:task"
type: HITL
status: closed
resolution: "external repo at ../siralos-godot d098926150cb73d29269584c32d107ee45f3cc09, 77 tests pass"
blockedBy: ["62-siralos-godot-distribution-and-migration-plan.md"]
---

# Ticket 63 — siralos-godot Repo Bootstrap + Verbatim Move

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Decision:** [61 C1–C6](../decisions/61-siralos-godot-externalization-entry-review.md) (PASS 2026-08-31) + [62 cutover plan](../decisions/62-siralos-godot-distribution-and-migration-plan.md)
**Blocked by:** [62 — Distribution & Migration Plan](../tickets/62-siralos-godot-distribution-and-migration-plan.md) (closed)

## Question

Planning 60→61→62 is complete and `repo bootstrap + verbatim move` is authorized per decision 61 C6. How is the standalone repo `github.com/CrimsX/siralos-godot` bootstrapped and the 41-file `crates/siralos-godot` tree moved verbatim so the monorepo can pin it?

Execute **slice 3** (one coherent pass, no `available` flip, zero spawn):

- **External repo init at `../siralos-godot`:** `Cargo.toml` workspace `resolver=3` single-crate (`crates/siralos-godot` content at repo root), copy `rust-toolchain.toml`/`rustfmt.toml` verbatim, `src/godot/**` 40 files + `src/lib.rs` + `Cargo.toml` (name `siralos-godot`, `edition.workspace=true`, `publish=false`, `forbid(unsafe_code)`, `siralos-godot → siralos-core` only), `README.md`/`LICENSE`/`ARCHITECTURE.md` pointer, `.git` with initial commit (SHA becomes `rev`), `72` tests green via `cargo test --workspace`.
- **Verification before monorepo pin:** `cargo test --workspace --all-features` in external repo passes, `git log --oneline -1` SHA recorded for pin.

Do not yet modify monorepo `Cargo.toml` members or `check-rust-architecture.mjs` — that is slice 4 (`monorepo pin + shim removal + doc sweep`), frozen but not authorized until this bootstrap is Verified (`cargo test` green, SHA pinned).

## Resolution

Closed — bootstrap Verified at `d098926150cb73d29269584c32d107ee45f3cc09` (external repo `C:/Users/L7490/Desktop/Repos/siralos-godot`, `Cargo.toml` + `rust-toolchain.toml` + `rustfmt.toml` + `src/lib.rs` + `src/godot/**` 40 files verbatim, `README.md`/`LICENSE`/`ARCHITECTURE.md`/`..gitignore`, `cargo test --workspace --all-features` **77 passed** at `crates/siralos-godot` via `path = "../siralos/crates/siralos-core"` local dev). Monorepo `crates/siralos-godot` retained as shim until pin slice — no `Cargo.toml` members changed, no `check:rust` break. This unblocks slice 4.

Blocked by: 62 cutover plan (closed). Frontier is this ticket.
