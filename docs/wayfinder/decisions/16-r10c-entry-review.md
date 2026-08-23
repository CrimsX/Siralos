# Decision — R10c Entry Review — Freeze the H3 Runtime-Readiness Contract

**Wayfinder ticket:** [R10c Entry Review — Freeze the H3 Runtime-Readiness Contract Before Any Code](../tickets/16-r10c-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R10b Entry Review](../decisions/15-r10b-entry-review.md) (PASS) + R10b implementation landed (differential parity 190/190 @ corpus v19, commits `5fe5361..a2269cd`)
**Decided:** 2026-08-22 (resolver session, interactive review of `packages/core/src/runtime/**` TS reference, `05-r10-scope.md` §2 R10c table, `14-r10-entry-review.md` §3 frozen subject names, and the Rust entry state at HEAD)
**Status:** **PASS — R10c contract frozen; R10c authorized as the next implementation slice**
**Self-loop ledger:** 5 criteria, one implementation pass (verification below)

> Mirrors decisions 12/15: entry review before any slice code. No Rust
> module lands, no corpus advances.

---

## Summary

Freeze **R10c — H3 runtime-readiness parity (deterministic core)** as four
differential subjects backed by the nine TS reference modules in
`packages/core/src/runtime/**` (~1,545 lines total). All four subjects
exercise the real TypeScript reference and the corresponding Rust port in a
new `siralos_core::runtime` module family. Everything modeled is
host-owned identity, budget, lifecycle, taxonomy, and readiness semantics;
no real process is ever launched (that remains R11/fail-closed).

## 1. Entry state

| Item                    | Value                                                                                                                                                                                  |
| ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Branch / worktree       | `main`, clean                                                                                                                                                                          |
| Prior verified worktree | `1623e800f8034d07825d7c6582768c27a91a973e` (R9 Verified); R10a + R10b core landed                                                                                                      |
| Corpus                  | schema 3, version 19, 195 files, 190/190 applicable required parity                                                                                                                    |
| R10c executable Rust    | absent — no `runtime` module exists under `crates/siralos-core/src`; H1/H2/ICM seams already in place (`siralos_core::identity`, `siralos_core::determinism`, `siralos_core::context`) |

## 2. Frozen subject schemas

### `runtime-readiness.identity`

Exercises `createRunId`, `createOperationId`, `createRunTraceRef`, and
`formatRunTraceRef` from `identity.ts`. Scenarios declare identity inputs
and assert:

- deterministic causal ids: `run_<kind>_<24hex>` over the
  `RunId v1` domain separator (`taskId`, `phaseId`, `sequence`, `kind`),
  `op_<24hex>` over `OperationId v1` (`runId`, `operation`)
- empty task/phase id and zero sequence rejected with oracle messages
- trace refs round-trip through the bounded formatter; a null
  operation renders without an `op=` segment

### `runtime-readiness.budgets`

Exercises `createRuntimeBudget`, `renderRuntimeBudget`, and the artifact
budget admission path (`enforceArtifactBudget`) from `budget.ts` +
`artifacts.ts`. Scenarios assert:

- default budget values and the digest bound over the canonical
  `RuntimeBudget v1` payload (single-digest primitive)
- rendered projection matches byte-for-byte (null memory/cpu segments
  omitted)
- over-budget artifacts are admitted or denied deterministically with
  typed outcomes — `resource_limit` surface, never a silent clamp

### `runtime-readiness.lifecycle`

Exercises the supervisor state machine and outcome model from
`supervision.ts` plus the fault-injection harness from `faults.ts`.
Scenarios drive FakeProcessDriver scripts under a fixed clock and assert:

- state machine ordering `prepared → starting → running → terminating → terminal`
- exactly one terminal disposition (`success | failure | cancelled |
resource_limit | uncertain`) plus an independent cleanup status
- the 13-kind failure taxonomy is preserved verbatim
- same FaultScript + same clock ⇒ same observation sequence and the
  same RunOutcome (H2 replay inside H3)

