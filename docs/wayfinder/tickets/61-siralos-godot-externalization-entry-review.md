---
title: "siralos-godot Externalization Entry Review — Freeze the Standalone Repo Contract"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: "decisions/61-siralos-godot-externalization-entry-review.md"
blockedBy: ["60-siralos-godot-externalization-research.md"]
---

# Ticket 61 — siralos-godot Externalization Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Decision:** will open as `docs/wayfinder/decisions/61-siralos-godot-externalization-entry-review.md` after HITL PASS

## Question

Stage 6 is Verified at `e2c3540` (four slices, differential 315/315 at v52/320, 81 expectations, pinned v32 oracle untouched). `crates/siralos-godot` holds the 6+3 R8/R9 surfaces plus the Stage 4.3 `runtime_adapter` specialization, in-repo behind `siralos:domain-abi@1.0.0` per decisions 34/37. The lean contract froze `github.com/CrimsX/siralos-godot` as **FUTURE / NOT DUE** until `siralos.toml → siralos.lock` (Stage 5.4 `0a6d592`) + `cargo deny` pinning was demonstrated — now both are Verified (Stage 5 at `c2c30f0`). No ticket exists to freeze how that in-repo crate becomes a standalone repo. How is the externalization frozen so an executor can implement it without rediscovering scope, authority, parity, or distribution boundaries?

> Wayfinder discipline: Plan, don't do. This ticket freezes the decision contract; implementation follows only after HITL PASS (one ticket per session). No file is moved here.

## Why now

- **Preconditions met:** `siralos-core` is domain-neutral again (decision 37, `FORBIDDEN_CORE_SYMBOL_PATTERN` exemption removed), `/domains` + `/domains-add` + `/domains-enable` + `/domains-activate` are Host-gated (decisions 38/39), `siralos.lock` is deterministic/idempotent (5.4), and the Stage 4–6 differential is 315/315 with zero spawn paths.
- **No open frontier tickets:** `01`–`59` closed; `60` research is the only frontier — the map's `Not yet specified` for this effort cannot graduate until the C1–C6 contract below is human-approved.
- **Without a frozen contract, the move risks:** inventing a new package ecosystem, weakening digest pinning, breaking `godot-*` parity, or pulling Out-of-scope items (marketplace, auto-acquisition per ADR 0036 §35-36).

## Contract draft (C1–C6) — draft for HITL confirmation

