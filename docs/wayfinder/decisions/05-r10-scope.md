# Decision — R10 Scope — H1/H2/ICM + H3 Runtime-Readiness Parity Slice Shape

**Wayfinder ticket:** [R10 Scope — H1/H2/ICM + H3 Runtime-Readiness Parity Slice Shape](../tickets/05-r10-scope.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R7.5 Review Rubric](../decisions/01-r7-5-review-rubric.md) (closed)
**Decided:** 2026-08-18 (resolver session, reads of `docs/development/PROJECT_CONTEXT.md` §14 H1/H2/ICM/H3, `docs/development/RUST_MIGRATION.md` milestone table R10, `ARCHITECTURE.md` context/executor sections, `packages/core/src/identity/**`, `packages/core/src/determinism/**`, executor briefing contracts S3M8/9/10/11, and existing R7 sub-slice precedent)
**Status:** Scope frozen — three ordered sub-slices inside one Verified milestone, with differential subjects and core seams named; R11 boundary explicit; no design doc or code.
**Self-loop ledger:** 3 criteria, one implementation pass (verification below)

> Wayfinder **Plan, don't do** — this is a decision, not a design doc. No Rust module is created, no corpus lands, no gate advances.

---

## Summary

**R10 remains one Verified milestone** (`R10 — H1 content identity, H2 determinism/replay, ICM context, and H3 runtime-readiness parity`) but is **internally sequenced as three entry-reviewed sub-slices** mirroring the R7 pattern (R7A → R7.1-7.5 each entry-reviewed before code):

- **R10a — H1 + H2** (content identity + determinism/replay)
- **R10b — ICM** (interpretable context / phase-contract / provenance)
- **R10c — H3** (runtime-readiness: identity, budgets, lifecycle, failure taxonomy, cancellation/cleanup/reconciliation, fault injection)

One Verified commit still closes R10, but each sub-slice freezes its contract and differential subjects with an entry review before implementation. This preserves the atomic Verified gate while limiting blast radius and making the dependency chain (H1 → H2 → ICM → H3) explicit.

---

## 1. Single milestone or split — decision and dependency justification

**Decision:** keep **one** R10 milestone (`Verified` in `docs/development/RUST_MIGRATION.md` milestone table) with **three ordered sub-slices** internally.

### Why not one flat slice

R10 spans four historically separate Stage-3 horizons (H1 identity, H2 determinism, ICM, H3 readiness) that were independently implemented, tested, and documented in TypeScript (`packages/core/src/identity/**`, `packages/core/src/determinism/**`, `packages/core/src/context/**`, `packages/core/src/runtime/**`). A flat slice would require freezing 8-10 differential subjects at once, repeating the pre-R7.3 oracle-correction incident (`4b805d4ac0a9eac6d6de5a2b90b64bc6146aeafc`) at larger scale. The R7 precedent (five surfaces → five sub-slices, each entry-reviewed in `R7_BEHAVIOR_EXTRACTION.md` §13/§14) shows ordered slices reduce rework without weakening the final gate.

### Why not three independent milestones (R10, R11, R12 renumbering)

Renumbering would ripple `ROADMAP.md`, `PROJECT_CONTEXT.md`, `RUST_MIGRATION.md`, ADR 0036, and `stage4-entry-gate.md`. R10's subjects share the same differential harness versioning and the same executor-briefing contract family (S3M8-11); splitting into separate Verified milestones would duplicate promotion mechanics for no authority gain. One milestone with internal ordering preserves the existing R1-R12 contract while giving executors a clear critical path.

### Dependency chain that fixes the order

`H1 → H2 → ICM → H3` (each arrow is a hard host dependency, not a preference)

