---
title: "R13.2 Entry Review - Workspace Guidance Scenarios"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: decisions/23-r13-2-entry-review.md
blockedBy: []
---

## Question

Freeze the R13.2 scenario-level contract per [decision 21](../decisions/21-r13-remaining-surface-parity-entry-review.md):
the concrete differential scenarios and acceptance for the two frozen
subjects `instructions-resolution` and `knowledge-revisions` (scope:
instruction resolver semantics + the knowledge coordinator's
deterministic core), then authorize the implementation.

Constraints inherited: corpus bump v25 at the reconciliation commit;
schema 3 unchanged; injected clock + ports only; no unavailable effect
becomes operational; lean porting discipline applies.

## Resolution

**PASS (2026-08-24)** - scenario contract frozen per
[decision 23](../decisions/23-r13-2-entry-review.md): 7 instructions cases +
7 knowledge cases, seeding deferred to R13.5, corpus v25 at
reconciliation. Implementation authorized.