| # | Clause | Draft |
|---|--------|-------|
| C1 | Scope — what moves | **Move `crates/siralos-godot` verbatim** (all `src/godot/**` + `src/adapters/**`/`src/godot_adapters/**` if present, `Cargo.toml` with `siralos-godot → siralos-core` only, `forbid(unsafe_code)`, `edition 2024`). No new surface, no `available` flip (`scene_mutation::apply` etc. stay `UNAVAILABLE`), zero new spawn paths. `siralos-core::language` / `siralos-core::domain` / `wit/domain-abi.wit` stay in monorepo. The move is refactoring under parity per ADR 0032 `behavioral parity != structural parity`. |
| C2 | Standalone repo shape | New repo `github.com/CrimsX/siralos-godot` — own `Cargo.toml` workspace (`resolver = "3"`), same `rust-toolchain.toml`/`rustfmt.toml`, `README.md`/`CONTRIBUTING.md`, `LICENSE` (same as Siralos — currently unpublished, record explicitly), `siralos:domain-abi@1.0.0` boundary preserved as documented interface (WIT file remains canonical in monorepo until unification decision per ADR 0036 §32). Repo is `publish=false` initially; crates.io publish is NOT part of this slice. |
| C3 | Distribution & pinning | Monorepo consumes the external crate via **cargo git dependency pinned to exact commit SHA** (e.g., `siralos-godot = { git = "https://github.com/CrimsX/siralos-godot", rev = "<sha>" }`) plus `Cargo.lock` and `cargo deny` advisories/licenses/bans — never a floating `branch = "main"` or `path =` outside local dev. The workspace `siralos.toml` `[plugins.godot]` record keeps its existing `{ path, digest }` Host flow (`DomainHost::install` SHA-256 verification before `Enabled→Active`); `siralos.lock` records the resolved digest per Stage 5.4. Alternative `git submodule` and `registry` publish are explicitly rejected for this slice (lean bias, ADR 0036 §35-37). |
| C4 | Parity & evidence | Existing `godot-*` differential subjects stay at required parity. Monorepo harness continues to emit the audit (315/315 at v52+ next bump) — the external crate's behavior is covered via **digest-bound expectation records** in `tests/differential/evidence/post-freeze/expectations.json` per decision 40 C7 (additive `expectationCoverage` provenance), not by re-running the TypeScript oracle. No corpus bump in this entry-review ticket; the subsequent move slice bumps `CORPUS_VERSION` and strict-loader asserts together with manifest version (all `contract.mjs` sites + `harness.rs` per map Notes). |
| C5 | Monorepo linkage & shim | During the move, `crates/siralos-godot` in monorepo becomes a **thin re-export shim** (or is removed after verification) that forwards to the git-pinned crate — `scripts/check-rust-architecture.mjs` `EXPECTED_CRATES`/`ALLOWED_DEPENDENCIES` updated without widening `FORBIDDEN_CORE_SYMBOL_PATTERN`, `siralos-core → siralos-godot` stays forbidden, `siralos-godot → siralos-core` stays the only edge. Local dev may use `[patch.crates-io]`/`[replace]` `path =` override only with evidence, never committed as default. |
| C6 | Lean guardrails & ordering | No marketplace, no auto-acquisition, no plugin dependency graph, no general Hooks/TaskGraph/Fleet (ADR 0036 §51). Ordered slices after PASS: (1) research (60) → (2) this entry review (61) → (3) repo bootstrap + verbatim move → (4) monorepo pin + shim removal + doc/CI sweep. Each slice entry-reviewed, one per session, budget one coherent pass + up to two repairs. `npm run check` (fmt/clippy/test/check:rust/arch/differential) stays green; any new spawn path is a failure. |

## HITL grilling — frontier questions (answer in session, then freeze C1–C6)

1. **Scope** — move exactly `crates/siralos-godot` verbatim as drafted, no new surface, no behavior change?
2. **Repo identity** — approve `github.com/CrimsX/siralos-godot` as public standalone repo with same license/toolchain, `publish=false`, WIT canonical in monorepo?
3. **Pinning** — approve `cargo git rev = "<sha>" + Cargo.lock + cargo deny` (reject `branch`/`path` default and registry publish for this slice)?
4. **Evidence** — approve additive expectation-coverage (decision 40 C7) with no corpus bump in this decision, shim during migration, zero spawn preserved?
5. **Ordering** — approve the 4-step ordered slice sequence, lean guardrails held?

## Acceptance

PASS when 5 answers recorded verbatim and C1–C6 frozen in `decisions/61-*`. That **authorizes only** the repo-bootstrap + verbatim move slice; the monorepo pin slice remains frozen but not authorized until that move is Verified (differential retained, gates green). `Out of scope` stays closed.

## Resolution

Closed — HITL 2026-08-31: Q1 Approve verbatim / Q2 Approve public / Q3 Approve git rev / Q4 Approve expectations / Q5 Approve ordering — C1–C6 frozen in [decisions/61-siralos-godot-externalization-entry-review.md](../decisions/61-siralos-godot-externalization-entry-review.md) — **PASS; repo-bootstrap + verbatim move authorized as next implementation slice.** This unblocks [62 — Distribution & Migration Plan](../tickets/62-siralos-godot-distribution-and-migration-plan.md) — frontier now includes 62.

Blocked by: 60-siralos-godot-externalization-research.md
