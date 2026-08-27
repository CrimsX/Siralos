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

| Slice                                | Subjects                                                       | Entry review                                              | Landed at                                             |
| ------------------------------------ | -------------------------------------------------------------- | --------------------------------------------------------- | ----------------------------------------------------- |
| R13.1 Host introspection & authority | `security-permissions`, `command-catalog`, `capability-doctor` | [decision 22](../decisions/22-r13-1-entry-review.md) PASS | `e2ee231` (corpus v24, parity held 220/220 locally)   |
| R13.2 Workspace guidance             | `instructions-resolution`, `knowledge-revisions`               | [decision 23](../decisions/23-r13-2-entry-review.md) PASS | `d682e62` (corpus v25, parity held 222/222 locally) — |
| R13.3 External knowledge boundaries  | `reference-identity`, `research-policy`                        | [decision 24](../decisions/24-r13-3-entry-review.md) PASS | `ef597de` (corpus v26, parity held 224/224 locally)   |
| R13.4 Planning & briefing            | `planning-runtime`, `executor-brief`                           | [decision 26](../decisions/26-r13-4-entry-review.md) PASS | landed (corpus v27, parity held 226/226 locally)      |
| R13.5a Slash-dispatch core           | `cli-session` (begins)                                         | [decision 28](../decisions/28-r13-5a-entry-review.md) PASS (oracle `cli-session-oracle.mjs` at `afb5f19`) | landed (corpus v28, parity held 227/227 locally — `cli-session` ×6: input-parsing, session-lifecycle, help-and-commands, status-view, unknown-command, prompt-turn) |
| R13.5b Briefing + real manifests     | `executor-brief` extension                                     | [decision 29](../decisions/29-r13-5b-entry-review.md) PASS (8 groups at v29) | landed (corpus v29, parity held 228/228 locally — `executor-brief` ×8: briefing-service memoization, S3M8×11, S3M9×13, S3M10×13, S3M11×18, selection, dynamic-context, fingerprint) |
| R13.5c Deferred seams                | existing subjects extended                                     | [decision 30](../decisions/30-r13-5c-entry-review.md) PASS (10 groups at v30) | landed (corpus v30, parity held 231/231 locally — `knowledge-revisions` seeding ×3, `reference-identity` access ×4, `research-policy` access ×4) |
| R13.5d Full cli-session closure      | `cli-session` closure                                          | mini review pending                                       | —                                                     |

## Definition of done

All R13 slices landed with differential parity at their corpus bumps and
the full gate observed on the final assembled tree; then [R13 Verified
Promotion] proceeds (seven-surface advancement), which unblocks the R12
disposition. Fail-closed postures never flip to operational.
