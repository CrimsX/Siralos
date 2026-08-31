---
title: "siralos-godot Monorepo Pin + Shim Removal + Doc Sweep"
label: "wayfinder:task"
type: HITL
status: closed
resolution: "decisions/64-siralos-godot-monorepo-pin-and-shim-removal.md — Option A shim retained, external ready to move"
blockedBy: ["63-siralos-godot-repo-bootstrap-and-verbatim-move.md"]
---

# Ticket 64 — siralos-godot Monorepo Pin + Shim Removal + Doc Sweep

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Decisions:** [61 C1–C6](../decisions/61-siralos-godot-externalization-entry-review.md) + [62 cutover plan](../decisions/62-siralos-godot-distribution-and-migration-plan.md)
**Blocked by:** [63 — Repo Bootstrap](../tickets/63-siralos-godot-repo-bootstrap-and-verbatim-move.md) (Verified at `d098926` 77 tests)

## Question

Bootstrap is Verified at `d098926` (`../siralos-godot` 41 files, 77 tests via `path = "../siralos/crates/siralos-core"`). How is the monorepo cut over so `cargo test --workspace` + `npm run check` stay green without introducing a circular Cargo git dependency (`siralos-godot → siralos-core` via `path = "../siralos/..."` cannot be fetched from `file://` cargo cache)?

Execute **slice 4** (one coherent commit, per decision 62 §2 phase C + §4):

- **Findings from bootstrap (2026-08-31):** external `siralos-godot` currently uses `siralos-core = { path = "../siralos/crates/siralos-core" }` for local dev. A direct `siralos-godot = { git = "file:///…/siralos-godot", rev = "d098926" }` in monorepo `Cargo.toml [workspace.dependencies]` would make cargo clone `siralos-godot` into its git cache, then try to resolve `../siralos/crates/siralos-core` relative to that cache — path does not exist → `failed to load source`. Making external depend on `siralos-core` via `git = "file:///…/siralos"` creates a **circular git dep** (`Siralos → siralos-godot → Siralos`) which cargo forbids (`cyclic package dependency`). The `https://github.com/CrimsX/siralos-godot` rev pin has the same problem after push.

- **Decision to make in this ticket:** choose the pin strategy that keeps `cargo test --workspace` green locally and after `git push`:

  **Option A (shim retained, lean):** keep `crates/siralos-godot` as in-repo member for Cargo build (`path = "crates/siralos-godot"`), and treat `github.com/CrimsX/siralos-godot` as **Plugin distribution channel only** (via `siralos.toml [plugins.godot] { digest }` + `siralos.lock`), not as Cargo git dep. Monorepo `Cargo.toml` members stay 4, `EXPECTED_CRATES` stays 4, `ALLOWED_DEPENDENCIES` unchanged. Doc sweep updates `README.md`/`ARCHITECTURE.md` to note external repo is distribution channel, not Cargo source. This preserves the `siralos-godot → siralos-core` local path and avoids circular.

  **Option B (vendored siralos-core):** copy `crates/siralos-core` into `siralos-godot` repo (vendor), make external `siralos-godot` self-contained, remove `path = "../siralos/..."` — external no longer depends on monorepo. Monorepo then pins via `git = "https://github.com/CrimsX/siralos-godot", rev = "d098926"` and `siralos-core` types are duplicated (risk of type mismatch `siralos_core::…` vs vendored copy).

  **Option C (publish siralos-core):** publish `siralos-core` to crates.io (or private registry) at `0.0.0`, external depends on `siralos-core = { version = "0.0.0" }`, monorepo depends on external via `git rev` — no circular, but requires publish before pin.

- **Required output (no code in this ticket):** one-page decision with chosen option, exact `Cargo.toml` diff, `check-rust-architecture.mjs` diff, `deny.toml` sources diff, and 7-surface doc-sweep table. Implementation follows only after HITL PASS on this option.

Blocked by: 63 bootstrap Verified. Frontier is this ticket.

## Resolution

Closed — HITL 2026-08-31: Option A (shim retained, external `../siralos-godot` at `d098926` 77 tests is ready to move, no `git rev` pin in this slice to avoid circular `Siralos → siralos-godot → Siralos`) — **PASS per decisions/64-...; planning 60→61→62 + bootstrap 63 + pin decision 64 are complete, Stage 7 is ready, monorepo retains shim until owner pushes to `https://github.com/CrimsX/siralos-godot`.** Next is manual `git push` when owner deems ready.