| Dependency              | Reason (host-authoritative)                                                                                                                                                                                                                                                                                                                                        | Evidence location                                                                                                                      |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| **H1 before H2**        | H2's determinism is defined as "same authoritative inputs produce the same host decision" — authoritative inputs are identified by H1 digests (`hash = exact identity`, `revision = lifecycle identity` — `PROJECT_CONTEXT.md:666-668`). Without H1's domain-separated `ArtifactDigest` and `TaskContract` / `TaskPlan` digests, H2 has nothing stable to compare. | `PROJECT_CONTEXT.md:673-686` identity model preservation; `packages/core/src/identity/artifact-digest.ts`, `contract-plan-identity.ts` |
| **H1+H2 before ICM**    | ICM is "phase-specific, provenance-aware, reconstructable typed context and artifacts" (`PROJECT_CONTEXT.md:688-689`). Phases carry `PhaseContract` digests, narrowed authority, and dependency manifests — all content-identity (H1) and must be replay-reconstructable (H2).                                                                                     | `ARCHITECTURE.md` § Interpretable context architecture; `packages/core/src/context/**` (PhaseContract, digest-bound envelopes)         |
| **H1+H2+ICM before H3** | H3 is "runtime-readiness identity, budgets, lifecycle, failure taxonomy, cancellation, cleanup, reconciliation, and fault injection" (`PROJECT_CONTEXT.md:690-691`). Budgets and lifecycle reference run identity and context provenance; failure taxonomy classification assumes deterministic host decisions.                                                    | `packages/core/src/runtime/**` (RunManifest, readiness, fault injection); new `S3M11`-style manifest                                   |

R10a therefore unblocks R10b; R10b unblocks R10c. No sub-slice may be implemented before its predecessor's entry review is PASS.

---

## 2. Per-piece differential subjects and core-owned seams

### R10a — H1 content identity + H2 determinism/replay

**What ships:** the content-identity model and the determinism contract, under differential parity.

| Differential subject (proposed, frozen at R10a entry review)                                                                                                                                                                                                 | Core-owned seam (owns semantics; adapters own no domain)                                                                                                                                                   | What the subject proves                                                                                                                                                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `content-identity.artifact-digest` — domain-separated `ArtifactDigest`, canonical JSON serialization, SHA-256 over exact bytes                                                                                                                               | `siralos-core::identity` (`identity/canonical.rs`, `identity/sha256.rs`, `identity/mod.rs`) — sole digest architecture; forbids ad-hoc `ProfileHash`/`ContextFingerprint` per `PROJECT_CONTEXT.md:678-680` | Digest of one canonical artifact matches the TypeScript oracle's `artifactDigest` byte-for-byte; protected workspace-relative identity is preserved where staleness depends on it (`PROJECT_CONTEXT.md:682-686`). |
| `content-identity.contract-digest` — `TaskContract` revision vs digest distinction, `TaskPlan` identity                                                                                                                                                      | `siralos-core::task` (existing R3 kernel extended for plan/contract digest binding — no new orchestration layer)                                                                                           | Contract revision (lifecycle) ≠ digest (material); typed verification                                                                                                                                             |
| `content-identity.manifests` — execution-input / guidance / tool-surface / review-input / acceptance-evidence manifests + digest-bound artifact envelopes                                                                                                    | `siralos-core::identity` + `siralos-core::task::evidence`                                                                                                                                                  | Manifests are digest-bound and deterministic; missing manifest → typed stale                                                                                                                                      |
| `content-identity.delta` — semantic deltas + explicit staleness rules                                                                                                                                                                                        | `siralos-core::identity` (delta derivation)                                                                                                                                                                | Delta reports typed changed surfaces; no conflation with provenance                                                                                                                                               |
| `determinism.replay` — explicit clock/randomness/ordering ports, environment + reproducibility manifests, deterministic validation/acceptance/retry decisions, concurrency normalization, deterministic discovery with ownership index, nondeterminism audit | `siralos-core::determinism` (new module family, host-owned ports) + `siralos-core::identity` for manifest binding                                                                                          | Same authoritative inputs → same host decision; nondeterministic observations recorded or marked `unreplayable` — never silently replayed; Stage-4 entry forbids mapping                                          |
| **Harness fixture:** scenario corpora driven via real `siralos-core` digest/manifest APIs; harness comparison is semantic JSON-path (ADR 0033), not string equality.                                                                                         | —                                                                                                                                                                                                          | —                                                                                                                                                                                                                 |

**Entry-review freeze:** H1/H2 must reuse the single digest architecture; no `SkillHash`/`PluginChecksum` parallel system.

### R10b — ICM (interpretable context / PhaseContract / provenance)

