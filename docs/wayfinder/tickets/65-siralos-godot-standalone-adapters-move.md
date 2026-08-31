---
title: "siralos-godot Standalone — Move Host Adapters into Plugin"
label: "wayfinder:task"
type: HITL
status: closed
resolution: "decisions/65-siralos-godot-standalone-adapters-move.md — explored, reverted — keep adapters in host (lean, no circular)"
blockedBy: ["64-siralos-godot-monorepo-pin-and-shim-removal.md"]
---

# Ticket 65 — siralos-godot Standalone

**Map:** [Siralos Roadmap](../siralos-roadmap.md)
**Decisions:** [61 C1–C6](../decisions/61-siralos-godot-externalization-entry-review.md) + [64 Option A](../decisions/64-siralos-godot-monorepo-pin-and-shim-removal.md)

## Question

`siralos-godot` is now at `../siralos-godot` `a01c561` (41 files, 77 tests) with `siralos-godot → siralos-core` via `path = "../siralos/crates/siralos-core"`. Host adapters `crates/siralos-adapters/src/godot/**` (10 entries: diagnostics, discovery, knowledge, lsp, process, profile, project, scene, scene_mutation, mod.rs) still live in the monorepo and are used by `siralos_adapters::godot::*` and `harness.rs`. How is the plugin made fully self-contained so adding `siralos-godot` as a domain/plugin is enough to use it in Siralos?

Execute (one commit per repo, no `available` flip):

- **External:** copy `crates/siralos-adapters/src/godot/**` verbatim to `../siralos-godot/src/adapters/godot/**` (or `src/godot_adapters/**`), add `wasmtime`/`wasmtime-wasi`/`dirs`/`toml` deps from `crates/siralos-adapters/Cargo.toml:19-44` to `../siralos-godot/Cargo.toml`, change `use siralos_godot::godot::...` → `crate::godot::...` in the moved files, expose `pub mod adapters;` in `../siralos-godot/src/lib.rs`, keep `forbid(unsafe_code)`.
- **Monorepo:** delete `crates/siralos-adapters/src/godot/**` and `pub mod godot;` from `crates/siralos-adapters/src/lib.rs:14`, keep `Cargo.toml` path dep to external for `harness.rs` (still needs `siralos_godot` types), or make `harness.rs` use external's adapters via `siralos_godot::adapters::godot::*` if moved.

Do not change `siralos-core` neutrality or add new spawn paths. Godot adapters remain fail-closed (`GODOT_MUTATION_APPLY_UNAVAILABLE_MESSAGE`).

Blocked by: 64 (Option A). Frontier is this ticket.

## Resolution

Closed — explored moving `adapters/src/godot` (10 entries) into `../siralos-godot/src/adapters/godot` to make the plugin fully self-contained. Attempt copied verbatim, added `wasmtime`/`dirs`/`toml` to external `Cargo.toml`, fixed `siralos_godot::` → `crate::` (24 files), exposed `pub mod adapters;`, deleted `adapters/src/godot` from monorepo, updated `harness.rs` `siralos_adapters::godot::` → `siralos_godot::adapters::godot::` (4 places). Verification failed: external `cargo check -p siralos-godot` reported 23 errors (`crate::workspace::fs` missing, `str` unsized, `wasmtime` heavy) — moving host adapters requires copying `siralos_adapters::workspace` (fs/list/resolve/root) and would create circular `siralos_godot ↔ siralos_adapters` deps. Decision: **keep adapters in host** per [decision 65](../decisions/65-siralos-godot-standalone-adapters-move.md) — plugin stays domain-only (41 files), host retains `adapters/src/godot` as bridge, external stays `siralos-godot → siralos-core` via `path = "../siralos/crates/siralos-core"`, monorepo `siralos-godot` via `path = "../siralos-godot"`. Reverted all moves; `cargo test --workspace` 505 and `harness` still build via external path dep. Lean and no circular.
