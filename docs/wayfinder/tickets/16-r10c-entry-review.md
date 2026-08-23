# Ticket — R10c Entry Review — Freeze the H3 Runtime-Readiness Contract Before Any Code

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:grilling` HITL
**Blocked by:** [R10b Entry Review](../decisions/15-r10b-entry-review.md) (PASS — R10b implemented and landed at corpus v19)
**Status:** RESOLVED by [decision 16 — R10c Entry Review](../decisions/16-r10c-entry-review.md)

## Questions this ticket resolves

1. What are the canonical payload shapes for the four frozen
   `runtime-readiness.*` subjects, grounded against the TS reference
   (`packages/core/src/runtime/**`: identity 79, budget 153,
   side-effects 139, supervision 360, readiness 224, faults 205,
   doctor 43, modes 103, artifacts 239 lines)?
2. How does the deterministic fault-injection harness stay
   harness-owned and reproducible under the H2 controlled clock —
   same FaultScript + clock ⇒ same observation sequence?
3. Which surfaces are core-owned (`siralos_core::runtime`) versus
   explicitly deferred to R11 (real process launch, sandbox
   enforcement, run-directory creation, recovery orchestration)?
4. How do budgets bind to run-identity digests through the single
   artifact-digest primitive without pretending memory/CPU limits are
   enforced when the backend cannot enforce them?
5. Does passing this gate authorize all four `runtime-readiness.*`
   subjects at once, and does it close R10's sub-slice chain
   (`R10a → R10b → R10c`) leaving only the single R10 Verified
   promotion ahead of R11?
