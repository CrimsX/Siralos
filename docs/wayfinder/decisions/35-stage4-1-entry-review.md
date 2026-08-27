# Decision — Stage 4.1 Entry Review — Freeze the Generic Controlled Runtime Contract

**Wayfinder ticket:** [Stage 4.1 Entry Review](../tickets/35-stage4-1-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [Stage 4.1 + Godot Extraction Contract](34-stage4-1-generic-runtime-and-godot-plugin-extraction.md) (PASS, Stage 4.1 authorized) + [R12 Disposition Execution](33-r12-disposition-execution.md) (PASS, retired)
**Decided:** 2026-08-27 (resolver session, interactive HITL grilling over `ARCHITECTURE.md` Host authority, `docs/adr/0035` domain-neutral boundary, `docs/development/RUST_MIGRATION.md:836` 4.1→4.7, `crates/siralos-core/src/runtime` readiness contracts, and `crates/siralos-adapters/src/process` fail-closed runners)
**Status:** **PASS — Stage 4.1 contract frozen; authorized as next implementation slice**
**Self-loop ledger:** 4 criteria, one decision pass (verification below)

> Mirrors `decisions/14-r10-entry-review.md` (one milestone, ordered entry-reviewed sub-slices). No implementation lands here.

---

## Summary

Stage 4.1 ports the **generic host-authorized Controlled Runtime Execution** (`siralos-core::runtime` + `siralos-adapters::runtime`) from the TypeScript reference's `unavailable` posture to the **first `available` behind identity-bound handles** (`openat`/`renameat`, `exec-by-handle`, `delete-by-handle`). It is _not_ a Godot launcher; the Godot Runtime Adapter (4.3) will later consume this generic boundary as its first specialization.

## 1. Frozen subject structure

| Slice                   | Scope                                                                                                                                                                                                               | Frozen differential subjects                  | Depends on                                                                                                   | Approx. size  |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------------------------------ | ------------- |
| **4.1** Generic runtime | Host-authorized process supervision with bounded structured `RuntimeEvidence` (exit code, duration, stdout/stderr bounded at 1 MiB each, truncated flag, `runId`/`operationId` causal, artifact digests, staleness) | `runtime-execution` ×6, `runtime-evidence` ×4 | `siralos-core::runtime` readiness (`identity/budgets/supervision/doctor`) + `siralos-core::identity` digests | ~12 src files |

Rationale: `runtime-execution` (6) covers the host decision table (success, `COMMAND_DENIED` via `process.execute` capability, `STALE` via revision, `RESOURCE_EXCEEDED` via `RuntimeBudget`, `CANCELLED` via `CancellationSignal`, `UNAVAILABLE` when handle primitive absent) while `runtime-evidence` (4) covers the bounded evidence projection (`exitCode`/`durationMs`/`truncated`/`artifactDigest`).

## 2. Corpus mechanics

- Schema stays `3`; corpus bumps `v32` at the 4.1 reconciliation commit; `subject` names above are frozen now, scenario-level fixtures are owned by the 4.1 implementation (mirrors `14-r10-entry-review.md`).
- Determinism: one injected clock + `NowFn` per run, one `RunId`/`OperationId` per `siralos_core::runtime::identity`, bounded `RuntimeEvidence` (SHA-256 per artifact, `candidateRecordsSha256` style), no wall clock, no ambient env, sanitizer as single output boundary.
- Fail-closed carry-forward: if the identity-bound primitive is absent on a platform, `runtime-execution` reports typed `unavailable` (`UNAVAILABLE: identity-bound launch primitive not available`) without mutation, launch, or cleanup — same posture as `workspace-apply`/`godot-probe` before it; `stage4-entry-gate.md` 1,3–6 remain `PASS` because the _contract_ is now available, the _primitive_ may still be platform-specific.

## 3. Boundaries — not in 4.1

- No Godot-specific logic, no `project.godot` scan, no `godot-knowledge`/`godot-*.` beyond what `4.3` will consume
- No visual evidence, controlled interaction, QA workflows, or profiling (4.4–4.7)
- No marketplace, no `siralos-godot` crate move (frozen in decision 34, not this slice)
- No new capability beyond `process.execute` (already `deny` by default, `ask` via `PermissionPolicy`)

## 4. Authorization

**Stage 4.1 is authorized as the next implementation slice** against this contract. `crates/siralos-godot` extraction remains **frozen but not authorized** until 4.1 is `Verified`; `Add Plugin` UI remains **frozen but not authorized** until the crate extraction is `Verified`.

---

## Self-loop verification

| Criterion                                    | Direct evidence                                                                                                                                                                                                  | Status |
| -------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Cases exercise reference-observable behavior | `process/command_runners.rs:22` `node_script_is_available() → false` (fail-closed) and `godot-knowledge` `unavailable` read this session; `runtime/supervision.rs` pure transition table (13-kind taxonomy) read | pass   |
| Determinism posture preserved                | §2 injects one clock/RunId per run, bounded `RuntimeEvidence`, no wall clock/TTY                                                                                                                                 | pass   |
| Ordering generic-first preserved             | §1 `runtime-execution` (generic) before `runtime-evidence` (structure) before 4.3 Godot adapter per decision 34 §3 DAG                                                                                           | pass   |
| Human decided the material cuts              | HITL answers 2026-08-27: 6+4 split, v32, `process.execute` only, `unavailable` when primitive absent                                                                                                             | pass   |
