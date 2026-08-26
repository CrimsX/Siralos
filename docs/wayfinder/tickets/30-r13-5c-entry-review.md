---
title: "R13.5c Entry Review - Deferred Seams (Knowledge Seeding + Reference/Research Access)"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: "PASS — R13.5c scenario set frozen; implementation authorized (decision 30)"
blockedBy:
  - 27-r13-5-entry-review.md
---

## Question

What is the frozen contract for R13.5c — how do the three deferred-seam groups (knowledge seeding via `buildGodotProjectKnowledgeCandidates`, reference access port + tools, research access port + tools) get exercised at full parity inside the existing `knowledge-revisions`, `reference-identity`, and `research-policy` subjects at corpus v30?

## Resolution

Resolved by HITL grilling on 2026-08-26 over reads of `knowledge-seeding.ts` (5 seed shapes, conservative candidates), `reference-access` and `research-access` port signatures, and the deferral notes in decisions 23/24. The frozen contract — ~10 groups at v30 (knowledge seeding ×3, reference access ×4, research access ×4) over existing subjects, injected clock/ports only — is recorded in [decision 30](../decisions/30-r13-5c-entry-review.md) — **PASS; R13.5c is authorized as the next implementation slice.**
