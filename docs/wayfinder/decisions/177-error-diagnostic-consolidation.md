---
title: "Error and Diagnostic Consolidation: Per-Subsystem Typed Errors Are the Design"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "134"
supersedes: []
---

# Error and Diagnostic Consolidation: Per-Subsystem Typed Errors Are the Design

Ticket [134](../tickets/134-error-diagnostic-consolidation.md) ·
[RFC_INDEX](../../architecture/RFC_INDEX.md) · [Map](../siralos-roadmap.md)

## 1. The question, unblocked

RFC-0011 (Error and Diagnostic Model) has said "consolidation follows subsystem
migration" since before the migration finished. The migration is complete (decision 40,
R1-R13 Verified), so the question is live: is a shared error/diagnostic vocabulary
worth it at this size, or is the per-subsystem shape the end state?

## 2. What is actually there (counted)

30 named error enums across the three crates — 18 in `siralos-core`
(`CapabilityIdError`, `CheckpointInvariantError`, `ContextGraphError`, `ContractError`,
`EvidenceError`, `FindingError`, `GitError`, `OpError`, …), 9 in `siralos-adapters`
(`BenchmarkError`, `CheckpointStoreError`, `ConfigError`, `ReadInputError`,
`ReplayStoreLoadError`, `ReplayStoreWriteError`, `ScanError`, `StateDirError`, …) and 3
in `siralos-cli` (`ConfigurationError`, `HarnessError`, `InteractiveError`). `thiserror`
appears nowhere: each type hand-writes `Display` and `std::error::Error`, which is what
`RUST_STYLE.md` asks for.

## 3. The ruling

**Per-subsystem typed errors are the design. No consolidation slice.** The decisive
argument is architectural, not aesthetic: the dependency direction is
`cli → adapters → core`, so a single shared error vocabulary living in `core` **cannot
name an adapter or CLI failure without inverting that direction**, and one living
above it would force every layer to depend upward. The alternatives are worse than the
status quo — a mega-enum in core (dependency inversion), a boxed `dyn Error` everywhere
(loses the typed precision the style guide requires and the tests rely on), or shared
string codes (a second vocabulary beside the typed one, which is the duplication this
codebase keeps refusing).

The 30 types are not accidental duplication: each names the subsystem and the operation
that can fail (`ReplayStoreLoadError` vs `ReplayStoreWriteError`, `ReadInputError`,
`StateDirError`), which is exactly what makes a failure legible at the call site.

**What IS shared already, and is the right shared thing:** the presentation boundary —
`sanitize_for_display` and the terminal sanitizer for anything reaching a terminal, and
the bounded sanitized diagnostic model in `siralos_core::language` for language-service
output. That is where a common shape belongs, and it exists.

**What would change the answer:** a cross-layer consumer that needs to branch on
failures it did not originate — e.g. a machine-readable code that must be stable across
the CLI boundary for an external caller. Then the answer is a small typed _category_
mapped by each layer at the boundary, not a merge of the layers' error types.

## 4. Consequences

RFC-0011 is recorded as **owned by the design** in the RFC index rather than
"partially owned", and ticket 134 closes. No code changes: this decision ratifies the
existing shape instead of proposing work with no evidence behind it.

## 5. Evidence

- The counts above were produced from `crates/**` for this record (enums named
  `*Error`, unique per crate); the `thiserror` count is zero.
- `RUST_STYLE.md`: explicit typed errors, no framework imported for their own sake.
- The dependency direction is enforced by `npm run check:rust` (architecture gate),
  which is what makes the inversion argument mechanical rather than stylistic.
