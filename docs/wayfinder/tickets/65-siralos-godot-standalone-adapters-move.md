---
title: "siralos-godot Standalone — Move Host Adapters into Plugin"
label: "wayfinder:task"
type: HITL
status: closed
resolution: "decisions/65-siralos-godot-standalone-adapters-move.md — plugin self-contained at 1bf2ca3, monorepo consumes via external path 87bfd35"
blockedBy: ["64-siralos-godot-monorepo-pin-and-shim-removal.md"]
---

# Ticket 65 — siralos-godot Standalone

**Map:** [Siralos Roadmap](../siralos-roadmap.md)
**Decisions:** [61 C1–C6](../decisions/61-siralos-godot-externalization-entry-review.md) + [64 Option A](../decisions/64-siralos-godot-monorepo-pin-and-shim-removal.md)

## Question

`siralos-godot` is now at `../siralos-godot` `a01c561` (41 files, 77 tests) with `siralos-godot → siralos-core` via `path = "../siralos/crates/siralos-core"`. Host adapters `crates/siralos-adapters/src/godot/**` (10 entries: diagnostics, discovery, knowledge, lsp, process, profile, project, scene, scene_mutation, mod.rs) still live in the monorepo and are used by `siralos_adapters::godot::*` and `harness.rs`. How is the plugin made fully self-contained so adding `siralos-godot` as a domain/plugin is enough to use it in Siralos?

Execute (one commit per repo, no `available` flip):

- **External:** copy `crates/siralos-adapters/src/godot/**` verbatim to `../siralos-godot/src/adapters/godot/**`, copy the `workspace` helpers (`fs/list/resolve/root/read/search/checkpoint/effects/git`), `config.rs`, and `paths.rs` the moved code needs, rewrite imports with a case-sensitive ordered pass (adapter-internal `crate::godot` → `crate::adapters::godot` first, then domain `siralos_godot::godot` → `crate::godot`), expose `pub mod adapters; pub mod config; pub mod godot; pub mod paths; pub mod workspace;`, add `dirs = "6"` for `paths::state_dir`. Keep `siralos-godot → siralos-core` only; no wasmtime/toml needed.
- **Monorepo:** delete `crates/siralos-adapters/src/godot/**` and `pub mod godot;` from `crates/siralos-adapters/src/lib.rs`, drop `siralos-godot` from `crates/siralos-adapters/Cargo.toml` (adapters → core only), point `harness.rs` at `siralos_godot::adapters::godot::*` and build `siralos_godot::config::UserGodotConfig` for the discovery record, keep `ALLOWED_DEPENDENCIES` `siralos-cli → core+adapters+godot` over the external path dep.

Do not change `siralos-core` neutrality or add new spawn paths. Godot adapters remain fail-closed (`GODOT_MUTATION_APPLY_UNAVAILABLE_MESSAGE`).

## Resolution

Closed — **plugin is fully self-contained.** First attempt (2026-08-31) failed on a missing `workspace` module and a case-insensitive rewrite that corrupted `KnowledgeSupportState`; repaired by re-copying pristine monorepo files and applying a case-sensitive ordered rewrite (`-creplace`: `crate::godot` → `crate::adapters::godot` first, then `siralos_godot::godot` → `crate::godot`), copying `workspace/`, `config.rs`, `paths.rs` into the plugin, and adding `dirs`. External **Verified at `1bf2ca3`** (55 files: 41 domain + adapters + workspace + config + paths; `cargo check --all-targets` clean; **234 tests pass** = 77 domain + 157 adapter; clippy `-D warnings` clean; `forbid(unsafe_code)` preserved; `siralos-godot → siralos-core` only). Monorepo **Verified at `87bfd35`** (adapters godot deleted, adapters → core only, harness consumes `siralos_godot::adapters::godot::*`; workspace all-targets clean; core 505 + adapters 155 + cli 70 tests pass; harness builds; **differential 315/315 parity held**; arch check PASS; doc links PASS; strict-loader assert PASS). Zero spawn paths, nothing flips `unavailable`.
