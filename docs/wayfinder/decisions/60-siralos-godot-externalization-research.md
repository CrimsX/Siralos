---
title: "siralos-godot Externalization Research — Inventory, Shape, and Harness Implications"
label: "wayfinder:research"
status: accepted
date: 2026-08-31
ticket: "60"
supersedes: []
---

# Decision 60 — siralos-godot Externalization Research

**Ticket:** [60 — siralos-godot Externalization Research](../tickets/60-siralos-godot-externalization-research.md) · label `wayfinder:research` AFK
**Map:** [Siralos Roadmap](../siralos-roadmap.md)

> Fact sheet only — no proposal, no code moved, no corpus bump. Provides the frozen file:line evidence that [61 — Entry Review](../tickets/61-siralos-godot-externalization-entry-review.md) must consume.

## 1. Crate inventory — `crates/siralos-godot`

**Crate manifest:** `crates/siralos-godot/Cargo.toml:1-14` — `name = "siralos-godot"`, `description` notes in-repo Plugin behind `siralos:domain-abi@1.0.0`, `version.workspace = true`, `edition.workspace = true (2024)`, `publish = false`, `lints.workspace = true`, `forbid(unsafe_code)` via `src/lib.rs:8` (`#![forbid(unsafe_code)]`), dependencies `siralos-core = { workspace = true }` + `serde_json = "1"` only — direction `siralos-godot → siralos-core` only.

**Lib root:** `crates/siralos-godot/src/lib.rs:1-13` — module doc cites decisions 34/37, re-exports `pub mod godot; pub use godot::*;`, confirms extraction after `crates/siralos-core/src/godot` removal.

**Source tree (40 files + lib.rs = 41, `Get-ChildItem -Recurse`):**

| Path                                                                                                                                                                                           | Notes                                                     |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------- |
| `crates/siralos-godot/src/godot/mod.rs`                                                                                                                                                        | root re-export                                            |
| `crates/siralos-godot/src/godot/api.rs`                                                                                                                                                        | version-bound API knowledge models                        |
| `crates/siralos-godot/src/godot/capabilities.rs`                                                                                                                                               | capability ids                                            |
| `crates/siralos-godot/src/godot/compatibility.rs`                                                                                                                                              | engine compatibility                                      |
| `crates/siralos-godot/src/godot/diagnostics.rs`                                                                                                                                                | GDScript check-only diagnostics models                    |
| `crates/siralos-godot/src/godot/digest.rs`                                                                                                                                                     | domain-separated godot digest                             |
| `crates/siralos-godot/src/godot/engine_profile.rs`                                                                                                                                             | static engine profile                                     |
| `crates/siralos-godot/src/godot/events.rs`                                                                                                                                                     | Godot events                                              |
| `crates/siralos-godot/src/godot/gdscript.rs`                                                                                                                                                   | GDScript utilities                                        |
| `crates/siralos-godot/src/godot/inspector.rs`                                                                                                                                                  | project inspector                                         |
| `crates/siralos-godot/src/godot/installations.rs`                                                                                                                                              | Godot discovery/installations                             |
| `crates/siralos-godot/src/godot/knowledge.rs`                                                                                                                                                  | knowledge index                                           |
| `crates/siralos-godot/src/godot/limits.rs`                                                                                                                                                     | bounds / limits                                           |
| `crates/siralos-godot/src/godot/lsp.rs`                                                                                                                                                        | bounded LSP framing                                       |
| `crates/siralos-godot/src/godot/probe.rs` + `probes.rs`                                                                                                                                        | probe contracts                                           |
| `crates/siralos-godot/src/godot/project.rs`                                                                                                                                                    | static project intelligence                               |
| `crates/siralos-godot/src/godot/selection.rs`                                                                                                                                                  | installation selection                                    |
| `crates/siralos-godot/src/godot/version.rs`                                                                                                                                                    | version parsing                                           |
| `crates/siralos-godot/src/godot/development/mod.rs` + `disposition.rs` + `order.rs` + `surface.rs`                                                                                             | deterministic unified `/develop` core                     |
| `crates/siralos-godot/src/godot/impact/mod.rs` + `analyzer.rs` + `model.rs`                                                                                                                    | review/impact intelligence                                |
| `crates/siralos-godot/src/godot/scene/mod.rs` + `intelligence.rs` + `limits.rs` + `models.rs` + `parser.rs` + `relationship_index.rs` + `resolution.rs` + `text.rs` + `tree.rs` + `variant.rs` | bounded `.tscn`/`.tres` intelligence                      |
| `crates/siralos-godot/src/godot/scene_mutation/mod.rs` + `prepared.rs`                                                                                                                         | prepare-only mutation contracts                           |
| `crates/siralos-godot/src/godot/runtime_adapter.rs`                                                                                                                                            | `decide_godot_launch` + Godot-scoped evidence (Stage 4.3) |

