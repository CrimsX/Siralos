---
title: "Remaining-Surface Ports Entry Review - Freeze the Continuation Contract"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: decisions/21-r13-remaining-surface-parity-entry-review.md
blockedBy: []
---

## Question

Freeze the Stage 3R continuation contract opened by the deferred R12
verdict (2026-08-24): the ordered slicing, per-slice differential
subjects, and corpus progression for porting the remaining TypeScript
Stage-3 surfaces to the Rust candidate, and authorize the first
implementation slice.

Surfaces to place on the sequence (from the [R12 Disposition Execution](20-r12-disposition.md)
inventory): project instructions + knowledge; references + research;
host-routed planning; executor briefing (ADR 0022-23); self-reference +
capability doctor; command catalog + security policy surface; approval
flow; the interactive CLI product layer (`apps/cli`).

Constraints inherited from the repo discipline: every slice gets an
entry review before code ([R8 Entry Review] pattern); corpus bumps land
at slice reconciliation commits ([R10 Entry Review] pattern);
fail-closed postures never flip to operational; lean composition rules
(ADR 0036) bound what gets ported vs redesigned. This ticket blocks
[R12 Disposition Execution](20-r12-disposition.md).

## Resolution

**PASS (2026-08-24)** - R13 contract frozen per [decision 21](../decisions/21-r13-remaining-surface-parity-entry-review.md): five ordered slices, nine frozen subjects, corpus bumps at reconciliation commits, decision-07 predecessor list amended to R1-R11 + R13, R13.5 full CLI parity. R13.1 authorized; landings tracked in the execution register.
