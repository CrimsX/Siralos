---
title: "siralos-godot Standalone — Move Host Adapters into Plugin"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "65"
supersedes: []
---

# Decision 65 — siralos-godot Standalone (Host Adapters Moved into the Plugin)

**Ticket:** [65 — siralos-godot Standalone](../tickets/65-siralos-godot-standalone-adapters-move.md) · label `wayfinder:task` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md)
**Blocked by:** [64 — Pin Option A](../decisions/64-siralos-godot-monorepo-pin-and-shim-removal.md)

> **HITL 2026-08-31 — user direction: "keep working until this is properly implemented, don't stop."** The plugin is now **fully self-contained**: host adapters, workspace helpers, config, and paths all live in `siralos-godot`; the monorepo consumes it via the external path dep. Supersedes the interim "keep adapters in host" posture recorded during the first failed attempt.

## Question

`siralos-godot` at `../siralos-godot` (41 domain files) plus host adapters `crates/siralos-adapters/src/godot/**` (10 entries) in the monorepo meant the plugin was not self-contained — adding `siralos-godot` alone was not enough to use it. How is the plugin made fully self-contained without a circular Cargo dep, without changing `siralos-core` neutrality, and without adding spawn paths?

## Why the first attempt failed, and the fix

| Problem                                                  | Cause                                                                                                                 | Fix                                                                                                                                            |
| -------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `crate::workspace::fs` not found                         | moved adapters used `crate::workspace::{fs,list,resolve,root}` which lived in `siralos-adapters`, not in the plugin   | copy `workspace/` (10 files), `config.rs`, `paths.rs` into the plugin (`config.rs` itself needs `paths` for `state_dir`)                       |
| `crate::godot::knowledgeSupportState` (lowercase)        | PowerShell `-replace` is **case-insensitive**, so the ordered module rewrites corrupted `KnowledgeSupportState`       | re-copy pristine files and rewrite with **case-sensitive** `-creplace` in a fixed order                                                        |
| ambiguous `crate::godot::X` (domain vs adapter-internal) | both domain refs (`siralos_godot::godot::X`) and adapter-internal refs (`crate::godot::X`) collapsed to the same path | ordered rewrite: **first** adapter-internal `crate::godot` → `crate::adapters::godot`, **then** domain `siralos_godot::godot` → `crate::godot` |
| circular dep risk                                        | plugin needing `siralos-adapters` while adapters needed the plugin                                                    | no edge remains: plugin carries its own copies of `workspace`/`config`/`paths`; monorepo adapters dropped the `siralos-godot` dep              |

## Final layout

```text
../siralos-godot (standalone, self-contained)          Siralos monorepo (3 members)
  src/godot/**            domain (41 files)              crates/siralos-core        (505 tests, domain-neutral)
  src/adapters/godot/**   host adapters (10 entries)     crates/siralos-adapters    (155 tests, -> core only)
  src/workspace/**        fs/list/resolve/root/...       crates/siralos-cli         (70 tests, -> core+adapters+godot)
  src/config.rs           UserGodotConfig                [workspace.dependencies]
  src/paths.rs            state_dir (dirs)                 siralos-godot = { path = "../siralos-godot" }
  src/lib.rs              adapters, config, godot,
                          paths, workspace             harness.rs -> siralos_godot::adapters::godot::*
```

`DomainHost::install` (`siralos.toml [plugins.godot] { digest }` + `siralos.lock`, decisions 38/39) remains the Host authority gate. Every runner stays fail-closed (`GODOT_MUTATION_APPLY_UNAVAILABLE_MESSAGE`); zero spawn paths; `forbid(unsafe_code)` preserved; `missing_docs`/clippy `-D warnings` clean.

## Verification (all observed PASS, 2026-08-31)

| Gate                                                                           | Result                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------- |
| External `cargo check -p siralos-godot --all-targets`                          | Finished clean                                                            |
| External `cargo test`                                                          | **234 passed** (77 domain + 157 adapter)                                  |
| External `cargo clippy --all-targets -- -D warnings`                           | Finished clean                                                            |
| Monorepo `cargo check --workspace --all-targets`                               | Finished clean                                                            |
| Monorepo `cargo test --workspace`                                              | core **505**, adapters **155**, cli **70** — 0 failures                   |
| Monorepo harness build (`--features differential-harness`)                     | Finished                                                                  |
| **`npm run check:differential`**                                               | **315/315 applicable required parity held** (pinned v32 oracle untouched) |
| `node scripts/check-rust-architecture.mjs`                                     | passed (3 members, adapters → core only)                                  |
| `cargo test -p siralos-cli --lib --all-features` (strict-loader corpus assert) | 70 passed                                                                 |
| `node scripts/check-doc-links.mjs`                                             | passed                                                                    |

Commits: external **`1bf2ca3`** (`feat(godot): make plugin self-contained — move host adapters into siralos-godot`, 55 files), monorepo **`87bfd35`** (`refactor(godot): consume host adapters from external siralos-godot`, 44 files).

## Self-loop verification

| Criterion                                | Evidence                                                                                                                                                             | Verdict |
| ---------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| Plugin self-contained, no circular dep   | `siralos-godot` deps: `siralos-core` (path) + `serde_json` + `dirs` only; monorepo adapters dropped `siralos-godot`; only `siralos-cli → siralos-godot` edge remains | pass    |
| Behavioral parity preserved              | differential **315/315** after the move (verbatim files, only import paths rewritten)                                                                                | pass    |
| Case-corruption repaired                 | pristine re-copy + `-creplace` ordered rewrite; `KnowledgeSupportState` casing restored; external compiles with zero errors                                          | pass    |
| Lean posture: no wasmtime/toml in plugin | plugin `Cargo.toml` deps = core + serde_json + dirs; `missing_docs` deny clean                                                                                       | pass    |
| No new authority/spawn                   | fail-closed messages retained; zero `std::process` additions; nothing flips `unavailable`                                                                            | pass    |
| Human direction recorded                 | HITL: "im leaning towards having the plugin completely standalone" + "keep working until this is properly implemented dont stop"                                     | pass    |

Stage 7 externalization is **complete**: domain + adapters + helpers all in `../siralos-godot` (`1bf2ca3`), monorepo consumes via external path dep (`87bfd35`), all gates green, parity held.
