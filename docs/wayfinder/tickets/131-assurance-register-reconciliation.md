---
title: "Assurance registers vs the Verified record, and the untracked items beside them"
label: "wayfinder:ticket"
status: closed
date: "2026-09-12"
supersedes: []
---

# Assurance registers vs the Verified record, and the untracked items beside them

Found by a read-only sweep of every Markdown file in the repository (2026-09-12),
run because the map reported no open work at all: 0 open tickets, no open fog.
That report is the input to this ticket; nothing here is a code defect.

## Why this exists

The map's frontier is empty and every milestone it names is Verified, yet four
documents that call themselves authoritative still describe work as PARTIAL,
NOT DUE or waiting on milestones that have since been Verified -- and a handful
of concrete follow-ups have no owning record anywhere. A reader who trusts those
documents would re-derive scope that is already delivered, or restate a boundary
as pending.

## A. Unreconciled registers (stale text, no owner)

| Where                                            | What it still says                                                                   | The record says                                                               |
| ------------------------------------------------ | ------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------- |
| `docs/requirements/REQUIREMENTS.md` (~30 rows)   | PARTIAL/NOT DUE against R4/R10/R11/R12                                               | R4-R13 Verified; CORE-001 cites a TypeScript reference removed by decision 40 |
| `docs/development/GOLDEN_TRACES.md` (13 rows)    | GT-015 "full H2 replay parity is R10-R11"; GT-012 "no credential-consuming provider" | R10/R11 Verified; providers Verified (decisions 66-71)                        |
| `docs/development/PLATFORM_CONFORMANCE.md`       | "not yet testable ... when the corresponding subsystems are ported (R4+)"            | R4+ landed; the list is now a coverage question, not a porting one            |
| `docs/development/STRUCTURED_INPUT_INVENTORY.md` | "the TypeScript reference remains the product's actual boundary surface"             | Rust is the sole source of truth (decision 40)                                |

Also stale, each a single sentence in a live document:
`docs/development/PROJECT_CONTEXT.md` ("Real provider integrations are not
implemented in the current milestone" -- contradicted by its own header),
`docs/development/R7_BEHAVIOR_EXTRACTION.md` and `docs/development/RUST_MIGRATION.md`
("R7 remains Active and is not marked Verified"),
`docs/development/performance-baseline.md` ("when their subsystems are ported
(R4+)"), `docs/adr/0034-godot-domain-host-boundary.md` ("Stage 4 execution
boundaries are not implemented yet"), and decision cross-references that were
closed later (decisions 84, 106, 112, 120, 139, 140, 162).

**Acceptance:** every row either cites the decision that closed it or is moved to
an explicitly open item with an owner; the four registers stop contradicting the
Verified record; no capability claim changes -- this is reconciliation, not
re-scoping.

## B. Untracked follow-ups (nothing owns them)

1. **Licensing decision.** `docs/development/SUPPLY_CHAIN.md`: "the licensing
   decision is deferred to Stage 6 release planning" -- Stage 6 is Verified and
   the crates still carry no license field, so the deferral's decision point
   passed with no owner. Concrete, and the only item here with a hard external
   consequence (unpublished crates with no license).
2. **`cargo-vet` re-evaluation.** Same file: "not adopted at this stage" -- the
   re-evaluation has no record.
3. **Deferred provider work** (decision 102): tool loading, reasoning-effort
   tier gating, structured output -- recorded in a decision only.
4. **Protocol shaping** (decision 132): "recorded as future work", and that
   sentence is the only record.
5. **RFC backlog** (`docs/architecture/RFC_INDEX.md`): durable journal, durable
   runtime trace, persistence, plugin ecosystem, and RFC-0011's reason ("follows
   subsystem migration") which the completed migration invalidates.

**Acceptance:** each item is either given a decision-ready shape (a ticket of its
own) or explicitly retired with a reason. Deliberate permanent boundaries (the
fail-closed posture, "FUTURE / NOT DUE" product limits, the dormant approval
seam, the gated context activation) are NOT in scope: they are boundaries, and
this ticket does not reopen them.

## Out of scope

- Any capability change, any code change, any corpus or differential change.
- Re-opening a boundary that a decision closed on purpose (this ticket
  reconciles text about them, it does not relitigate them).

## Resolution (2026-09-12)

Both halves are done, and [decision 174](../decisions/174-assurance-registers-reconciled.md)
records it.

**Registers.** `REQUIREMENTS.md` 34 rows rewritten (17 promoted to `VERIFIED` with
the closing milestone or decision in the evidence cell; 17 kept `PARTIAL`/`NOT DUE`
with a current reason and an owner); `GOLDEN_TRACES.md` 13 rows rewritten (3
promoted, the rest re-reasoned); `RFC_INDEX.md` 6 rows re-owned;
`PLATFORM_CONFORMANCE.md` and `STRUCTURED_INPUT_INVENTORY.md` corrected in prose.
Each of the four registers opens with a short `Reconciliation (2026-09-12)` note
stating what changed -- status and evidence only, never the normative text.

**Follow-ups.** The licensing decision and the cargo-vet re-evaluation are ticket
[132](132-supply-chain-decisions.md); the provider work deferred in decision 102 and
the protocol shaping from decision 132 are ticket
[133](133-provider-interface-deferred-work.md); RFC-0011's consolidation (now
unblocked) is ticket [134](134-error-diagnostic-consolidation.md); the durable
journal, persistence, the marketplace and the durable trace protocol are recorded as
boundaries rather than pending work.

Four single sentences that contradicted the record outright were fixed with the
ticket: the map's own Notes line, `PROJECT_CONTEXT.md`, and the two "R7 remains
Active" tails. No capability claim was weakened.