### `runtime-readiness.doctor`

Exercises `evaluateRuntimeReadiness`, `executionAllowed`,
`renderRuntimeReadiness` (`readiness.ts`), and
`buildRuntimeReadinessDiagnostic` (`doctor.ts`). Scenarios toggle
declared capabilities and assert:

- readiness manifests are deterministic per mode (headless vs visual)
- unsupported limits appear as capability state (`memory: false,
cpu: false`), never pretended enforcement
- execution is allowed only when every blocking item passes; rendered
  projections match byte-for-byte

## 3. Harness-owned determinism invariant (mechanical)

Fault injection is harness-owned, not ad-hoc:

- `FAULT_SCRIPTS` is a closed 14-script vocabulary; unknown scripts fail
  typed at parse
- observations are pure functions of `(script, nowMs, requested)` under
  the injected fixed clock — no wall-clock, no real process spawn
- differential fixtures declare script + clock steps + requests; both
  implementations must produce identical observation sequences

## 4. Digest binding

Run ids, operation ids, and budgets bind through the single
artifact-digest primitive with domain separators `siralos:RunId:v1\0`,
`siralos:OperationId:v1\0`, and `siralos:RuntimeBudget:v1\0` over
canonical JSON payloads. No parallel hash family may be introduced
(`check:architecture` guardrails stay authoritative).

## 5. Evidence assignment

| Boundary                                      | Evidence layer                                          | Owner                                   |
| --------------------------------------------- | ------------------------------------------------------- | --------------------------------------- |
| Run/operation identity + trace formatting     | Core unit + differential `runtime-readiness.identity`   | `siralos-core::runtime`                 |
| Budget defaults/digest/render/admission       | Core unit + differential `runtime-readiness.budgets`    | `siralos-core::runtime`                 |
| Supervision lifecycle + fault reproducibility | Core unit + differential `runtime-readiness.lifecycle`  | `siralos-core::runtime`                 |
| Readiness manifest + doctor diagnostic        | Core unit + differential `runtime-readiness.doctor`     | `siralos-core::runtime` (+ doctor area) |
| Real spawn/sandbox enforcement                | **Out of scope — R11**; nothing here launches a process | R11                                     |

## 6. Measurement

No speculative benchmark. Candidate hot spots if profiled later:
supervisor transition table traversal under long fault scripts; artifact
admission accounting for large registries.

## 7. Authorization

**PASS — R10c (H3 runtime-readiness) implementation is authorized as the
next slice** against the four frozen subjects above. This completes the
R10 sub-slice chain (`R10a → R10b → R10c`): after R10c lands with
evidence, the single R10 Verified promotion closes the milestone; R11 and
Stage-4 remain untouched by this decision.

---

## Self-loop verification

| Criterion                                      | Direct evidence                                                                                       | Status |
| ---------------------------------------------- | ----------------------------------------------------------------------------------------------------- | ------ |
| Four subject schemas grounded against real TS  | §2 cites all nine `runtime/**` modules with line counts and exported entry points                     | pass   |
| Fault injection harness-owned and reproducible | §3 specifies the closed script vocabulary + pure-function observation rule under the controlled clock | pass   |
| Single-digest binding preserved                | §4 pins three domain separators through the existing primitive; no new hash family                    | pass   |
| Entry state audited                            | §1 observed this session (clean tree; corpus v19/195; no `siralos_core::runtime` module)              | pass   |
| Authorization limited to R10c                  | §7; R11 effect-boundary/security/recovery explicitly out of scope                                     | pass   |

Evidence ladder: L1 observed entry state + TS module inventory with line
counts; L2 file:line citations (`supervision.ts:19-51`, `budget.ts`,
`identity.ts`, `faults.ts:1-52`, `readiness.ts`, `doctor.ts`);
L3 porting-gate precedent (decisions 10/12/14/15); L4 this decision.