| Differential subject                                                                                                                                                              | Core-owned seam                                                                                                 | What the subject proves                                                                                                                                                  |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `icm.phase-contract` — formal `PhaseContract` with narrowing-only authority (`readOnly ⇒ preparedOnly`, never the reverse), typed `ContextClass`, digest-bound artifact envelopes | `siralos-core::context` (new — PhaseContract, ContextClass registry)                                            | Authority never broadens across phase transitions; digest-bound envelope versioning                                                                                      |
| `icm.dependency-manifests` — deterministic why-diagnostics, provenance chain, targeted incremental staleness                                                                      | `siralos-core::context` + `siralos-core::identity` (manifest binding)                                           | Staleness is content-addressed and phase-targeted; not timestamp-based                                                                                                   |
| `icm.projection` — phase-driven projection, recording-only source-integrity signals                                                                                               | `siralos-core::projection` (extension of R7.3 disposable projection, not replacement) + `siralos-core::context` | Projection reconstructs typed context deterministically; integrity signal never overwrites provenance                                                                    |
| `icm.briefing` — Executor Context Pack, `ExecutorBrief` compiler (revision-stamped, never restates permanent rules), `S3M8`-style manifest discovery                              | `siralos-core::executor` (`executor/brief-compiler.ts` + architecture context index)                            | Brief at revision N references the execution contract by revision; deterministic selection (root + scoped AGENTS.md, index, accepted ADRs — archive/superseded excluded) |

**Dependency:** consumes R10a manifests/digests — its manifests are digest-bound artifacts; without H1 they cannot be named.

### R10c — H3 runtime-readiness

| Differential subject                                                                                                                                                       | Core-owned seam                                                             | What the subject proves                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `runtime-readiness.identity` — causal run identity, `RunManifest`, side-effect policy, run-owned boundaries                                                                | `siralos-core::runtime` (Run identity family)                               | One run → one identity; policy is host-owned, not provider-derived                                                     |
| `runtime-readiness.budgets` — artifact budgets, retention, process supervision limits                                                                                      | `siralos-core::runtime` + `siralos-core::identity` (manifest-bound budgets) | Budgets enforced before execution; exceeded → typed `resource_exceeded`                                                |
| `runtime-readiness.lifecycle` — cancellation, reconciliation, readiness manifest, deterministic fault-injection harness                                                    | `siralos-core::runtime`                                                     | Cancellation is host-observed; reconciliation is deterministic and typed; fault injection is harness-owned, not ad-hoc |
| `runtime-readiness.doctor` — determinism + readiness doctor area, typed failure taxonomy (retryable / non-retryable / denied / stale / resource / unavailable / uncertain) | `siralos-core::doctor` + `siralos-core::runtime`                            | Failures stay distinguishable so recovery never depends on substring matching (lean R10 invariant)                     |

**Dependency:** consumes R10a identity/replay (run identity is a digest) and R10b provenance (doctor reports include PhaseContract provenance) — therefore ordered last.

### Shared seams across R10

- **Digest:** `siralos-core::identity` is the single owner; every other R10 seam depends on it — do not duplicate.
- **Determinism ports:** explicit `Clock`, `Random`, `Ordering` traits owned by core (no implicit `Date.now()`/`Math.random()` in host decisions).
- **Executor briefing:** real manifests S3M8/9/10 (verified) + S3M11 (planned at R10 entry review) via `packages/core/src/executor/brief-compiler.ts` / `ARCHITECTURE.md` executor section.

---

## 3. What stays out of R10 and belongs to R11

R10 is **реplay and provenance**, not **effect enforcement**. The following remain **R11 — Full differential, effect-boundary, security, recovery, and cross-platform parity** and must not be smuggled into R10 (or R10c):