Count matches `72 tests` cited in `AGENTS.md:91` and `crates/siralos-godot` doctests/unit tests — no `src/adapters` subtree; adapters live as `crates/siralos-godot/src/godot/**` models, not `siralos-adapters/src/godot` (that path no longer exists post-decision 37; check `crates/siralos-adapters/src` contains workspace/godot shims only via re-export, not full implementation).

**Workspace shape:** `Cargo.toml:3-8` — `members = ["crates/siralos-core","crates/siralos-adapters","crates/siralos-godot","crates/siralos-cli"]`, `resolver = "3"`, `exclude = ["fuzz"]`. `crates/siralos-core/src/lib.rs:14-27` — no `pub mod godot;` (removed by decision 37), confirming domain neutrality.

## 2. Boundary & lean invariants

| Invariant                                              | Enforcement                                                                                                                                                                                                        | Evidence                                                                                                                                |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------- |
| `siralos-core` must never depend on a domain           | `scripts/check-rust-architecture.mjs:63-67` `ALLOWED_DEPENDENCIES` — `siralos-core → none`, `siralos-godot → siralos-core`, `siralos-adapters → siralos-core + siralos-godot`, `siralos-cli → core+adapters+godot` | `runChecks` fails if `siralos-core` lists `siralos-godot`                                                                               |
| Godot symbols forbidden in core                        | `FORBIDDEN_CORE_SYMBOL_PATTERN = /godot                                                                                                                                                                            | gdscript                                                                                                                                | \.tscn | \.tres | project\.godot | nodepath | autoload/i` `check-rust-architecture.mjs:37-38`, checked at `341-342`with`coreOutsideGodot`guard and`runtimeExempt`/`r13AuthorityExempt` allowlist | `siralos-core/src/godot` no longer exists → check passes by absence, not allowlist (decision 37 §2) |
| Lean constitution — no marketplace/auto-acquisition    | `docs/adr/0036-lean-product-composition-and-extension-model.md:505-513` §51 Removed, `ARCHITECTURE.md: Out of scope`                                                                                               | External repo was `FUTURE` per `decisions/34-stage4-1-generic-runtime-and-godot-plugin-extraction.md:24,54` until `siralos.lock` proven |
| Domain host boundary                                   | `crates/siralos-adapters/wit/domain-abi.wit` `siralos:domain-abi@1.0.0` — sole host/guest boundary per `34:50`                                                                                                     | `siralos-godot` consumes it indirectly via `siralos-core::domain` lifecycle/capability semantics                                        |
| `forbid(unsafe_code)` + edition 2024 + `publish=false` | `Cargo.toml:15-27` lints + `crates/siralos-godot/Cargo.toml:7-13`                                                                                                                                                  | Workspace-level `forbid` enforced via `scripts/check-rust-architecture.mjs:60` `UNSAFE_PATTERN`                                         |

## 3. Differential subjects proved by this crate

**Corpus:** `tests/differential/corpus/manifest.json:3-4` `schemaVersion: 3, corpusVersion: 52`, `crates/siralos-cli/src/harness.rs:12` `CORPUS_VERSION = 52`, 320 scenario files, 315/315 applicable required at `e2c3540` with 81 expectation records in `tests/differential/evidence/post-freeze/expectations.json` and pinned v32 oracle at `tests/differential/evidence/typescript-freeze-v32/oracle.json` per decision 40 C7.

**Godot subjects (12 subjects, all in `tests/differential/corpus/godot-*.json`):**

| Subject                      | Files                                                  | Corpus version landed       |
| ---------------------------- | ------------------------------------------------------ | --------------------------- |
| `godot-discovery`            | 4 (`godot-discovery.*.json`)                           | v16 decision 10             |
| `godot-knowledge`            | 5 (`godot-knowledge.*.json`)                           | v16                         |
| `godot-diagnostics`          | 4 (`godot-diagnostics.*.json`)                         | v16                         |
| `godot-lsp`                  | 4 (`godot-lsp.*.json`)                                 | v16                         |
| `godot-scene-resolve`        | 5 (`godot-scene-resolve.*.json`)                       | v16                         |
| `godot-review-context`       | 4 (`godot-review-context.*.json`)                      | v17 decision 12             |
| `godot-mutation-prepare`     | 4 (`godot-mutation-prepare.*.json`)                    | v17                         |
| `godot-develop-plan`         | 4 (`godot-develop-plan.*.json`)                        | v17 (deterministic order)   |
| `godot-runtime-launch`       | 5 (`godot-runtime-launch.*.json`)                      | v34 decision 41 (`5bedf57`) |
| `godot-runtime-evidence`     | 4 (`godot-runtime-evidence.*.json`)                    | v34                         |
| `visual-evidence` etc.       | generic but Godot specialization via `runtime_adapter` | v35–v38                     |
| `composition-*` / `evolve-*` | not godot, but crate not involved                      | —                           |

