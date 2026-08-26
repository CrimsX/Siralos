---
title: "R13.5 Entry Review - CLI Product Composition Contract"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: "PASS — R13.5 contract frozen as four ordered sub-slices; R13.5a authorized (decision 27)"
blockedBy:
  - 21-remaining-surface-ports-entry-review.md
---

## Question

What is the frozen contract for R13.5 CLI product composition — how do the
~75-command interactive session, the briefing-service wiring, the real S3M
manifests, and the three seam groups deferred from R13.2/R13.3/R13.4 get
sliced, probed, and closed at full behavioral parity?

## Resolution

Resolved by interactive HITL grilling on 2026-08-25 over reads of
`apps/cli/src/interactive-session.ts` (~75 dispatch arms),
`apps/cli/src/{session,input,output}` (~188 symbols),
`packages/core/src/executor/briefing-service.ts`, and the real manifest
constants (`s3m8/s3m9/s3m10/s3m11-manifest.ts`). The frozen contract — four
ordered sub-slices (a: slash-dispatch core, b: briefing + real manifests,
c: deferred seams inside existing subjects, d: full cli-session closure) with
scripted-stdin session probes over the real composition and all four real
manifests as fixture constants — is recorded in
[decision 27](../decisions/27-r13-5-entry-review.md) — **PASS; R13.5a is
authorized as the next implementation slice.**
