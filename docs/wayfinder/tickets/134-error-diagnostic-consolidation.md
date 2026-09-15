---
title: "Error and diagnostic consolidation, unblocked by the completed migration"
label: "wayfinder:ticket"
status: "open"
date: "2026-09-12"
supersedes: []
---

# Error and diagnostic consolidation, unblocked by the completed migration

`docs/architecture/RFC_INDEX.md` still says RFC-0011 (Error and Diagnostic Model) is
waiting because "consolidation follows subsystem migration". The migration is
complete (decision 40, and R1-R13 Verified), so that reason has expired: the
consolidation is unblocked and unowned.

## What is actually there

Typed result and error contracts exist per subsystem — `siralos_core::language`
diagnostics, the typed provider/tool outcomes, the CLI's `InteractiveError`, the
adapter error enums, the replay-store and lockfile error types — and each was built
for its own slice. Nobody has looked at them together.

## Acceptance

An entry review that answers, with evidence: is a shared error/diagnostic vocabulary
worth it at this size, or is per-subsystem typed errors the right end state? Either
answer closes RFC-0011 — a decision to consolidate into a slice, or a decision that
the current shape is the design, recorded in the RFC index as owned.

## Out of scope

- Changing any error message that a test or the differential corpus pins.
- Introducing a new error framework before the review says it is worth one.
