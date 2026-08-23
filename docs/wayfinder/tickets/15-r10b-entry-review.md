# Ticket — R10b Entry Review — Freeze the ICM Contract Before Any Code

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:grilling` HITL
**Blocked by:** [R10 Entry Review](../decisions/14-r10-entry-review.md) (PASS — R10a authorized and landed)
**Status:** RESOLVED by [decision 15 — R10b Entry Review](../decisions/15-r10b-entry-review.md)

## Questions this ticket resolves

1. Which of the four frozen `icm.*` subject schemas are in scope for the
   R10b implementation slice, and what are their canonical payload shapes
   grounded against the TS reference (`packages/core/src/context/**`)?
2. What does the **narrowing-only authority invariant** mean mechanically,
   and how is it tested differentially?
3. How does the PhaseContract digest bind to the H1 artifact-digest
   primitive, and what is the exact domain separator?
4. Which context/provenance/staleness helpers are core-owned versus
   adapter-owned, and what is the differential boundary between them?
5. Does passing this gate authorize all four `icm.*` subjects at once?