| Belongs to R11                                                                                                                                                                                                                                                                                                                                              | Why it is not R10                                                                                                                                                                                                          | Where it lives when it lands                                                            |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| **Effect-boundary hardening** — workspace create/edit/delete application + safe undo + new checkpoint creation becoming operational (requires the identity-bound directory-relative create/replace/delete primitive that R4 hardening intentionally reports as `unavailable`; see fact sheet `decisions/03-godot-boundaries.md` §2 + `SECURITY.md:210-224`) | R10 records the _intention_ (identity of what changed) and the _evidence_ (manifest of what would be written); it does not promise the write will mechanically succeed. Hardening is a filesystem primitive, not a digest. | `siralos-adapters::workspace` + checkpoint store — new primitive, separate entry review |
| **New private run-directory creation/cleanup** (R11effect)                                                                                                                                                                                                                                                                                                  | R10c's runtime-readiness models budgets and lifecycle, but run boundaries remain reader-facing while the run-directory provider reports `unavailable`.                                                                     | `siralos-adapters::process` / run-directory provider — separate primitive               |
| **Security / sandbox enforcement** — AnthropicSandboxRuntimeBackend host-read allowlist, network/credential denial, process-tree supervision, output-limit termination, descendant confinement, loopback denial (live conformance `npm run test:sandbox` with truthful unavailable reporting)                                                               | Security regressions are measured against the full harness, not against context manifests alone. R10's TypedFailure is necessary but not sufficient — sandbox conformance proves the boundary mechanically.                | `siralos-adapters::sandbox` + conformance suite                                         |
| **Recovery orchestration** (retry with fresh context, reconciliation across failures)                                                                                                                                                                                                                                                                       | R10 typed failures remain distinguishable (retryable/non-retryable/denied/stale/…) so _later_ recovery can decide without substring matching — recovery itself is an R11 workflow, not an R10 contract.                    | `siralos-core::runtime` recovery layer (not due)                                        |
| **Cross-platform parity** — Tier-1 matrix (Linux/Windows/macOS) digest-bound audits, corpus replay stress, performance baselines                                                                                                                                                                                                                            | R10 subjects run differentially, but the cross-platform audit at full-corpus scale (not per-subject) closes R11.                                                                                                           | `tests/differential` + CI matrix                                                        |
| **Stage 4 execution** (controlled runtime, visual evidence, Godot runtime adapter beyond R9, controlled interaction)                                                                                                                                                                                                                                        | Staged product direction (ADR 0036) — Stages 4-6 remain direction subject to evidence, not committed commitments; R10 does not unlock Stage 4.                                                                             | Stage 4 entry after R11 + R12                                                           |

**Lean guardrail:** multi-agent machinery, general Hooks, TaskGraph, workflow engines, marketplaces, automatic Skill/Plugin acquisition, model-router, generic Memory, GUI/TUI ownership remain **Future / Not Due** per `ARCHITECTURE.md` lean constitution (ADR 0036) and must not be introduced in R10 or R11.

---

## 4. Runnable next step — not code, but the next entry review

This decision becomes actionable only after **R7 Verified** (decision 02's promotion) and **R8/R9 entry review + implementation**. The next executable act for R10 is:

1. **R10 entry review** — freeze the five R10 subject families above (H1, H2, ICM, H3) at byte-level contracts with scenario schemas, corpus bump, and measurement plan (mirrors `R7.3 Projection parity` §14 restart).
2. Then **R10a entry review** → implementation → differential parity → **R10b entry review** → … → **R10c** → **R10 Verified** (one promotion commit with corpus audit 133 → new corpus, all gates green).

Wayfinder frontier after this close: `R11 Gate` (now unblocked — blockedBy R10 Scope) becomes frontier; downstream `R12` / `Stage 4` remain blocked pending their parents.

---

## Self-loop verification (this decision)

| Criterion                                             | Direct evidence                                                                                                                                                                                                                                                                                                | Status |
| ----------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| R10 stays single vs split — justified by dependency   | §1 decides one Verified milestone + three ordered sub-slices, with `H1→H2→ICM→H3` table citing `PROJECT_CONTEXT.md:666-691` (each arrow's host dependency) + R7 precedent                                                                                                                                      | pass   |
| Differential subjects + core seams per piece          | §2 tables: 6 subjects for R10a (artifact-digest, contract-digest, manifests, delta, replay), 4 for R10b (phase-contract, manifests, projection, briefing), 4 for R10c (identity, budgets, lifecycle, doctor) — each with `siralos-core::identity/determinism/context/projection/executor/runtime/doctor` owner | pass   |
| What stays out (R11 boundary) so scope does not creep | §3 five-row table (effect-boundary, run-directory, security/sandbox, recovery, cross-platform) + Stage 4 exclusion + lean Future/Not Due guardrail                                                                                                                                                             | pass   |

Evidence ladder: L1 reads of `PROJECT_CONTEXT.md` §14, `RUST_MIGRATION.md` milestone table, `ARCHITECTURE.md`, `identity/` + `determinism/` file listings; L3 porting gate precedent; L4 diff inspection. No code or corpus mutation.

---

## Out of scope for this decision (per lean ADR 0036)

No R10 design doc, no Rust module, no corpus promotion. General Hooks, multi-agent machinery, TaskGraph, workflow engines, marketplaces, plugin ecosystems, model-router, generic Memory, GUI/TUI remain Future / Not Due. Stages 4-6 remain staged product direction, not commitments.
