---
title: "R8 Entry Review — Freeze R8 Contract, Subjects, Measurement"
label: "wayfinder:grilling"
type: HITL
status: closed
blockedBy: ["02-r7-verified-promotion.md"]
---

## Question

Entry-review R8 (optional Godot Stage-2 parity) at the same bar as R7.3 Projection section 14 before any R8 code lands.

Resolve (one decision, frozen contract — the slice is not verified by this ticket):

- Which 6 R8 surfaces ship (discovery, recovery, knowledge, check-only, bounded LSP, read-only scenes) — restate the two-row table from decisions/04-r8-r9-cut.md section 1 as the frozen contract for this gate.
- Per-surface fail-closed posture the future tests/differential subject must prove (unavailable without filesystem mutation or process launch where the cited primitive is unavailable — SECURITY.md:210-228).
- Differential subject names + scenario counts per R8 subject (advisory in 04 section 1 becomes frozen at this gate) and the corpus bump (today v15 133 to vN at R8).
- Measurement plan for R8 (benchmarks only where a hot spot is measured, per RUST_STYLE.md:568-589).
- Corpus version bump + manifest regeneration mechanics and the audit mechanism that gates remediation.

This entry review does not verify any R8 subject and does not authorize R9 — it mirrors R7.3 Projection section 14 (restart after the oracle correction 4b805d4). R8 implementation begins only after this gate is PASS.

Blocked by: R7 Verified Promotion (Verified at 61fbf99 / bb72482). After this gate is PASS the next authorized work is R8 code; R9 still waits on R8 Verified.
