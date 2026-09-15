---
title: "The Assurance Registers Reconciled With the Verified Record"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "131"
supersedes: []
---

# The Assurance Registers Reconciled With the Verified Record

Ticket [131](../tickets/131-assurance-register-reconciliation.md) · the sweep that
found it · [Map](../siralos-roadmap.md)

## 1. What was wrong

The map said there was no open work (0 open tickets, no open fog) while four
documents that call themselves authoritative still described a repository that no
longer exists. A reader following them would re-derive scope that is Verified, or
mistake a deliberate boundary for a queue.

## 2. What was reconciled

**`docs/requirements/REQUIREMENTS.md`** — 34 rows rewritten. 17 are now `VERIFIED`
with the milestone or decision that closed them in the evidence cell (CORE-001/007/
008/010/011/012/013/019, HAR-008/023/024/029/042/043/050/053, AP-011); the other 17
keep `PARTIAL`/`NOT DUE` and now say WHY in current terms — either a real open item
with an owner, or the fail-closed boundary (mutation, checkpoint creation, process
execution) pointing at SECURITY.md instead of implying unfinished work.

**`docs/development/GOLDEN_TRACES.md`** — 13 rows rewritten: GT-008/009/015 are
`VERIFIED` (R6 lifecycle scenarios; `/domains-activate`; the replay arc), and the
rest now distinguish "waiting on a milestone" (nothing) from "needs an effect the
Host refuses to perform" (GT-005/006/013/017) or "needs the persistence that is not
committed" (GT-018).

**`docs/architecture/RFC_INDEX.md`** — 6 rows re-owned: the durable journal,
persistence and the marketplace are recorded as boundaries rather than pending work;
RFC-0011's reason ("consolidation follows subsystem migration") expired with the
completed migration, so it is now owned by ticket 134; the supply-chain item by
ticket 132.

**`docs/development/PLATFORM_CONFORMANCE.md`** — the untested list is now framed as
what it is: a boundary (no mutation, checkpoint or process effect exists to test),
with the read surfaces and the three atomic writers named as the coverage that does
exist.

**`docs/development/STRUCTURED_INPUT_INVENTORY.md`** — no longer claims the TypeScript
reference is the boundary surface; `packages/**` rows are marked historical and the
Rust crates named as their home.

Four single sentences that contradicted the record outright were fixed with the
ticket (the map's own "external siralos-godot repo stays FUTURE", PROJECT_CONTEXT's
"real provider integrations are not implemented", and the two "R7 remains Active and
is not marked Verified" tails in R7_BEHAVIOR_EXTRACTION.md and RUST_MIGRATION.md).

## 3. What the sweep's other findings became

- **Licensing** (the one item with an external consequence: the crates still carry no
  license field and Stage 6 — the deferral's decision point — is Verified) and the
  **cargo-vet re-evaluation**: ticket [132](../tickets/132-supply-chain-decisions.md).
- **Provider work deferred in decision 102** (tool loading, reasoning-effort tier
  gating, structured output) and the **protocol shaping** from decision 132: ticket
  [133](../tickets/133-provider-interface-deferred-work.md).
- **RFC-0011's consolidation**: ticket
  [134](../tickets/134-error-diagnostic-consolidation.md).
- **Deliberate boundaries** (fail-closed posture, "FUTURE / NOT DUE" product limits,
  the dormant approval seam, gated context activation): recorded as boundaries, not
  reopened. The sweep looked for work; it did not get to relitigate decisions.

## 4. Evidence

- `npm run check` exit 0 with the reconciled registers (the docs gates — links,
  project context, formatting — are what cover them).
- Not a code change: no capability claim is weakened, no implementation is touched,
  and every promoted row names the decision or milestone a reviewer can check.
- One process note worth keeping: the sweep itself was read-only, and one early
  rewrite of `REQUIREMENTS.md` was discarded because a tool truncated the file on
  read (the register was restored from git before anything was committed). The
  reconciliation was then applied through a row-mapping script with a rewritten-row
  count and a missing-id check, which is the safe way to edit a 179-line table.
