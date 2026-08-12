---
id: ADR-0014
status: accepted
domains: [task-runtime, evidence]
paths: [packages/core/src/tasks/**]
supersedes: []
---

# ADR 0014 — Host-owned structured Task Runtime

- Status: accepted (Stage 3 milestone 1)
- Date: current milestone
- Related: ADR 0012 (GDScript development workflow), ADR 0013 (quality gates and independent review)

## Context

Up to Stage 2, Siralos's working state lived in the model conversation plus a
collection of application-owned subsystem states (the development workflow
session, quality reports, checkpoints, the command registry). That is
sufficient for a single interactive flow, but it has structural limits:

- **Conversation history is not state.** The provider's transcript is
  untrusted, lossy, and private to the provider; it cannot serve as the
  authoritative record of what a task requires, what has been verified, or
  what remains.
- **No explicit completion contract.** Nothing distinguishes what the user
  requested from constraints, acceptance criteria, and pause conditions.
  "Done" is whatever the model's final text claims.
- **No evidence discipline.** A step or task can be declared finished purely
  by assertion; nothing binds completion to host-observed artifacts
  (mutation receipts, checkpoint ids, parser results, LSP results, review
  results).
- **No host-observed progress.** Nothing distinguishes productive execution
  from loops of identical failed commands or repeated searches, because
  progress is not tracked as host-observed state.
- **No reproducibility snapshot.** A running workflow is implicitly bound to
  whatever the global configuration happens to be at any moment; ordinary
  config changes silently alter in-flight work.
- **No audit trail.** There is no typed, append-only record of task-relevant
  runtime events.
- **No single-owner discipline.** Several components could plausibly keep
  competing copies of "the task" (CLI, provider, workflow), which is how
  duplicated mutable state and drift begin.

Future milestones (context projection, planning, scene/resource development,
multi-agent orchestration, persistence, `/evolve`) require a structured,
revisioned, evidence-aware task foundation.

## Decision

Introduce a host-owned structured task runtime in `packages/core/src/tasks/`
(Stage 3 milestone 1):

- `TaskContract` — revisioned, immutable contract distinguishing request,
  constraints, individually addressable acceptance criteria
  (`deterministic | review | user`), and pause policy.
- `TaskState` — authoritative, serializable working state: phase, bounded
  steps, acceptance states, evidence-backed findings, validation/review
  status, iteration, host-observed progress. Never contains chain-of-thought,
  provider continuation internals, secrets, or raw adapter output.
- `TaskRuntime` — the single owner of every mutable `TaskState`. State is
  closure-private; every other component receives immutable snapshots,
  projections, or events. Mutation happens only through the handle API
  (`transitionPhase`, `beginStep`, `completeStep(stepId, evidenceRefs)`,
  `attachEvidence`, `verifyCriterion`, `submitDisposition`,
  `evaluateCompletion`, `completeTask`, `cancel`, `fail`, `markBlocked`,
  `observe`).
- Evidence-backed step completion: a step completes only when the referenced
  evidence exists, belongs to the task, and matches the step's declared
  evidence kinds. Read-only/research steps accept read/lookup evidence; there
  is no hard-coded "every step needs a mutation".
- Structured `WorkflowDisposition` (`continue | complete | blocked`): a
  request, not a mutation. A model-issued `complete` is a completion request
  that still passes the host completion gate (steps completed, criteria
  satisfied, validation/review clean, no unresolved critical/high findings).
- Host-observed progress with deterministic stuck-pattern detection
  (`healthy | degraded | stalled`) over a bounded window of canonicalized
  action+result observations.
- Immutable `TaskRuntimeSnapshot` captured at task start (runtime version,
  provider profile id, sandbox profile id, capability-policy fingerprint,
  workspace identity, Godot engine fingerprint, workflow identity and
  prepared-operation digest). Ordinary global config changes affect future
  tasks only.
- Typed append-only `TaskActivityEvent` log (per-task deterministic
  sequences, host timestamps) for auditability, debugging, future
  persistence, UI projection, and behavior tests — deliberately not event
  sourcing and not a generic event bus.
- Single-owner architecture enforcement: core task modules must not import
  provider ports, sandbox implementations, or Godot modules (the development
  bridge and the generic digest utility are the only exceptions); provider
  adapters must not import the task runtime surface.
- Deterministic behavior fixtures (`tests/behavior/`) covering the required
  behaviors at the final observable boundary, including the full Stage 2
  `/develop` loop through the task gate.

The current `/develop` workflow is integrated without rewriting it: request →
`TaskContract` → `TaskState` → existing development workflow → existing
validation/review → host completion gate. Stage 2 quality gates remain
authoritative; infrastructure failures stay `validation_incomplete`, never
criterion failure and never success.

## Alternatives rejected

- **State only in the model conversation** — untrusted, private to the
  provider, lossy, and incapable of host verification.
- **Generic workflow engine / workflow DSL** — premature abstraction; the
  milestone needs a bounded task foundation, not a configurable engine.
- **Pure event sourcing** — the authoritative state is a materialized
  object; the activity log is an audit/projection record, not a
  reconstruction source.
- **Letting the provider mark tasks complete directly** — a model-issued
  `complete` is a request that passes the host gate; provider text never
  mutates state.
- **Duplicate mutable state in CLI/workflow/provider** — the single-owner
  rule and architecture enforcement forbid competing copies.
- **Requiring mutation evidence for every task type** — read-only
  research/review work completes on read/review evidence instead.
- **Generic publish/subscribe event bus** — rejected; the activity log is
  local, typed, and explicit.

## Consequences

- Stronger reliability: completion is host-verified against explicit
  criteria and evidence; model assertions are never authoritative.
- More explicit architecture: one owner per authoritative domain, enforced
  structurally where reasonable.
- Easier future compaction/resume: revisioned contracts, serializable state,
  append-only activity records, and the immutable snapshot give later
  milestones what they need to reconstruct and continue work.
- Modest additional state-management complexity: the runtime, gate, evidence
  validation, progress tracking, and the development bridge are ~1.4k lines
  of tested core code; behavior fixtures make regressions visible.
- Task state is descriptive/control-flow state: it can never grant
  capabilities. Security policy, approvals, sandboxing, and checkpoints
  remain authoritative elsewhere and are unchanged by this milestone.

### Persistent-state schema/versioning requirements (future work)

TaskState may remain in memory in this milestone. When persistence lands it
must: (1) record `TaskRuntimeSnapshot.runtimeVersion` and the contract
revision to reject incompatible state; (2) persist the immutable contract
revision history, not only the current revision; (3) persist the append-only
activity log separately from the materialized state; (4) keep evidence as
references to already-owned artifacts (no raw output); (5) refuse to load
state that embeds runtime handles or secrets. No SQLite is introduced for
this milestone.

### Approval revision groundwork (immediate follow-up)

TaskContract revisions are now first-class. The immediate follow-up is to
bind mutation approvals to `(task contract revision, prepared operation
digest)` so an approval cannot outlive the contract revision it was granted
against; the current approval flow (exact one-time approval bound to the
prepared digest) is unchanged by this milestone.
