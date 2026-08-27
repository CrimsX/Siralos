# Decision — Add Plugin UI Entry Review — Freeze the Empty-State Domains Slice

**Wayfinder ticket:** [Add Plugin UI Entry Review](../tickets/38-add-plugin-ui-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [Godot Crate Extraction Entry Review](37-godot-crate-extraction-entry-review.md) (PASS, extraction landed at `7f4e1ba` — `siralos-core` domain-neutral, differential 231/231 v31 retained) + [Stage 4.1 + Godot Extraction Contract](34-stage4-1-generic-runtime-and-godot-plugin-extraction.md) (PASS, UI contract §2 frozen)
**Decided:** 2026-08-27 (resolver session; interactive HITL grilling over the four open UI-contract questions)
**Status:** **PASS — Add Plugin UI slice frozen; authorized as next implementation answer**
**Self-loop ledger:** 4 criteria, one decision pass (verification below)

> Mirrors `decisions/35-stage4-1-entry-review.md` and `37-godot-crate-extraction-entry-review.md` (one arrow, entry-reviewed slice). No implementation lands here; decision 34 §3 made each arrow a separate entry-reviewed slice.

---

## Summary

The first arrow after the extraction — the empty-state `Domains` view + `Add Plugin` flow (decision 34 §2 UI contract) — is entry-reviewed and frozen as **View + Add Plugin only** (HITL answer 3 below). It is presentation and host-verified installation; no new authority, no marketplace, no external repo. `Enable`/`Activate` remain Host-gated and unfrozen for a later slice. Everything stays fail-closed: any picker failure yields a typed reason and an empty view.

## 1. Frozen UI contract (this slice)

| Item                                        | Frozen decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Command surface                             | `/domains` (empty-state view) + `/domains-add <pick-folder>` (manifest picker + install) in `siralos-cli`; registered in the core command catalog (`commands.rs`), which is the single command-vocabulary source                                                                                                                                                                                                                                                                                                    |
| Empty state                                 | `No domains installed.` plus an `Add Plugin` affordance; never a silent success, never fabricated entries                                                                                                                                                                                                                                                                                                                                                                                                           |
| Manifest file                               | `domain-manifest.toml` at the picked-folder ROOT only; TOML keys: `id` (package id), `digest` (64-hex SHA-256), `abi` (`siralos:domain-abi@1.0.0`-compatible string), `capabilities` (optional string list). Unknown top-level keys are ignored; malformed/missing required keys fail with the typed `DomainFailure` reason                                                                                                                                                                                         |
| Persistence                                 | Workspace-root `siralos.toml`; `[plugins.<id>]` table with `path = "<picked-folder relative to workspace root>"` and `digest = "sha256:<hex>"`. Write-once semantics: `add` appends/updates only the `[plugins.<id>]` table; the file is created (or the section merged) without touching other sections                                                                                                                                                                                                            |
| Add-time verification                       | The picker (1) lstat-checks the manifest as a regular non-symlink file, (2) reads it bounded (4 KiB manifest bound), (3) parses `id`/`digest`/`abi`/`capabilities` via the existing R6 validation (reusing `domain::package` parsers), (4) if the manifest names an optional relative component file, verifies the component within the picked folder exists, is a bounded regular file, no symlink, and its SHA-256 equals the manifest digest — every failure returns a typed reason and performs no installation |
| Host boundary                               | The add flow drives `DomainHost::install` (adapters) with a prepared `DomainPackage`; `Enable`/`Activate` are NOT in this slice (documented Host-gated steps, later slice). The lifecycle stays `absent`-default when no `siralos.toml` entry exists                                                                                                                                                                                                                                                                |
| Out of scope (still frozen, not authorized) | `Enable`/`Activate` UI, external `github.com/CrimsX/siralos-godot` repo (FUTURE until `siralos.lock`), marketplace/auto-acquisition, any `available` flip of Godot runners                                                                                                                                                                                                                                                                                                                                          |

## 2. Open questions resolved by HITL (2026-08-27)

| #   | Frontier question                                  | Human answer                                                                                                                    |
| --- | -------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Manifest file name and format the picker looks for | `domain-manifest.toml` at folder root, TOML `{ id, digest, abi, capabilities }`                                                 |
| 2   | Where the plugin-record `siralos.toml` lives       | Workspace root, single portable file                                                                                            |
| 3   | Slice scope                                        | View + Add Plugin only (Recommended)                                                                                            |
| 4   | Component presence at add                          | Manifest + optional relative component path; component verified at add (exists, bounded, digest-match; typed reason on failure) |

## 3. Boundaries — not in this slice

- No `Enable`/`Activate` command rendering; Host-gated authority remains unfrozen (decision 34 §2")
- No external repo, no marketplace, no auto-acquisition (ADR 0036)
- No corpus bump yet: the slice adds CLI commands whose rendering is new, so `command-catalog-snapshot` gains entries at implementation time (a corpus v32 bump at the reconciliation commit), not in this decision
- No `available` flip; all Godot runners stay fail-closed (`unavailable`)

## 4. Authorization

**The Add Plugin UI slice is authorized as the next implementation arrow** against this frozen contract. It requires, in the implementation session: manifest parser (adapters, reusing `domain::package`), workspace `siralos.toml` `[plugins]` read/merge/write, `/domains` + `/domains-add` rendering in `siralos-cli`, catalog registration, and the corpus v32 bump at the reconciliation commit.

---

## Self-loop verification

| Criterion                                            | Direct evidence                                                                               | Status |
| ---------------------------------------------------- | --------------------------------------------------------------------------------------------- | ------ |
| All four open questions answered by the human        | §2 table: manifest name/format, siralos.toml location, slice scope, component presence        | pass   |
| Frozen contract is fail-closed and presentation-only | §1: bounded reads, no symlink, typed reasons, no `Enable`/`Activate` in slice, no marketplace | pass   |
| Boundaries explicit (what stays frozen)              | §3: Enable/Activate UI, external repo, corpus bump, no available flip                         | pass   |
| No code written before the decision                  | Present state: worktree clean at `7f4e1ba`; only docs changed                                 | pass   |
