---
title: "R10 Scope — H1/H2/ICM + H3 Runtime-Readiness Parity Slice Shape"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: decisions/05-r10-scope.md
blockedBy: ["01-r7-5-review-rubric.md"]
---

## Question

Decide the shape of **R10 (H1 content identity, H2 determinism/replay, ICM context, H3 runtime-readiness parity)**.

Resolve:

- Should R10 remain a single milestone or split into sub-slices (e.g., R10a identity/determinism, R10b ICM, R10c runtime-readiness)? Justify via dependency: which H1/H2/ICM pieces must precede H3?
- For each piece, name the differential subject(s) and the core-owned seams (canonical digests in `siralos-core::identity`, deterministic ordering ports, executor briefing contracts `S3M8/9/10/11`).
- State what stays out of R10 and belongs to R11 (effect-boundary hardening, recovery orchestration, cross-platform) so scope does not creep.

Decision only; no R10 design doc.

Blocked by: `01-r7-5-review-rubric.md`.

## Resolution

Closed — decision recorded in [decisions/05-r10-scope.md](../decisions/05-r10-scope.md). This was unblocked by the rubric (01) and is read-only-only — no code. This unblocks [R11 Gate](../tickets/06-r11-gate.md) (which was blockedBy this ticket) — frontier now includes 06.
