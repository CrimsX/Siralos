---
id: ADR-0027
status: accepted
domains: [godot, workflow, mutation, task-runtime, security]
paths:
  [
    packages/core/src/godot/development/development-surface.ts,
    packages/core/src/godot/development/unified-change-set.ts,
    packages/core/src/godot/development/unified-order.ts,
    packages/core/src/godot/development/cross-surface-consistency.ts,
    packages/core/src/godot/development/blocked-disposition.ts,
    packages/adapters/src/godot/development/unified-development-service.ts,
    packages/core/src/executor/s3m11-manifest.ts,
  ]
supersedes: []
---

# ADR 0027 — Unified Godot-Native Development Workflow

- Status: accepted (Stage 3 milestone 11)
- Date: current milestone
- Related: ADR 0012 (GDScript development and repair loop), ADR 0013
  (quality gates and independent review), ADR 0020 (host-controlled
  planning), ADR 0025 (impact intelligence), ADR 0026 (approved
  scene/resource mutation), ADR 0022 (executor briefing and milestone
  acceptance)

## Problem

Stage 3 had two mutation surfaces — the GDScript exact-text change-set
loop (Stage 2/ADR 0012) and the structured native scene/resource
mutation path (Stage 3.10/ADR 0026) — but no way to execute a bounded
development request that needs both (change `player.gd` + configure
`player.tscn`), and no host-owned routing between the surfaces. A mixed
task must keep per-target revision, approval, checkpoint, and
verification semantics without inventing a second workflow engine.

## Decision

Complete the Stage 3 development loop with one host-owned unified
workflow that composes the existing surfaces:

- **Surface routing** (`development-surface.ts`): the host classifies a
  request as `script_only | native_only | mixed | none` from
  host-observed evidence (request signals, verified/candidate
  touchpoints, project surface inventory) — never from model claims.
  The routing is preliminary at planning time (request signals only)
  and re-derived from the actual prepared target paths at change-set
  preparation time. Mixed tasks route to full planning
  (`mixed-surface-relationships`).
- **Unified multi-target change set** (`unified-change-set.ts`): one
  bounded change set containing text targets and/or prepared native
  mutations. Every target retains its own source revision, prepared
  fingerprint, approval state, and verification slot; a combined digest
  binds the exact prepared batch. The raw text boundary is unchanged —
  `.tscn`/`.tres` paths remain refused at the provider-facing text
  change-set boundary (ADR 0026 invariant).
- **Derived apply order** (`unified-order.ts`): explicit cross-target
  dependency edges (script attachments, resource references resolved
  from current documents) are topologically sorted with a deterministic
  path tie-break; "scripts first" / "scenes first" is never hardcoded.
  The rationale is recorded on the change set.
- **One checkpoint-then-apply batch** (`unified-development-service.ts`):
  the combined approval binds the exact batch; every target's pre-state
  is revalidated before any write; one mutation lock and per-file
  checkpoints precede sequential hash-verified application. An
  externally changed target blocks the whole batch — no target is
  mutated under a stale approval.
- **Per-surface verification**: GDScript targets verify through the
  check-only parser gate and a fresh LSP session; native targets
  through reparse and semantic-effect verification. A mutation step is
  not successful until its required verification passes; a gate that
  cannot run reports `infrastructure_failure`, never success.
- **Cross-surface consistency** (`cross-surface-consistency.ts`): after
  a mixed apply, script attachments resolve to existing scripts, signal
  targets resolve structurally, resource references keep valid
  identities, and script/scene pairs changed together disclose the
  runtime-only compatibility limitation honestly. Disclosures never
  fail consistency; concerns do.
- **Impact-driven validation and review**: post-change impact is
  re-derived over the actual changed surfaces and feeds the existing
  ReviewContextManifest and the read-only independent reviewer. The
  reviewer context stays bounded: contract, acceptance criteria, the
  actual diff/changeset, semantic evidence, manifest, validation
  evidence.
- **Bounded repair**: blocking Critical/High findings enter the
  existing bounded repair loop; repairs prepare fresh mutations from
  current revisions with new approvals. Stale prepared artifacts and
  approvals are structurally unusable (fingerprint/TTL/one-time
  consumption). Repair budgets come from the existing task/progress
  budgets — no new retry controller.
- **Structured blocked dispositions** (`blocked-disposition.ts`):
  unsupported requirements end honestly as `blocked` with a concrete
  reason; successful prior changes are preserved (never auto-reverted
  by a later review failure).
- **Acceptance and undo**: completion requires host-observed evidence —
  an executor completion claim alone cannot complete (TaskState gate +
  AcceptanceEvaluator). Checkpoints created before mixed batches keep
  undo semantics valid for every surface.

The identity-bound apply gate is unchanged: production primitives fail
closed (`canApplyIdentityBound: false`) before any lock, checkpoint, or
write; the full loop is real, tested machinery exercised through
injected in-memory primitives in the behavior harness.

## Explicit rejections

- **Model-controlled workflow state** — TaskState owns progress; the
  model only proposes.
- **One giant mutation tool** — the unified change set composes the
  existing exact-text and structured-native surfaces; no monolithic
  mutation primitive was added.
- **Raw `.tscn`/`.tres` text-edit fallback** — never; native mutation
  stays structural and prepare-only on the provider surface.
- **Plan approval as mutation approval** — plan approval binds only the
  exact plan revision (ADR 0020); every mutation batch needs its own
  exact approval.
- **Review without fresh evidence** — the reviewer always receives the
  current diff, current semantic evidence, and the derived impact
  manifest.
- **Executor assertion as acceptance** — completion requires
  host-observed evidence.
- **Permanent accumulation of all task source/context** — phase-specific
  bounded context is preserved (ADRs 0022–0024).
- **Generic workflow-engine abstraction** — no second engine; the
  unified workflow composes the existing owners (planning policy,
  approval subsystem, checkpoint store, revision registry, mutation
  services, impact analyzer, reviewer, acceptance evaluator).

## Consequences

- `/develop` can execute script-only, native-only, and bounded mixed
  Godot tasks with the same authorization, checkpoint, verification,
  review, and acceptance discipline.
- Mixed tasks carry surface-conditional acceptance criteria
  (`native-verified`, `cross-surface-consistent`) so script-only tasks
  are never blocked by native gates.
- Tool projection exposes native prepare tools only when the host-derived
  surface requires them; planner and reviewer surfaces stay read-only.
- Production mutation application remains intentionally unavailable
  until an identity-bound commit primitive exists; availability is never
  claimed.
