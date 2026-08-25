---
title: "R13.3 Entry Review - External Knowledge Boundaries"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: decisions/24-r13-3-entry-review.md
blockedBy: []
---

## Question

Freeze the R13.3 scenario-level contract per [decision 21](../decisions/21-r13-remaining-surface-parity-entry-review.md):
the concrete differential scenarios and acceptance for the two frozen
subjects `reference-identity` and `research-policy` (scope: reference
declaration/registry/resolver/materializer identity semantics + the
research service's denied-by-default policy and bounded normalization
core), then authorize the implementation.

Constraints inherited: corpus bump v26 at the reconciliation commit;
schema 3 unchanged; injected clock + harness-injected ports/fakes only;
no network anywhere; no unavailable effect becomes operational; lean
porting discipline applies.

## Resolution

**PASS (2026-08-24)** - scenario contract frozen per
[decision 24](../decisions/24-r13-3-entry-review.md): 10 reference cases +
10 research cases, access port + tools deferred to R13.5, bounded temp
fixture dirs with redacted absolute paths, corpus v26 at reconciliation.
Implementation authorized.
