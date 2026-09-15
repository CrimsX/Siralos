---
title: "Error and diagnostic consolidation, unblocked by the completed migration"
label: "wayfinder:ticket"
status: closed
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

## Resolution (2026-09-12)

[Decision 177](../decisions/177-error-diagnostic-consolidation.md) answers the
question and closes RFC-0011: **per-subsystem typed errors are the design**, no
consolidation slice.

The decisive argument is mechanical: the dependency direction is
`cli → adapters → core` (enforced by `npm run check:rust`), so a shared error
vocabulary in `core` cannot name an adapter or CLI failure without inverting that
direction, and the alternatives are worse — a mega-enum in core, `dyn Error`
everywhere (losing the typed precision the style guide requires), or shared string
codes (a second vocabulary beside the typed one).

Counted for the record: 30 named error enums (core 18, adapters 9, cli 3), each
naming its subsystem and operation, none importing an error framework. What IS
shared is the presentation boundary — `sanitize_for_display` and the terminal
sanitizer, plus the bounded diagnostic model in `siralos_core::language` — which is
where a common shape belongs.

RFC-0011 is recorded as **owned by the design** in the RFC index; the one thing that
would reopen it is a cross-layer consumer needing a stable machine-readable category
(mapped at the boundary by each layer, not by merging the layers' types).
