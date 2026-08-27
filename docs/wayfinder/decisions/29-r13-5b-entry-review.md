# Decision — R13.5b Entry Review — Briefing-Service + Real Manifest Fixtures

**Wayfinder ticket:** [R13 Execution Register](../tickets/22-r13-execution-register.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md)
**Blocked by:** [R13.5 Entry Review](27-r13-5-entry-review.md) (PASS) + R13.5a landed (`e40de7c`, parity held 227/227 at corpus v28)
**Decided:** 2026-08-26 (resolver session, HITL grilling over reads of `packages/core/src/executor/briefing-service.ts:34-242`, `crates/siralos-core/src/executor/briefing.rs`, and the four real manifests `s3m8`→`s3m11`)
**Status:** **PASS — R13.5b scenario set frozen; implementation authorized**

---

## Frozen scenario set (`executor-brief` extension, ~8 groups, corpus v29)

All cases extend the existing `executor-brief` subject (no new subject name) and reuse the R13.4 synthetic-task + injected-clock posture, but replace synthetic milestone fixtures with the **four real manifest constants** as fixture inputs. The briefing-service composition (memoized `latestOrCompile` over task-stable identity) is exercised through the pure Rust `siralos_core::executor::briefing` seam.

| #   | Case                                  | What it proves                                                                                                                                                                                                                                                                   |
| --- | ------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `briefing-service-memoization`        | Same task id / contract rev / plan id-rev-approval / milestone version / execution-contract rev + same dynamicContextDigest returns the memoized brief (pointer-identical fingerprint); a different `planRevision` or `dynamicContextDigest` invalidates the memo and recompiles |
| 2   | `s3m8-real-manifest`                  | `S3M8_MILESTONE_MANIFEST` (id S3M8, 15 acceptance ids) compiles via `compileExecutorBrief` with correct `executionContract` reference and `architectureConcerns`; fingerprint stable across two compiles                                                                         |
| 3   | `s3m9-real-manifest`                  | `S3M9_MILESTONE_MANIFEST` (id S3M9, 13 acceptance ids) compiles; `prerequisites` and `invariants` preserved byte-equal                                                                                                                                                           |
| 4   | `s3m10-real-manifest`                 | `S3M10_MILESTONE_MANIFEST` (id S3M10, 13 acceptance ids) compiles; `deliverables` and `nonGoals` preserved                                                                                                                                                                       |
| 5   | `s3m11-real-manifest`                 | `S3M11_MILESTONE_MANIFEST` (id S3M11, 18 acceptance ids) compiles; `requiredTests` and `architectureConcerns` include `executor-briefing`                                                                                                                                        |
| 6   | `milestone-selection-by-request`      | `selectMilestone` host-owned routing: request containing Godot `scene`/`resource` token selects S3M11, plain request selects null — deterministic, never model-decided                                                                                                           |
| 7   | `dynamic-context-digest-invalidation` | A `workspaceScope` change (verified vs candidate file classification) changes `dynamicContextDigest` and forces recompilation even though task-stable identity is unchanged                                                                                                      |
| 8   | `fingerprint-canonical-stability`     | `computeExecutorBriefFingerprint` over the S3M8 brief is canonical-JSON-bound and byte-identical on twin compiles                                                                                                                                                                |

## Mechanics

- Oracle probe extends `executor-brief-oracle.mjs` with a new `briefing-service` case group that drives the **real TypeScript** `createExecutorBriefing` over synthetic task contracts but **real** `S3M*_MILESTONE_MANIFEST` constants; the Rust side ports via `siralos_core::executor::briefing` behind the same `executor-brief` subject. Placement obeys `cli → adapters → core`.
- Corpus bumps **v29** at the R13.5b reconciliation commit; schema stays 3.
- **Injected clock everywhere**: `createdAt`, `approvedAt`, and rendered timestamps come from one fixed clock so records are byte-stable.
- **Real manifests as fixture constants** (HITL decision 2026-08-25, decision 27 §1.4): the four `S3M*_MILESTONE_MANIFEST` values are imported as constants, never generated from the repository's live manifest selection.
- Determinism posture: one injected `NowFn` per brief, no filesystem, no network, no TTY, sanitizer not involved.

## Boundaries — not in R13.5b

- Deferred seams stay in R13.5c (`knowledge-seeding`, `reference-identity` access tools, `research-policy` access port/tools) per decision 27 §2.
- Full `cli-session` closure (remaining ~75 commands, `godot`/`develop` unavailable reporting, sanitizer boundary) stays in R13.5d.
- No capability surface changes: briefing-service memoization never grants capability; milestone selection remains host-owned.

## Authorization

Implementation of R13.5b is authorized against this frozen set; landings are recorded in the [R13 Execution Register](../tickets/22-r13-execution-register.md).

---

## Self-loop verification

| Criterion                                    | Direct evidence                                                                                                                                                                                                                                                                   | Status |
| -------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Cases exercise reference-observable behavior | `briefing-service.ts:34-242` memo key (taskId/contractRevision/planId/planRevision/planApproval/milestoneVersion/executionContractRevision/dynamicContextDigest) and `compileForRequest` read this session; `s3m8`→`s3m11-manifest.ts` acceptance counts (15/13/13/18) enumerated | pass   |
| Determinism posture preserved                | § Mechanics injects one fixed clock; synthetic contracts + real manifest constants only; no fs/network                                                                                                                                                                            | pass   |
| Overlap resolved, no double port             | R13.4 synthetic `executor-brief` cases (10) remain; R13.5b adds service + real manifests only; R13.5c/d seams untouched per decision 27                                                                                                                                           | pass   |
| Human decided the material cuts              | HITL answer 2026-08-26: “8 groups as proposed, v29, real manifests as constants, memoization included”                                                                                                                                                                            | pass   |
