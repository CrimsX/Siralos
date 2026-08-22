# Decision — R10 Entry Review — Freeze the Runtime-Readiness Milestone Contract

**Wayfinder ticket:** [R10 Entry Review — Freeze the Runtime-Readiness Milestone Contract](../tickets/14-r10-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R9 Verified Promotion](../decisions/13-r9-verified-promotion.md) (Verified at worktree `1623e80`)
**Decided:** 2026-08-22 (resolver session, interactive review of `decisions/05-r10-scope.md` §1–§4, `docs/development/PROJECT_CONTEXT.md` §14 H1/H2/ICM/H3 (lines 663–691), the TypeScript reference inventory for `packages/core/src/{identity,determinism,context,runtime}/**`, and the Rust entry state at HEAD)
**Status:** **PASS — R10 milestone contract frozen; R10a (H1 + H2) authorized as the next implementation slice**
**Self-loop ledger:** 5 criteria, one implementation pass (verification below)

> Mirrors decision 10/12: contract frozen before any slice code. Per
> `05-r10-scope.md` §4, this is the milestone-level freeze; R10b and R10c
> still require their own sub-slice entry reviews before their code.

---

## Summary

Freeze **R10 — H1 content identity, H2 determinism/replay, ICM context, and
H3 runtime-readiness parity** as one Verified milestone with three ordered,
entry-reviewed sub-slices (`R10a → R10b → R10c`, hard dependency chain
`H1 → H2 → ICM → H3`). This decision graduates all thirteen subject names from
proposed to frozen, freezes the R10a scenario schemas, and authorizes **R10a
only**. R10b and R10c freeze their byte-level schemas at their own sub-slice
entry reviews.

## 1. Entry state

| Item                    | Value                                                                                                                                                                                                                                                   |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Branch / worktree       | `main`, clean                                                                                                                                                                                                                                           |
| Prior verified worktree | `1623e800f8034d07825d7c6582768c27a91a973e` (R9 Verified)                                                                                                                                                                                                |
| Corpus                  | schema 3, version 17, 167 files, 162/162 applicable required parity                                                                                                                                                                                     |
| R10 executable Rust     | absent — no `determinism`, `context`, `runtime`, or `doctor` modules exist under `crates/siralos-core/src`; the H1 seam partially exists as `siralos_core::identity` (canonical JSON, SHA-256, `artifact_digest_hex`) inherited from the R3 task kernel |
| Gates at this gate      | fmt / clippy `-D warnings` / workspace tests (691) / check:architecture / check:rust / check:differential 162/162 — observed PASS on the identical tree during the R9 promotion sweep                                                                   |

## 2. Co-located classification table

| Behavior                                                                                                                                                                                                                                       | Classification                                                           | Frozen boundary and owner                                                                                                                                                                                                                                                                                                                                                                                                           |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| H1 content identity (artifact digest, contract/plan identity, manifests, semantic delta, staleness)                                                                                                                                            | **R10a MUST PORT**                                                       | Single digest architecture only: domain-separated `ArtifactDigest` over canonical JSON + SHA-256 of exact bytes (`PROJECT_CONTEXT.md:668-686`). TS oracle `packages/core/src/identity/{artifact-digest,contract-plan-identity,manifests,semantic-delta,staleness}.ts` (108/191/446/106/87 lines). Owner: `siralos-core::identity` extended — never a parallel hash family.                                                          |
| H2 determinism/replay (explicit clock/random/ordering ports, environment + reproducibility manifests, deterministic decisions/validation/retry, concurrency normalization, deterministic discovery with ownership index, nondeterminism audit) | **R10a MUST PORT**                                                       | Host-owned ports replace implicit `Date.now()`/`Math.random()` in host decisions; nondeterministic observations are recorded or typed `unreplayable` — never silently replayed. TS oracle `packages/core/src/determinism/{context,decisions,discovery,doctor,environment,reproducibility}.ts`. Owner: `siralos-core::determinism` (new module family) + `identity` for manifest binding.                                            |
| ICM phase contracts / provenance / projection / briefing                                                                                                                                                                                       | **R10b MUST PORT** (schema freeze deferred to the R10b sub-slice review) | PhaseContract with narrowing-only authority, digest-bound envelopes, targeted content-addressed staleness, phase-driven projection extension, Executor Context Pack / brief compiler (`packages/core/src/context/**`: artifacts 183, phase-contract 517, projection 147, provenance 131, source-integrity 113, staleness 113; `executor/brief-compiler.ts`). Owner: `siralos-core::context` (new) + projection/executor extensions. |
| H3 runtime-readiness (run identity/budgets/lifecycle/failure taxonomy/fault injection)                                                                                                                                                         | **R10c MUST PORT** (schema freeze deferred to the R10c sub-slice review) | Causal run identity, manifest-bound budgets with typed `resource_exceeded`, host-observed cancellation, deterministic reconciliation, harness-owned fault injection, seven-way failure taxonomy (`packages/core/src/runtime/**`: identity 79, budget 153, side-effects 139, supervision 360, readiness 224, faults 205, doctor 43, modes 103, artifacts 239). Owner: `siralos-core::runtime` + `doctor`.                            |
| Effect-boundary hardening, run-directory creation/cleanup, sandbox enforcement conformance, recovery orchestration, full-corpus cross-platform audit                                                                                           | **LATER — R11** (restated boundary)                                      | R10 records intention and evidence (digests/manifests), never promises writes or proves the security boundary — `05-r10-scope.md` §3 five-row table stands unchanged.                                                                                                                                                                                                                                                               |
| Parallel hash families (`SkillHash`, `PluginChecksum`, per-domain digests)                                                                                                                                                                     | **DO NOT PORT — forbidden by freeze**                                    | The single-digest invariant (`PROJECT_CONTEXT.md:678-680`) is enforced mechanically: new digest helpers must live in `siralos-core::identity`, and `npm run check:architecture` source guardrails reject digest-shaped helpers outside it.                                                                                                                                                                                          |
| Lean machinery (multi-agent, Hooks, TaskGraph, workflow engines, marketplaces, model-router, Memory, GUI/TUI)                                                                                                                                  | **DO NOT PORT — lean freeze**                                            | ADR 0036 unchanged.                                                                                                                                                                                                                                                                                                                                                                                                                 |

