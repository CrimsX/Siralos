# Ticket — R9 Entry Review — Freeze the Godot Stage-3 Contract Before Any Code

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:grilling` HITL
**Blocked by:** [R8 Verified Promotion](../decisions/11-r8-verified-promotion.md) (Verified at worktree `c075b3c`)
**Status:** OPEN

## Questions this ticket resolves

1. Which of the three Stage-3 TypeScript surfaces (`ROADMAP.md` §3.9 review
   context & impact intelligence, §3.10 approved scene/resource mutation,
   §3.11 unified `/develop` workflow) are **R9 MUST PORT**, and which parts are
   generic seams or later-milestone work?
2. What is each surface's **fail-closed posture** — where does `apply`,
   checkpoint creation, or any directory-relative write stay typed
   `unavailable`, and what does "prepare-only" mean for provider tools?
3. Which **differential subjects** freeze for R9, grounded in what the
   TypeScript oracle can execute deterministically without engine or write
   effects?
4. What evidence layers and measurement discipline apply (per
   `RUST_STYLE.md` evidence-first rule)?
5. Does passing this gate authorize R9 only — never R10+ or Stage-4 entry?

Resolves per the porting-gate precedent (`R7_BEHAVIOR_EXTRACTION.md` §14 /
[decision 10](../decisions/10-r8-entry-review.md)): contract frozen at
byte-level before any R9 implementation commit; the real corpus version bump
lands with the R9 implementation reconciliation, not this decision.
