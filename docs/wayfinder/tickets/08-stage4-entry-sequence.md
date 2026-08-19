---
title: "Stage 4 Entry — Generic Controlled Runtime vs Godot Adapter Layering"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: decisions/08-stage4-entry-sequence.md
blockedBy: ["07-r12-disposition.md"]
---

## Question

Decide the **Stage 4 entry sequence** after R12, settling the generic-vs-Godot layering dispute.

Resolve:

- Generic **Controlled Runtime Execution** (host-authorized bounded process supervision producing structured runtime evidence, no unrestricted desktop/network) must come before which Godot specializations (Godot Runtime Adapter, Visual Evidence, Controlled Interaction, QA Workflows, Profiling).
- The exact entry gate (`docs/development/stage4-entry-gate.md`) checks that must pass after R12 and which lean product sentence from ADR 0036 governs the ordering.
- Confirm that Stages 4–6 remain staged product direction subject to evidence, not guaranteed commitments, so this map does not promise delivery.

Decision, not Stage 4 work.

Blocked by: `07-r12-disposition.md`.

## Resolution

Closed — decision recorded in [decisions/08-stage4-entry-sequence.md](../decisions/08-stage4-entry-sequence.md). Generic 4.1-4.2 Host boundary before Godot 4.3+ specialization; 4-arrow ordered gate (R1-R11 and Stage 1-3 audit and R12 and 17-criteria gate); Stages 4-6 remain staged product direction, not guarantees. This was blockedBy 07, which closed before this close. This was the final map ticket — Wayfinder Destination is reached (decision/thin spec, not implementation).
