---
title: "Godot Crate Extraction Entry Review — Freeze the Source Move"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: "PASS — Godot crate extraction frozen; authorized as next implementation slice (decision 37)"
blockedBy: ["36-stage4-1-verified-promotion.md"]
---

## Question

What exact file list moves from `crates/siralos-core/src/godot/**` + `crates/siralos-adapters/src/godot/**` into `crates/siralos-godot` under `siralos:domain-abi@1.0.0`, how does `check:rust` stay green without widening exemptions, and what stays `unavailable`?

## Resolution

Resolved by interactive HITL grilling on 2026-08-27 over `crates/siralos-core/src/godot/**` (23 files) and `crates/siralos-adapters/src/godot/**` (10 entries) and `scripts/check-rust-architecture.mjs` (`EXPECTED_CRATES` already includes `crates/siralos-godot` at `0996b38`). The frozen move — 23+10 files verbatim into `crates/siralos-godot`, `pub mod godot` removed from `siralos-core/src/lib.rs`, `check:rust` exemption _removed_ not widened, no new scenario — is recorded in [decision 37](../decisions/37-godot-crate-extraction-entry-review.md) — **PASS; Godot crate extraction is authorized as the next implementation slice.**
