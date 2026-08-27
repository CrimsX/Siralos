# Decision — Godot Crate Extraction Entry Review — Freeze the Source Move

**Wayfinder ticket:** [Godot Crate Extraction Entry Review](../tickets/37-godot-crate-extraction-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [Stage 4.1 Verified Promotion](36-stage4-1-verified-promotion.md) (PASS, `9383ab8` / executable `72e20be`) + [Stage 4.1 + Godot Extraction Contract](34-stage4-1-generic-runtime-and-godot-plugin-extraction.md) (PASS, shim `0996b38` now exists as `siralos-godot → siralos-core`)
**Decided:** 2026-08-27 (resolver session, interactive HITL grilling over `crates/siralos-core/src/godot/**` 6+3 surfaces, `crates/siralos-adapters/src/godot/**` adapters, `scripts/check-rust-architecture.mjs` `EXPECTED_CRATES`/`ALLOWED_DEPENDENCIES`, and `Cargo.toml:3` workspace `resolver = "3"`)
**Status:** **PASS — Godot crate extraction frozen; authorized as next implementation slice**
**Self-loop ledger:** 4 criteria, one decision pass (verification below)

> Mirrors `decisions/14-r10-entry-review.md` and `35-stage4-1-entry-review.md` (one milestone, entry-reviewed slice). No file is moved here — this freezes what moves, how `check:rust` is relaxed, and what stays fail-closed.

---

## Summary

The 6+3 Godot domain moves out of the host into `crates/siralos-godot` **as a pure file move under parity** (ADR 0032 `behavioral parity != structural parity`). No observable changes, no new differential subject, no `available` flip, no marketplace, no external repo. The Rust gate stays green (`check:differential` 231/231 v31 retained) and `godot` is still `NOT INSTALLED` by default (`stage4-entry-gate.md:33`).

## 1. Frozen move scope

| Crate              | From                                                                                                                                                                                                                                                                                                                                                                                   | To                                                                                                                                                                  | Rule                                                                                                                                                                                               |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `siralos-core`     | `src/godot/**` (23 files: `mod.rs` + `api.rs`, `capabilities.rs`, `compatibility.rs`, `diagnostics.rs`, `digest.rs`, `engine_profile.rs`, `events.rs`, `gdscript.rs`, `inspector.rs`, `installations.rs`, `knowledge.rs`, `limits.rs`, `lsp.rs`, `probe.rs`, `probes.rs`, `project.rs`, `selection.rs`, `version.rs` + `development/**`, `impact/**`, `scene/**`, `scene_mutation/**`) | `crates/siralos-godot/src/godot/**` (same tree)                                                                                                                     | Move verbatim; `pub mod godot;` removed from `siralos-core/src/lib.rs:20`; `crates/siralos-godot/src/lib.rs` replaces shim `pub use siralos_core::godot::*` with `pub mod godot; pub use godot::*` |
| `siralos-adapters` | `src/godot/**` (10 entries: `diagnostics/`, `discovery/`, `knowledge/`, `lsp/`, `process/`, `profile/`, `project/`, `scene/`, `scene_mutation.rs`, `mod.rs`)                                                                                                                                                                                                                           | `crates/siralos-godot/src/adapters/**` (or `src/godot_adapters/**` if `adapters` name collides) + thin re-export shim `siralos-adapters/src/godot` during migration | Move verbatim; `siralos-godot/Cargo.toml` gains `siralos-adapters`-needed deps only if probes need them, otherwise stays `siralos-godot → siralos-core` only                                       |
| Consumers          | `siralos-core` imports of `crate::godot` inside moved files → `crate::godot` inside new crate                                                                                                                                                                                                                                                                                          | Once sources are in `siralos-godot`, the crate's `lib.rs` owns them; no consumer changes beyond `Cargo.toml` dependency additions                                   | `cargo check --workspace` must stay green with no `siralos-core`→`siralos-godot` edge                                                                                                              |

**Not moved:** `siralos-core/src/language/**` (generic, language-neutral), `siralos-core/src/identity/**` digests (used by runtime), TypeScript GDScript scanner (reference only), `wit/domain-abi.wit` (`siralos:domain-abi@1.0.0` stays in `siralos-adapters`), differential corpus `godot-*` subjects (no corpus bump; move is refactoring under parity).

## 2. Workspace & check:rust relaxations

- `Cargo.toml:3` `members = ["crates/siralos-core", "crates/siralos-adapters", "crates/siralos-godot", "crates/siralos-cli"]` — already landed in `0996b38`.
- `scripts/check-rust-architecture.mjs:27` `EXPECTED_CRATES` already includes `crates/siralos-godot` (landed `0996b38`).
- `ALLOWED_DEPENDENCIES` (`siralos-godot → siralos-core` only) stays; the `FORBIDDEN_CORE_SYMBOL_PATTERN` (`godot|gdscript|…`) exemption for `crates/siralos-core/src/godot/**` is **removed** once that subtree no longer exists — core returns to full domain-neutrality and the check passes by the directory being absent, not by an allow-list.
- `siralos-godot` keeps `forbid(unsafe_code)`, `edition.workspace = true` (2024), `lints.workspace = true`, `publish = false`.

## 3. Boundaries — not in this slice

- No `Add Plugin` UI (`/domains` + `/domains-add`) — still frozen per decision 34 §2 UI contract (`siralos.toml` digest pin, `Enable`/`Activate` Host-gated); not this slice
- No external `github.com/CrimsX/siralos-godot` repo — `FUTURE` until `siralos.toml → siralos.lock` (Stage 5)
- No new capability, no `available` flip (`scene_mutation::apply` stays `GODOT_MUTATION_APPLY_UNAVAILABLE_MESSAGE`)
- No marketplace, no auto-acquisition, no `path =` hack beyond local dev

## 4. Authorization

**Godot crate extraction is authorized as the next implementation slice** against this frozen file list. The `Add Plugin` empty-state `Domains` view remains **frozen but not authorized** until this extraction is `Verified` (differential 231/231 retained, `check:rust` + `check:architecture` + `cargo test` green).

---

## Self-loop verification

| Criterion                                           | Direct evidence                                                                                                                                                                     | Status |
| --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Move list enumerates exactly 6+3 surfaces           | §1 table lists 23+10 files matching `04-r8-r9-cut.md` 6+3 (discovery/profiling, recovery, knowledge, diagnostics, LSP, scene/resource + review/impact, scene_mutation, development) | pass   |
| No new observable / no new scenario                 | `Differential parity` row: `231/231` v31 retained; `e24f4bb` unchanged; corpus `godot-*` subjects already at parity                                                                 | pass   |
| check:rust stays green without expanding exemptions | §2: `0996b38` already added `crates/siralos-godot` to `EXPECTED_CRATES`/`ALLOWED_DEPENDENCIES`; after move the Godot exemption is _removed_, not widened                            | pass   |
| Human decided the material cut                      | HITL answers 2026-08-27: `Move exactly 6+3` + `local crate first` (decision 34) — re-affirmed for this entry review                                                                 | pass   |
