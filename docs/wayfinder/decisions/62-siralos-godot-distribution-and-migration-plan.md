---
title: "siralos-godot Distribution & Migration Plan — Pin, Shim, and Cutover"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "62"
supersedes: []
---

# Decision 62 — siralos-godot Distribution & Migration Plan

**Ticket:** [62 — siralos-godot Distribution & Migration Plan](../tickets/62-siralos-godot-distribution-and-migration-plan.md) · label `wayfinder:task` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md)
**Blocked by:** [61 — Entry Review](../decisions/61-siralos-godot-externalization-entry-review.md) (C1–C6 frozen PASS 2026-08-31)

> Cutover plan frozen — no code in this decision. The next implementation slices (repo bootstrap + verbatim move, then monorepo pin + shim removal + doc/CI sweep) are authorized against this plan per decision 61 C6.

## 1. Pin mechanism detail

**Canonical snippet (monorepo after cutover):**

`Cargo.toml` at workspace root `[workspace.dependencies]`:

```toml
[workspace.dependencies]
siralos-core = { path = "crates/siralos-core", version = "0.0.0" }
siralos-adapters = { path = "crates/siralos-adapters", version = "0.0.0" }
# was: siralos-godot = { path = "crates/siralos-godot", version = "0.0.0" }
siralos-godot = { git = "https://github.com/CrimsX/siralos-godot", rev = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", version = "0.0.0" }
```

- `rev` is **full 40-hex commit SHA**, never `branch = "main"` or `tag =` (determinism, supply-chain). `Cargo.lock` carries the exact checkout and must be committed atomically with the `Cargo.toml` change.
- Consumers `crates/siralos-cli/Cargo.toml:16` and `crates/siralos-adapters/Cargo.toml:11` keep `siralos-godot = { workspace = true }` — they resolve via the workspace table, no per-crate edit.
- **Local dev override** (never committed): `[patch.crates-io]` or cargo `[replace]` with `path = "../siralos-godot"` for iteration, gated by evidence (`cargo test --workspace` must pass with and without patch). The committed tree must never contain `path =` to an external checkout.
- **`cargo deny` (`deny.toml`):** enable `advisories` + `licenses` + `bans` + `sources` — `sources` must forbid unpinned `git` (`unknown-git` = deny) and allow only `https://github.com/CrimsX/siralos-godot` with `rev` (no `branch`). CI runs `cargo deny check` before `cargo test`.
- **`siralos.toml` / `siralos.lock` digest flow stays authority gate:** `DomainHost::install` (bounded `lstat` + `isFile` + `maxFileSha256Bytes` + `is_path_within` + SHA-256 verify before `Enabled→Active` per decisions 38/39) records `siralos.toml [plugins.godot] { digest = "sha256:..." }`; `siralos.lock` (Stage 5.4 `0a6d592` deterministic/idempotent `create_workspace_lock` over artifact-digest primitive) records resolved digest. The `cargo git rev` pin and the `siralos.lock` digest are **independent** — cargo pins the Rust build, `siralos.lock` pins the Host's Plugin identity; both must be updated atomically, neither implies authority.

**Why `branch = "main"` and committed `path =` are rejected:** `branch` floats and breaks `hash = exact identity` (H1) + reproducibility; committed `path` breaks portability (other machines lack the checkout) and hides the exact revision. Both weaken `siralos.lock` determinism and `cargo deny` supply-chain guarantees (ADR 0036 §12).

**Rejected alternatives for this slice:** `git submodule` (extra checkout state, `path` drift, not a cargo source) and `crates.io publish + version` (lean bias, extra release ceremony before verbatim move is proven) — both per decision 61 C3.

## 2. Shim strategy — thin `crates/siralos-godot` re-export vs immediate deletion

