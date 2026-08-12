---
id: ADR-0031
status: accepted
domains: [runtime, security, task-runtime]
paths: [packages/core/src/runtime/**, packages/core/src/doctor/capability-doctor.ts]
supersedes: []
---

# ADR 0031 — Runtime Readiness and Operational Resilience

- Status: accepted (Stage 3 — Runtime Readiness & Operational Resilience,
  final hardening milestone before Stage 4)
- Date: current milestone
- Related: ADR 0028 (content identity), ADR 0029 (determinism and
  reproducibility), ADR 0030 (interpretable context architecture), ADR
  0004 (sandbox/permission boundary), ADR 0007 (sandboxed command
  runners)

## Problem

Stage 4 will need to safely control and observe Godot runtime execution
under a sandbox. None of the host infrastructure for that boundary
existed: no causal run identity, no would-be-run manifest, no side-effect
policy or run-owned filesystem model, no artifact lifecycle or budgets,
no typed failure taxonomy, no supervision contract, no cancellation or
restart-reconciliation semantics, and no fail-closed readiness gate.
This milestone builds everything around the future execution box —
without launching real project/runtime Godot execution.

## Decision

- **Causal run identity** (`runtime/identity.ts`): hierarchical
  correlation `TaskId → PhaseId → RunId → OperationId/ProcessId →
Evidence/Artifact` with deterministic ids and `RunTraceRef` on
  evidence/artifacts. Causal correlation, not distributed tracing.
- **RunManifest** (`runtime/modes.ts`): immutable H1-digest-bound
  description of what WOULD be run (contract/plan/execution-input/
  reproducibility digests, Godot fingerprint, project identity, runtime
  mode, sandbox profile, side-effect policy, resource budget,
  environment identity). H3 never executes it.
- **Runtime modes**: explicit `headless | visual` capability dimensions;
  visual is never assumed from Godot availability (display and sandbox
  support are explicit requirements).
- **RuntimeSideEffectPolicy** (`runtime/side-effects.ts`): host-owned —
  source workspace always `protected` (never authorizes source mutation),
  disposable runtime workspace `runtime_mutable`, user-data redirected
  and run-owned, temp files run-owned, network explicit, child processes
  supervised, environment allowlisted. Policy may narrow, never broaden,
  sandbox/security authority.
- **Run-owned filesystem boundaries**: host-resolved roots with
  containment-checked path resolution (`..`, absolute, drive-qualified
  paths rejected); cleanup operates only on host-owned run roots and is
  bounded, run-scoped, and idempotent; cleanup failure is observable and
  never hides the primary result.
- **Artifact model** (`runtime/artifacts.ts`): reference-only
  `RuntimeArtifactRef` (id, digest, runId, kind, mediaType, size,
  producer, createdAt, retentionClass, location); large/binary contents
  never enter TaskState/ActivityLog/ExecutorBrief/model context.
  Deterministic budgets: truncate with explicit metadata, stop capture,
  or produce an `artifact_limit` outcome — evidence is never silently
  dropped while claiming complete capture. Retention classes:
  ephemeral/task/diagnostic/retained.
- **Failure taxonomy** (`runtime/supervision.ts`): one typed 13-kind
  taxonomy (readiness_failed, spawn_failed, sandbox_denied,
  startup_timeout, idle_timeout, hard_timeout, cancelled,
  process_crashed, kill_failed, output_limit, artifact_limit,
  environment_unavailable, cleanup_failed) with distinct terminal
  dispositions (success / failure / cancelled / resource_limit /
  uncertain); non-zero outcomes are never collapsed into one
  process_failed. `RunOutcome` has exactly one terminal execution
  disposition plus an independent cleanup status.
- **Process supervision contract**: pure deterministic state machine
  `prepared → starting → running → terminating → terminal` driven by
  typed observations under the H2 controlled clock; terminal is
  absorbing so "process exited but supervisor remains running" is
  structurally impossible. Startup/idle/hard timeouts remain distinct;
  liveness is mode-aware (no-stdout ≠ hung by default).
- **RuntimeBudget** (`runtime/budget.ts`): typed budget covering
  startup/idle/hard timeouts, stdout/stderr bytes, artifact bytes/count,
  child-process count; memory/CPU appear only when the backend can
  enforce or reliably observe them — unsupported limits are exposed as
  capability state, never pretended enforced.
- **Cancellation**: deterministic and idempotent — repeated requests
  return the same single cleanup flow; semantics are stop accepting new
  work → bounded termination → terminate owned children → finalize
  evidence → cleanup run-owned state → terminal cancelled outcome.
- **Restart reconciliation** (`runtime/budget.ts`):
  `classifyIncompleteRun` conservatively produces
  interrupted/unknown/cleanup_required; Solaris restart never implies the
  external process is gone, and success is never fabricated. No durable
  job system.
- **Fault-injection harness** (`runtime/faults.ts`): deterministic fake
  process drivers for 14 scenarios (normal, spawn failure, sandbox
  denial, startup hang, idle hang, hard timeout, cancellation during
  startup/running, crash, child refusing termination, output flood,
  artifact quota, cleanup failure, restart with incomplete run state)
  under the controlled clock — no real Godot project needed.
- **RuntimeReadinessManifest** (`runtime/readiness.ts`): deterministic
  fail-closed evaluation (supported/available/configured/degraded/
  blocked/unsupported) over Godot executable+fingerprint, project
  identity, sandbox backend, process supervision, filesystem and
  user-data isolation, network policy, artifact storage, headless/visual
  mode, display, and resource-limit capabilities. If the requested mode
  requires an isolation property that cannot be provided, readiness is
  blocked and no execution request can proceed — security is never
  silently downgraded. The evaluator never executes Godot and does not
  duplicate CapabilityDoctor semantics.
- **CapabilityDoctor**: a new non-mutating `readiness` area reports
  headless/visual availability, supervision, and artifact capture
  without launching the project.
- **Phase contract**: a future `runtime_execution` PhaseContract
  (inputs: RunManifest + RuntimeReadinessManifest; authority: exact
  runtime capability only; outputs: RunOutcome + artifacts/evidence;
  verification: terminal process state, artifact bounds, cleanup result)
  is described for Stage 4 consumption — it grants no authority.

## Explicit distinctions

```text
run identity ≠ task identity
process exit ≠ acceptance
artifact ≠ model context
runtime side effects ≠ source mutation authority
readiness ≠ execution
```

## Explicit rejections

- Launching directly from the source workspace; uncontrolled user-data
  writes; unlimited stdout/artifacts; one generic process-failed result;
  cleanup using model-supplied arbitrary paths.
- Real Godot execution in H3; screenshots/visual QA; keyboard/mouse
  automation; performance profiling; a runtime scenario DSL; remote
  workers; containers solely for this milestone; a new sandbox backend;
  a general telemetry platform; an artifact content-addressed blob
  store; a generic job scheduler.
- Binary artifacts in TaskState/transcript; giant observability
  frameworks; premature Stage 4 execution behavior.

## Consequences

- Stage 4 Controlled Godot Execution consumes TaskContract +
  runtime PhaseContract + RunManifest + RuntimeReadinessManifest +
  RuntimeSideEffectPolicy + RuntimeBudget → controlled execution →
  RunOutcome → runtime evidence/artifacts, host-controlled throughout.
- Determinism is preserved across the execution boundary: equivalent
  manifests + supervisor observations + controlled clock produce
  equivalent host outcomes (proven by the fault-injection suite).
- H1/H2 identity and determinism primitives are reused; nothing is
  duplicated.
