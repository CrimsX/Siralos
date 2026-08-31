---
title: "siralos-godot Standalone — Keep Host Adapters in Host"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "65"
supersedes: []
---

# Decision 65 — siralos-godot Standalone (Keep Adapters in Host)

**Ticket:** [65 — siralos-godot Standalone](../tickets/65-siralos-godot-standalone-adapters-move.md) · label `wayfinder:task` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md)
**Blocked by:** [64 — Pin Option A](../decisions/64-siralos-godot-monorepo-pin-and-shim-removal.md)

> **HITL 2026-08-31 — explored making the plugin fully self-contained by moving `siralos_adapters::godot` into `siralos_godot`; reverted — keep adapters in host, plugin stays domain-only.**

## Question

`siralos-godot` at `../siralos-godot` `a01c561` (41 files, 77 tests) is domain-only (`siralos_godot::godot`), host adapters `crates/siralos-adapters/src/godot/**` (10 entries) still live in monorepo. User leaned toward a fully self-contained plugin at `../siralos-godot` so adding it as a domain/plugin is enough to use it. How to make it self-contained without breaking host control or creating a circular Cargo dep?

## Investigation (2026-08-31, one coherent pass)

**Attempt (ticket 65):**
- Copied `crates/siralos-adapters/src/godot/**` verbatim to `../siralos-godot/src/adapters/godot/**` (10 entries) + `src/adapters/mod.rs` (`pub mod godot;`)
- Added `wasmtime = "=47.0.3"` / `wasmtime-wasi = "=47.0.3"` / `dirs = "6"` / `toml = "1.1"` from `crates/siralos-adapters/Cargo.toml:19-44` to `../siralos-godot/Cargo.toml:10`
- Fixed `siralos_godot::godot::…` → `crate::godot::…` in 24 files under `src/adapters/godot/**`
- Exposed `pub mod adapters;` in `../siralos-godot/src/lib.rs:12`
- Deleted `crates/siralos-adapters/src/godot/**` and `pub mod godot;` from `crates/siralos-adapters/src/lib.rs:14` in monorepo
- Updated `crates/siralos-cli/src/harness.rs:7287,7471,7558,7626` `siralos_adapters::godot::` → `siralos_godot::adapters::godot::` (4 places)
- Removed `siralos_godot` from `crates/siralos-adapters/Cargo.toml:10`

**Verification (failed):**
- `cargo check -p siralos-godot --manifest-path ../siralos-godot/Cargo.toml` → 23 errors:
  - `crate::workspace::fs::{decode_utf8, ...}` / `crate::workspace::{list,resolve,root}` not found — moved files used `crate::workspace::…` which was `siralos_adapters::workspace::…` in monorepo; `siralos_godot` has no `workspace` module.
  - Copying `crates/siralos-adapters/src/workspace/**` (fs.rs, list.rs, resolve.rs, root.rs, etc. — 10 files, `decode_utf8` at `workspace/fs.rs:166` `-> Option<String>`) to `../siralos-godot/src/workspace` and exposing `pub mod workspace;` made the `str` vs `String` error disappear but introduced a new circular dep: `siralos_godot` (external) → `siralos_adapters::workspace` (host) and `siralos_adapters` → `siralos_godot` (external) via `path = "../siralos-godot"` → Cargo `cyclic package dependency` if both depend on each other. The alternative `siralos_godot` → `siralos_core::workspace` is wrong type (`siralos_core::workspace` is revision/path, not fs/list).
  - The `siralos_godot::adapters::godot::scene::service.rs:207,410` `let Some(content) = decode_utf8(&bytes) else` error was a symptom of the missing `workspace` module, not the `str` type itself.

**Trade-off:**
- **Keep adapters in host (current):** host retains `siralos_adapters::workspace::fs/list/resolve/root` (bounded reads, `is_path_within`, `lstat`), plugin stays pure domain (models, scene parser, `forbid(unsafe_code)`), `siralos_godot → siralos_core` only via `path = "../siralos/crates/siralos-core"`, monorepo `siralos_godot` via `path = "../siralos-godot"` — no circular, `cargo test --workspace` 505 and `cargo build --bin siralos-harness` both green, `harness.rs` still uses `siralos_adapters::godot::*` + `siralos_godot::godot::*` as before.
- **Move adapters to plugin (self-contained):** plugin becomes self-contained — adding `siralos_godot` is enough — but plugin then needs `host-effects` (filesystem, process) and must either vendor `siralos_adapters::workspace` or depend on `siralos_adapters`, creating a circular or a larger `wasmtime` dep in the plugin. Violates ADR 0036 lean bias (host stays small, plugin is domain-only until concrete need).

## Decision

**Keep host adapters in host** — `siralos_godot` at `../siralos-godot` stays domain-only (41 files, `a01c561` / `190ef6d` docs update), `crates/siralos-adapters/src/godot/**` stays in monorepo as the bridge. The plugin is *not* fully self-contained by Cargo alone; it is self-contained at the Host/plugin distribution layer (`siralos.toml [plugins.godot] { digest }` + `siralos.lock` digest, `DomainHost::install` SHA-256 gate per decisions 38/39), which is the lean model (ADR 0036). No new `host-effects` granted to the plugin in this slice.

All moves for this ticket were **reverted**: `../siralos-godot/src/adapters` + `../siralos-godot/src/workspace` deleted, `../siralos-godot/Cargo.toml` and `src/lib.rs` restored to `a01c561`, `crates/siralos-adapters/src/godot/**` and `src/lib.rs:14` restored, `crates/siralos-cli/src/harness.rs` restored to `siralos_adapters::godot::`, `crates/siralos-adapters/Cargo.toml:10` restored to `siralos_godot = { workspace = true }` (but monorepo's `Cargo.toml` keeps `siralos_godot = { path = "../siralos-godot" }` from commit `5da2cab`, so the crate is still outside via path, not in-repo). `git status` clean except ticket 65.

## Self-loop verification

| Criterion | Evidence | Verdict |
|-----------|----------|---------|
| Plugin domain stays 41 files, 77 tests, `forbid(unsafe_code)` | `../siralos-godot` at `a01c561` / `190ef6d`, `cargo metadata` shows `siralos_godot → siralos_core` + `serde_json` only (wasmtime not added) | pass |
| Host adapters stay in host, no circular | `crates/siralos-adapters/src/godot` present, `Cargo.toml` members 3, `siralos_godot` via `path = "../siralos-godot"` external, `check-rust-architecture.mjs` 3 members, `cargo check -p siralos-core` PASS | pass |
| Move was attempted and reverted cleanly | `git status --short` shows only ticket 65 untracked after `git restore` of all moved files | pass |
| No new spawn path, no `available` flip | `GODOT_MUTATION_APPLY_UNAVAILABLE_MESSAGE` retained in `crates/siralos-godot/src/godot/scene_mutation` | pass |
| Human leaning recorded, decision explicit | Ticket 65 HITL: user leaned to standalone, investigation showed circular, decision keeps host bridge per ADR 0036 | pass |

## Implementation record

_No code change retained from this ticket's attempt — all file moves were reverted. The decision is to **keep the split**: domain outside at `../siralos-godot`, adapters inside at `crates/siralos-adapters/src/godot`. Ticket 65 closed as **explored, reverted — keep adapters in host**._
