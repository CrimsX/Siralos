---
title: "Stage 4.1 Entry Review — Freeze the Generic Controlled Runtime Contract"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: "PASS — Stage 4.1 contract frozen; authorized as next implementation slice (decision 35)"
blockedBy: ["34-stage4-1-generic-runtime-and-godot-plugin-extraction.md"]
---

## Question

What is the frozen contract for Stage 4.1 generic Controlled Runtime Execution — how do the host-authorized process supervision, bounded `RuntimeEvidence`, `process.execute` capability, and identity-bound handle primitive scope, get sliced, probed, and measured?

## Resolution

Resolved by interactive HITL grilling on 2026-08-27 over `ARCHITECTURE.md` Host authority, `ADR 0035` domain-neutral boundary, `RUST_MIGRATION.md:836` 4.1→4.7, and `crates/siralos-core/src/runtime` readiness. The frozen contract — `runtime-execution` ×6 + `runtime-evidence` ×4 at corpus v32, `process.execute` only, `unavailable` when primitive absent — is recorded in [decision 35](../decisions/35-stage4-1-entry-review.md) — **PASS; Stage 4.1 is authorized as the next implementation slice.**
