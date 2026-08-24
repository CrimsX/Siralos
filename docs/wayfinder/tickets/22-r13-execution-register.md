---
title: "R13 Execution Register - Remaining Surface Parity Slices"
label: "wayfinder:repair"
type: HITL
status: open
resolution: null
blockedBy: []
---

## Purpose

Tracks the R13 slice landings frozen by
[decision 21](../decisions/21-r13-remaining-surface-parity-entry-review.md)
and gates the [R12 Disposition Execution](20-r12-disposition.md).

| Slice                                       | Subjects                                                       | Entry review | Landed at |
| ------------------------------------------- | -------------------------------------------------------------- | ------------ | --------- |
| R13.1 Host introspection & authority        | `security-permissions`, `command-catalog`, `capability-doctor` | pending      | â€”       |
| R13.2 Workspace guidance                    | `instructions-resolution`, `knowledge-revisions`               | pending      | â€”       |
| R13.3 External knowledge boundaries         | `reference-identity`, `research-policy`                        | pending      | â€”       |
| R13.4 Planning & briefing                   | `planning-runtime`, `executor-brief`                           | pending      | â€”       |
| R13.5 CLI product composition (full parity) | `cli-session`                                                  | pending      | â€”       |

## Definition of done

All five slices landed with differential parity at their corpus bumps and
the full gate observed on the final assembled tree; then [R13 Verified
Promotion] proceeds (seven-surface advancement), which unblocks the R12
disposition. Fail-closed postures never flip to operational.
