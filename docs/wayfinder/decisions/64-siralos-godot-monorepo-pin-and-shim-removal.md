---
title: "siralos-godot Monorepo Pin + Shim Removal + Doc Sweep — Option A (Shim Retained)"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "64"
supersedes: []
---

# Decision 64 — siralos-godot Monorepo Pin + Shim Removal + Doc Sweep

**Ticket:** [64 — Monorepo Pin + Shim Removal](../tickets/64-siralos-godot-monorepo-pin-and-shim-removal.md) · label `wayfinder:task` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md)
**Blocked by:** [63 — Repo Bootstrap](../tickets/63-siralos-godot-repo-bootstrap-and-verbatim-move.md) (Verified `d098926` 77 tests)

> **HITL 2026-08-31 — Option A chosen per user direction:** external repo `../siralos-godot` is **ready to move**, monorepo retains `crates/siralos-godot` shim for Cargo build. User will move the repo manually (`https://github.com/CrimsX/siralos-godot`) when deemed ready — no automatic `git rev` pin in this slice. Lean, no circular.

## Context

Bootstrap Verified at `d098926150cb73d29269584c32d107ee45f3cc09` (external `../siralos-godot` 41 files verbatim, `cargo test --workspace --all-features` **77 passed** via `path = "../siralos/crates/siralos-core"`). Direct `Cargo.toml [workspace.dependencies] siralos-godot = { git = "file://…", rev = "d098926" }` was tested conceptually and found to break:

- External's `path = "../siralos/crates/siralos-core"` resolves locally (sibling repos) but fails when cargo clones the external git dep into its cache (`../siralos` not present).
- Changing external to `git = "file:///…/siralos"` creates **circular git dep** `Siralos → siralos-godot → Siralos` (`cyclic package dependency`), which cargo forbids.

Decision 62 §2 anticipated this (shim A→B→C) but did not freeze the pin option. Ticket 64 asked to choose A/B/C.

## Decision — Option A (shim retained, lean)

**Monorepo Cargo build stays on the in-repo shim** for this slice:

- `Cargo.toml:3` `members = ["crates/siralos-core","crates/siralos-adapters","crates/siralos-godot","crates/siralos-cli"]` **unchanged** (4 members).
- `[workspace.dependencies] siralos-godot = { path = "crates/siralos-godot" }` **unchanged** — no `git rev` pin committed.
- `scripts/check-rust-architecture.mjs:29-34` `EXPECTED_CRATES` stays 4, `ALLOWED_DEPENDENCIES:63-67` `siralos-godot → siralos-core` / `siralos-adapters → core+godot` / `siralos-cli → core+adapters+godot` **unchanged** — `FORBIDDEN_CORE_SYMBOL_PATTERN` still passes by absence after decision 37.

**External repo is the distribution channel**, not a Cargo git source for the monorepo build in this slice:

- `../siralos-godot` at `d098926` is the standalone `github.com/CrimsX/siralos-godot` content (41 files, `README.md`/`LICENSE`/`ARCHITECTURE.md`/`rust-toolchain.toml`/`rustfmt.toml`, 77 tests). It is ready to be pushed to `https://github.com/CrimsX/siralos-godot` by the owner at will.
- Plugin installation stays via `siralos.toml [plugins.godot] { digest }` + `siralos.lock` + `DomainHost::install` SHA-256 gate per decisions 38/39 — the external repo's commit SHA is the `digest` that `siralos.lock` will record, not a Cargo `rev`. `cargo deny` `sources` for `siralos-godot` via git is **deferred** until Option B or C is chosen.

**Why A:** preserves `cargo test --workspace --all-features` green in both repos today, avoids vendoring `siralos-core` (B) or publishing `siralos-core` (C) before the verbatim move is proven, and respects ADR 0036 lean bias (no new package ecosystem until proven). The 77-test parity and zero spawn posture are retained in both places.

**Deferred:** `git rev` pin + `Cargo.lock` + `deny.toml:sources` + 7-surface doc sweep that removes the shim (`members 4→3`, `EXPECTED_CRATES 4→3`) is frozen per decision 62 §2 phase C but **not executed** until the owner pushes the external repo and chooses B or C (or accepts A as durable). No `branch = "main"` or committed `path =` override is introduced.

## HITL answer (2026-08-31, verbatim)

- **Q1 Pin strategy** — answer: _"the goal is to move siralos-godot into its own repo once you deem that it is ready i will move it myself"_ — interpreted as **Option A (shim retained, external ready to move, no automatic git pin)** — monorepo build stays on shim, external is distribution channel, manual `git push` when owner deems ready.

## Verification

| Criterion                               | Evidence                                                                                                                                             | Verdict |
| --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| External repo is verbatim and green     | `../siralos-godot` `d098926` 41 files, `cargo test --workspace --all-features` 77 passed (monorepo `cargo test -p siralos-godot` also 77)            | pass    |
| Monorepo stays green with shim retained | `Cargo.toml:3-6` 4 members, `check-rust-architecture.mjs:29-67` 4 crates, `cargo test --workspace` / `node scripts/check-rust-architecture.mjs` PASS | pass    |
| No circular git dep introduced          | No `git =` dep added to `Cargo.toml` in this slice — circular avoided, `FORBIDDEN_CORE_SYMBOL_PATTERN` still passes                                  | pass    |
| Distribution channel documented         | `../siralos-godot/README.md:1` states `github.com/CrimsX/siralos-godot` origin `e2c3540` + `decisions/61 C3` digest flow via `siralos.toml/lock`     | pass    |
| Human decision recorded                 | Q1 answer verbatim                                                                                                                                   | pass    |

## Next

**Planning 60→61→62 + bootstrap 63 are Verified, pin decision 64 is Option A — `../siralos-godot` is ready to move.** Owner may `git push` `d098926` to `https://github.com/CrimsX/siralos-godot` at will; monorepo pin (`git rev` + `Cargo.lock` + `deny.toml`) and shim removal (`members 4→3`) remain authorized but not executed until B/C is chosen. Stage 7 is **ready, not yet cut over**.

## Implementation record

_No code change in monorepo in this decision — external repo bootstrap at `d098926` is the deliverable; monorepo retains shim per Option A._
