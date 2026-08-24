---
title: "R13.1 Entry Review - Host Introspection & Authority Scenarios"
label: "wayfinder:grilling"
type: HITL
status: open
resolution: null
blockedBy: []
---

## Question

Freeze the R13.1 scenario-level contract per [decision 21](../decisions/21-r13-remaining-surface-parity-entry-review.md):
the concrete differential scenarios and acceptance for the three frozen
subjects `security-permissions`, `command-catalog`, and
`capability-doctor` (scope: command catalog/registry/runners, security
policy evaluation + behavioral-config classification, self-reference +
capability doctor, approval binding), then authorize the implementation.

Constraints inherited: corpus bump v24 at the reconciliation commit;
schema 3 unchanged; no unavailable effect becomes operational; lean
porting discipline applies.
