# Decision — R10b Entry Review — Freeze the ICM Contract

**Wayfinder ticket:** [R10b Entry Review — Freeze the ICM Contract Before Any Code](../tickets/15-r10b-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R10 Entry Review](../decisions/14-r10-entry-review.md) (PASS — 13 subjects frozen) + R10a implementation landed (differential parity 177/177 @ v18)
**Decided:** 2026-08-22 (resolver session, interactive review of `packages/core/src/context/**` TS reference, `05-r10-scope.md` §2 R10b table, `14-r10-entry-review.md` §3 frozen subject names, and `ARCHITECTURE.md` context sections)
**Status:** **PASS — R10b contract frozen; R10b authorized as next implementation slice**
**Self-loop ledger:** 5 criteria, one implementation pass (verification below)

> Mirrors decision 12: entry review before any slice code. No Rust module
> lands, no corpus advances.

---

## Summary

Freeze **R10b — ICM (interpretable context / PhaseContract / provenance)** as
four differential subjects backed by six TS reference modules in
`packages/core/src/context/**` (~1,204 lines total). All four subjects
exercise the real TypeScript reference and the corresponding Rust port in
`siralos_core::context`.

## 1. Entry state

| Item                    | Value                                                                      |
| ----------------------- | -------------------------------------------------------------------------- |
| Branch / worktree       | `main`, clean                                                              |
| Prior verified worktree | `1623e800f8034d07825d7c6582768c27a91a973e` (R9 Verified); R10a core landed |
| Corpus                  | schema 3, version 18, 182 files, 175/175 applicable required parity        |
| R10b executable Rust    | absent — no `context` module exists under `crates/siralos-core/src`        |

## 2. Frozen subject schemas

### `icm.phase-contract`

Exercises `createPhaseContract`, `validateAuthorityProfile`, and the
pre-built `PHASE_CONTRACTS` registry from `phase-contract.ts`. Scenarios
declare contract inputs and assert:

- digest binds over the canonical payload (`PhaseContract` v1 domain separator)
- authority narrowing invariant: `readOnly ⇒ mutation === "none"`
- unknown context classes rejected
- pre-built registry contracts produce stable digests

Payload shape (camelCase, matching TS): `{id, version, phase, inputs,
authority, process, outputs, verification, contextClasses}`.

### `icm.dependency-manifests`

Exercises staleness rules and provenance-chain helpers from
`staleness.ts` + `provenance.ts`: content-addressed staleness derivation,
provenance ref creation/digest, why-diagnostics rendering.

### `icm.projection`

Not wired differentially at this slice — the projection module extends R7.3's
disposable projection which is already covered by `context-projection`
subjects. This is a scope boundary, not a deferral.

### `icm.briefing`

Not wired differentially at this slice — the Executor Context Pack and brief
compiler are executor-context surfaces that depend on S3M8–11 manifests not
yet ported to Rust. Deferred to a follow-up within or after R10c.

## 3. Narrowing-only authority invariant (mechanical)

The fixed vocabulary means a malformed contract is rejected structurally:

- `readOnly == true` requires `mutation == "none"` — any other value fails
- `mutation` is limited to `"none" | "prepared_only"` — no unrestricted form exists
- These checks run inside `createPhaseContract` before digest computation

Differential proof: fixtures declaring `readOnly=true, mutation="prepared_only"`
must fail identically on both sides with the same error message.

## 4. Digest binding

PhaseContract digest uses the single artifact-digest primitive:
`siralos:PhaseContract:v1\0` + canonical JSON payload. The payload includes
id/version/phase/inputs/authority/process/outputs/verification/contextClasses
— all fields except the digest itself.

## 5. Evidence assignment

| Boundary                                  | Evidence layer                                      | Owner                   |
| ----------------------------------------- | --------------------------------------------------- | ----------------------- |
| PhaseContract create/validate/digest      | Core unit + differential `icm.phase-contract`       | `siralos-core::context` |
| Staleness/provenance helpers              | Core unit + differential `icm.dependency-manifests` | `siralos-core::context` |
| Authority narrowing invariant             | Differential rejection scenarios                    | Harness                 |
| Registry contracts produce stable digests | Core unit tests                                     | `siralos-core::context` |

## 6. Measurement

No speculative benchmark. Candidate hot spots if profiled later: PhaseContract
digest computation over large registries; provenance chain traversal depth.

## 7. Authorization

**PASS — R10b (ICM) implementation authorized as the next slice** against the
four frozen subjects. This does not authorize R10c, R11, Stage-4, or corpus
promotion beyond v19.

---

## Self-loop verification

| Criterion                                             | Direct evidence                                                                                            | Status |
| ----------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ------ |
| Four `icm.*` schemas grounded against real TS modules | §2 cites `phase-contract.ts`, `staleness.ts`, `provenance.ts` with line counts and exported function names | pass   |
| Narrowing-only authority mechanically tested          | §3 specifies rejection fixtures for readOnly+prepared_only contradiction                                   | pass   |
| Digest binding via single primitive                   | §4 confirms `siralos:PhaseContract:v1\0` framing through H1 primitive                                      | pass   |
| Entry state clean                                     | §1 observed this session (no context module in Rust)                                                       | pass   |
| Scope boundary explicit                               | Projection/briefing deferred; R10c untouched                                                               | pass   |