## 3. Frozen differential subject names

Graduated proposed → frozen (names verbatim from `05-r10-scope.md` §2):

- **R10a (authorized now):** `content-identity.artifact-digest`,
  `content-identity.contract-digest`, `content-identity.manifests`,
  `content-identity.delta`, `determinism.replay`
- **R10b (frozen names, schemas at its review):** `icm.phase-contract`,
  `icm.dependency-manifests`, `icm.projection`, `icm.briefing`
- **R10c (frozen names, schemas at its review):** `runtime-readiness.identity`,
  `runtime-readiness.budgets`, `runtime-readiness.lifecycle`,
  `runtime-readiness.doctor`

Scenario schemas for the five R10a subjects are frozen against the TypeScript
entry points: `canonicalArtifactPayload` / `computeArtifactDigestHex` /
`validateArtifactDigest` (`artifact-digest.ts:42-82`), the contract/plan
identity functions, manifest builders, delta derivation, and the determinism
ports. Fixtures drive both implementations through real digest/manifest APIs;
harness comparison stays semantic JSON-path (ADR 0033).

## 4. Corpus mechanics across a multi-slice milestone

The corpus advances when fixtures land, never at a review: **v18 lands with
the R10a reconciliation commit**, v19 with R10b's, v20 with R10c's (final
numbering may compress if slices share a reconciliation). One promotion commit
still closes R10 as a single Verified milestone once a/b/c are all
evidence-backed.

## 5. Measurement

Per `RUST_STYLE.md:568-589`: benchmarks appear only where a hot spot is
measured. Candidate hot spots named now for honesty, none speculative:
canonical payload serialization throughput (large manifests) and digest
batching. None are added at freeze time.

## 6. Authorization

**PASS — R10a (H1 + H2) implementation is authorized as the next slice**
against the five frozen subjects above. This does not authorize R10b, R10c,
R11, Stage-4, or any corpus promotion; those gate on their own reviews and
reconciliation commits respectively.

---

## Self-loop verification (this decision)

| Criterion                                         | Direct evidence                                                                                                                  | Status |
| ------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Sub-slice structure honored (freeze now vs defer) | Summary + §2 classification rows mark R10b/R10c schema freezes as deferred to their own reviews, per `05-r10-scope.md` §4        | pass   |
| Subject names graduated with grounded schemas     | §3 lists 13 frozen names; R10a schemas cite TS entry points (`artifact-digest.ts:42-82`) and module inventories with line counts | pass   |
| Entry state audited                               | §1 table observed this session (clean tree; corpus v17/167; no R10 Rust modules; partial H1 seam identified)                     | pass   |
| Single-digest invariant mechanically enforced     | §2 DO-NOT-PORT row assigns the rejection to `check:architecture` guardrails, not comments                                        | pass   |
| Authorization limited to R10a                     | §6; R10b/c/R11/Stage-4 explicitly untouched                                                                                      | pass   |

Evidence ladder: L1 observed entry state + TS/Rust module inventories with
line counts; L2 file:line citations (`PROJECT_CONTEXT.md:663-691`,
`artifact-digest.ts:42-82`, `05-r10-scope.md` §1–§4); L3 porting-gate
precedent (decisions 10/12); L4 this decision.
