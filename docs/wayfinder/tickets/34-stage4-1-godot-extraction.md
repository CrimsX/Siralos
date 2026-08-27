---
title: "Stage 4.1 + Godot Plugin Extraction — Grilling & Contract Freeze"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: "PASS — Stage 4.1 authorized next; Godot extraction contract frozen as in-repo crate crates/siralos-godot with empty-state Add Plugin UI (decision 34)"
blockedBy: ["32-r13-verified-promotion.md", "33-r12-disposition-execution.md"]
---

## Question

Can the Godot domain be separated to a new GitHub repo `siralos-godot` now, what 6+3 surfaces move, and should the Domains UI start empty with an `Add Plugin` folder picker (Godot never auto-shown)?

## Resolution

Resolved by interactive HITL grilling on 2026-08-27 over `ARCHITECTURE.md` modular monolith, `PROJECT_CONTEXT.md` Godot absent, `ADR 0034` WIT boundary, `ADR 0036` Plugin model, and `crates/siralos-core/src/godot` 6+3 surfaces.

**Answers (8):**

1. New GitHub `siralos-godot` repo eventual — **in-repo `crates/siralos-godot` first**
2. **Stage 4.1 generic Controlled Runtime Execution first** (host `siralos-core::runtime` + `siralos-adapters::runtime` behind identity-bound handles)
3. **Move exactly 6+3 R8/R9 surfaces** (discovery/profiling, recovery, knowledge, diagnostics, LSP, scene/resource + review/impact, mutation prepare, develop core)
4. **No new marketplace, no auto-acquisition** (ADR 0036 §35-36)
5. **Add Plugin (manifest folder)** — not raw `project.godot`
6. **`siralos.toml` portable** (`[plugins.godot] path + digest`, `cargo deny` pinned)
7. **Typed fail-closed picker** (`lstat`/`is_path_within`/SHA-256 before `Enabled→Active`)
8. **Local crate first**, external repo FUTURE until `siralos.toml`→`siralos.lock` proven

Frozen contract: `34-stage4-1-generic-runtime-and-godot-plugin-extraction.md` — PASS; Stage 4.1 authorized next; crate extraction and empty-state UI frozen but not yet authorized to start.