| Phase                               | `crates/siralos-godot` in monorepo                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | `scripts/check-rust-architecture.mjs`                                                                                                                                                                                          | Gate |
| ----------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---- |
| **A. Before move** (today)          | Full crate 41 files, member of `Cargo.toml:3` `members = ["crates/siralos-core","crates/siralos-adapters","crates/siralos-godot","crates/siralos-cli"]` `EXPECTED_CRATES:29-34` = 4, `ALLOWED_DEPENDENCIES:63-67` `siralos-godot → siralos-core`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | `cargo test --workspace --all-features` + `npm run check:differential` 315/315 v52 green                                                                                                                                       |
| **B. Shim (during verification)**   | Thin re-export: `Cargo.toml` keeps `name = "siralos-godot"` but `dependencies` becomes `siralos-godot-external = { git = "...", rev = "..." }` + `lib.rs` = `pub use siralos_godot_external::*;` (no domain logic, just forwarding). `EXPECTED_CRATES` stays 4, `ALLOWED_DEPENDENCIES` unchanged — allows `cargo check` to compile both the shim and the external checkout and prove no symbol drift.                                                                                                                                                                                                                                                                                                                                                                  | `cargo test --workspace --all-features` must pass with shim, `npm run check:differential` 315/315 retained                                                                                                                     |
| **C. Cutover (after verification)** | **Delete** `crates/siralos-godot/` directory and remove `Cargo.toml:6` member `crates/siralos-godot` (members → 3: `siralos-core`, `siralos-adapters`, `siralos-cli`), drop `EXPECTED_CRATES` entry for `crates/siralos-godot`, update `ALLOWED_DEPENDENCIES` to `siralos-adapters → siralos-core` (remove `siralos-godot` edge) and `siralos-cli → core+adapters` (drop `godot` if no longer workspace member; the external crate is now a git dep, not a member — the script checks only workspace members, so the external checkout is verified via `Cargo.lock` + `deny.toml`, not `EXPECTED_CRATES`). `FORBIDDEN_CORE_SYMBOL_PATTERN:37-38` stays enforced — `siralos-core` still must not contain `godot` symbols by virtue of directory absence, not allowlist. | `node scripts/check-rust-architecture.mjs` must pass with 3 members, `cargo test --workspace --all-features` passes (now builds external git dep), `npm run check` green (fmt/clippy/differential 315/315+ next bump retained) |

Shim exists only to give a green commit that proves the external checkout is byte-identical before the monorepo deletes its copy; the shim is removed in the same atomic doc-sweep commit that advances the 7 surfaces (below). No commit may contain both a deleted shim and a new `branch = "main"` pin.

## 3. Bootstrap checklist — `github.com/CrimsX/siralos-godot`

| Item                                                                    | Content                                                                                                                                                                                                                                                                                                                             | Evidence                                      |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------- |
| `Cargo.toml` workspace                                                  | `resolver = "3"` `members = ["crates/siralos-godot"]` equivalent single-crate workspace, `version = "0.0.0"` `edition = "2024"` `rust-version = "1.85"` `publish = false` `lints.workspace = true` `forbid(unsafe_code)` — verbatim from `crates/siralos-godot/Cargo.toml:1-14`                                                     | `cargo check --workspace` green               |
| `rust-toolchain.toml` + `rustfmt.toml`                                  | Copy verbatim from monorepo (channel pinned, `max_width = 79`)                                                                                                                                                                                                                                                                      | `scripts/check-rust-architecture.mjs:269-284` |
| `src/godot/**`                                                          | **Verbatim move** 40 files per decision 60 §1 (no edits, no new surface) + `src/lib.rs:1-13`                                                                                                                                                                                                                                        | `diff --stat` 40 files, `cargo test` 72 tests |
| `README.md` / `CONTRIBUTING.md` / `LICENSE` / `ARCHITECTURE.md` pointer | `README` states in-repo Plugin crate extracted per decisions 34/37/61, points to `siralos:domain-abi@1.0.0` canonical in monorepo (`crates/siralos-adapters/wit/domain-abi.wit`), `LICENSE` same as Siralos (currently unpublished — record as `no-license` with owner `CrimsX`, issue to publish license before crates.io publish) | doc links green                               |
| `.github/workflows/rust.yml` subset                                     | `cargo fmt --all --check` + `clippy --workspace --all-targets --all-features -- -D warnings` + `cargo test --workspace --all-features` + `cargo deny check`                                                                                                                                                                         | CI green, zero spawn paths                    |
| Tests                                                                   | `72` Godot tests preserved, run via `cargo test --workspace` in external repo                                                                                                                                                                                                                                                       | `cargo test` pass                             |
| No WIT duplication                                                      | `wit/domain-abi.wit` stays canonical in monorepo until ADR 0036 §32 unification; external repo documents boundary via pointer, not copy                                                                                                                                                                                             | lean guardrail                                |