Removing the crate without replacement breaks 150/150 (v16) + 162/162 (v17) + 243/243 (v34) → current 315/315 audit.

**Expectation coverage:** post-freeze subjects (v33+ after `5da5cde` freeze) covered by `tests/differential/evidence/post-freeze/expectations.json` (81 records at `e2c3540`), additive `expectationCoverage` provenance in audit per decision 40 C7 — mechanism to retain for external crate without live oracle.

## 4. Distribution surface today (in-repo)

- **Profile/Plugin record:** `siralos.toml` `[plugins.godot]` with `{ path, digest }` per `decisions/34-stage4-1-generic-runtime-and-godot-plugin-extraction.md:29` + decision 38/39 `DomainHost::install` boundary (`lstat` + `isFile` + `maxFileSha256Bytes` bounded read + `is_path_within` containment + SHA-256 verify before `Enabled→Active`). Verified at `7fe9b66` (49 entries) + `978ac07` (51 entries).
- **Lockfile:** `siralos-core::composition::lock::create_workspace_lock` deterministic/idempotent + `siralos-adapters::lockfile` load/write/verify over atomic `lstat`+`rename` at `ce3e7dc`/`0a6d592` (Stage 5.4). Monorepo `Cargo.lock` pins `siralos-godot` via `path =` workspace member; no `git` pin yet.
- **CLI consumers:** `crates/siralos-cli/Cargo.toml:16` `siralos-godot = { workspace = true }`, `crates/siralos-adapters/Cargo.toml:11` same — both depend on crate for Godot types. `ARCHITECTURE.md:187` `siralos-cli → siralos-adapters → siralos-godot → siralos-core`.

## 5. CI / gate retention

| Gate          | Command                                                                                                                                                                                | Current status after crate move must remain green                                                                                                                                                                             |
| ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Architecture  | `node scripts/check-rust-architecture.mjs`                                                                                                                                             | `EXPECTED_CRATES` includes `crates/siralos-godot` `mjs:29-34`; `ALLOWED_DEPENDENCIES` `mjs:63-67`; `FORBIDDEN_CORE_SYMBOL_PATTERN` `mjs:37-38` — externalization will replace member with git-pinned dep, not add placeholder |
| Differential  | `npm run check:differential` (pinned oracle `tests/differential/evidence/typescript-freeze-v32/oracle.json`, expectations `tests/differential/evidence/post-freeze/expectations.json`) | 315/315 at v52 retained                                                                                                                                                                                                       |
| Rust tests    | `cargo test --workspace --all-features` + `cargo test -p siralos-cli --lib --all-features` (strict-loader assert `harness.rs:12`)                                                      | 72 godot tests must still run (either in external repo or via git dep)                                                                                                                                                        |
| Format/clippy | `cargo fmt --all` + `clippy -- -D warnings`                                                                                                                                            | `forbid(unsafe_code)` preserved                                                                                                                                                                                               |

**Cross-repo implication:** harness will need to compile the external crate via `git = { rev = "<sha>" }` — `Cargo.lock` then carries the exact revision. No new `std::process`/`Command::new().spawn()` paths may appear; every `runtime_adapter` decision stays typed `UNAVAILABLE` under absent primitive (decisions 41–45).

## 6. What is NOT needed for externalization

- No new `godot-*` differential subject (move is refactoring under parity per ADR 0032).
- No `publish = true` / crates.io publish in this slice (lean bias, decision 34 §1).
- No marketplace, `path =` hack beyond local dev `[patch]` override, or `branch = "main"` pin.

## Self-loop verification (AFK research)

| Criterion                                 | Evidence                                                                   | Verdict |
| ----------------------------------------- | -------------------------------------------------------------------------- | ------- |
| Crate inventory enumerated with file:line | §1 table + `Cargo.toml:1-14`, `src/lib.rs:1-13`, `Get-ChildItem` 41 files  | pass    |
| Boundary/lean enforcement located         | §2 table `check-rust-architecture.mjs:29-67`, ADR 0036 §51, decision 34/37 | pass    |
| Differential subjects enumerated          | §3 corpus manifest v52 + 10 godot subjects                                 | pass    |
| Distribution surface located              | §4 `siralos.toml`/`siralos.lock`/`DomainHost::install`                     | pass    |
| Gate retention identified                 | §5 scripts/manifests                                                       | pass    |

Decision 60 is **fact sheet PASS** — provides frozen evidence for [61 Entry Review](../tickets/61-siralos-godot-externalization-entry-review.md). No code, no corpus bump, no behavior change.
