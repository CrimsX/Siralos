---
title: "R13.5b Entry Review - Briefing-Service and Real Manifests"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: "PASS — R13.5b scenario set frozen; implementation authorized (decision 29)"
blockedBy:
  - 27-r13-5-entry-review.md
---

## Question

What is the frozen contract for R13.5b — how do the briefing-service memoization and the four real S3M manifests (s3m8/s3m9/s3m10/s3m11) get exercised at full parity inside the existing `executor-brief` subject at corpus v29?

## Resolution

Resolved by HITL grilling on 2026-08-26 over reads of `packages/core/src/executor/briefing-service.ts` (memoization over task-stable identity + dynamicContextDigest), `crates/siralos-core/src/executor/briefing.rs`, and the four real manifest constants (`s3m8` 15 ids / `s3m9` 13 / `s3m10` 13 / `s3m11` 18). The frozen contract — eight groups at v29 (memoization, S3M8-11 real manifests, milestone selection, dynamic-context invalidation, fingerprint stability) — is recorded in [decision 29](../decisions/29-r13-5b-entry-review.md) — **PASS; R13.5b is authorized as the next implementation slice.**