## 4. Doc & link sweep — 7-surface atomic advance at cutover

Mirroring `decisions/33-r12-disposition-execution.md:33` + `decisions/40-typescript-archive-removal.md:46` seven-surface pattern, one commit advances atomically:

| Surface                                           | Change                                                                                                                                                            |
| ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `docs/development/PROJECT_CONTEXT.md`             | §2 Current implementation: `crates/siralos-godot` now external `github.com/CrimsX/siralos-godot` git-pinned; §9 Next frontier: Stage 7 planning complete          |
| `docs/development/RUST_MIGRATION.md`              | R1–Stage 6 rows unchanged, add Stage 7 externalization row (`rev` + verification)                                                                                 |
| `ROADMAP.md`                                      | Stage 7 externalization: `siralos-godot` now standalone git-pinned, monorepo consumers via workspace `git rev`                                                    |
| `AGENTS.md`                                       | Repository `crates/siralos-godot` line: note external `github.com/CrimsX/siralos-godot`, members = 3, external via `git rev`                                      |
| `README.md`                                       | Architecture `siralos-cli → siralos-adapters → siralos-godot → siralos-core` → `siralos-cli → siralos-adapters → siralos-core` + external `siralos-godot` git dep |
| `ARCHITECTURE.md` + `docs/architecture/README.md` | Dependency direction: external `siralos-godot → siralos-core`, monorepo no longer members-includes it; update `EXPECTED_CRATES` narrative                         |
| `scripts/check-project-context.mjs` expectations  | Update expected member count 4→3, expected crate list, deny config reference                                                                                      |

All surface edits are `search → bounded read → verification → acceptance` doc edits, not code — host-owned state/digests unaffected.

## 5. Rollback

- Every phase is a **single git commit** (bootstrap commit in external repo, shim commit in monorepo, cutover commit in monorepo). `git revert` to the shim commit restores the verbatim in-repo crate; no silent `path =` fallback.
- If `cargo test --workspace --all-features` or `npm run check:differential` fails at phase B or C, revert the cutover commit, keep the shim, and file a repair — never leave a half-pinned state (`Cargo.lock` updated but `Cargo.toml` not, or vice versa).
- Git history retains the exact `rev` SHAs — `Cargo.lock` is the source of truth for which external checkout was built.

## Self-loop verification (task planning)

| Criterion                                             | Evidence                                                                          | Verdict |
| ----------------------------------------------------- | --------------------------------------------------------------------------------- | ------- |
| Pin mechanism is deterministic & supply-chain-bound   | §1 `git rev` + `Cargo.lock` + `cargo deny` sources, reject `branch`/`path`        | pass    |
| Shim strategy preserves `check:rust` without widening | §2 phases A→B→C, `EXPECTED_CRATES` 4→3, `FORBIDDEN_CORE_SYMBOL_PATTERN` unchanged | pass    |
| Bootstrap is verbatim (no new surface, zero spawn)    | §3 40-file list per decision 60 §1, `forbid(unsafe_code)`                         | pass    |
| Doc sweep is atomic 7-surface                         | §4 table mirroring decision 33/40 pattern                                         | pass    |
| Rollback is explicit                                  | §5 single-commit revert, no silent fallback                                       | pass    |

Planning for distribution & migration is **PASS** — implementation is now decision-ready. No code, no corpus bump, no behavior change in this decision.
