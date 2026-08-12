---
id: ADR-0030
status: accepted
domains: [context, task-runtime, security]
paths: [packages/core/src/context/**, apps/cli/src/output/context.ts]
supersedes: []
---

# ADR 0030 — Interpretable Context Architecture

- Status: accepted (Stage 3 — Interpretable Context Architecture, H2
  extension)
- Date: current milestone
- Related: ADR 0028 (content identity and semantic deltas), ADR 0029
  (deterministic execution and reproducibility), ADR 0014 (task runtime),
  ADR 0015 (context/tool/evidence projection), ADR 0020 (host-controlled
  planning), ADR 0022/0023 (executor briefing and scope discipline)

## Problem

Major Solaris workflow phases communicated partly through accumulated
conversation history rather than typed, inspectable, hash-bound
artifacts, and per-phase context/authority were implicit (projection
modes, stability classes) rather than declared. The system should rely
less on transcript history and more on structured artifacts, with every
phase able to answer deterministically: what context does it need, what
authority does it have, what does it consume/produce, how is the output
verified, and what downstream artifacts depend on it.

## Decision

Adapt the useful principles of Interpretable Context Methodology (ICM)
to Solaris's typed, host-controlled architecture — never literally:

- **Formal context classes** (`context/phase-contract.ts`): `global`,
  `routing`, `phase_contract`, `stable_reference`, `working` with a
  bounded artifact-kind vocabulary. No phase defaults to repository-wide
  context; additional context remains available through bounded
  discovery (3.7A/3.7B).
- **PhaseContract**: a typed contract per major workflow phase
  (planning, inspection, preparation, approval, mutation, verification,
  validation, impact, review, repair, acceptance) declaring inputs,
  a **fixed-vocabulary authority ceiling** (read-only / prepared-only
  mutation / approval grant / acceptance authority — narrowing only),
  operations, outputs, and verification, with an H1 digest. It is not a
  state machine: TaskState remains authoritative for workflow progress.
  A malformed contract (e.g. a review contract demanding unrestricted
  mutation) is rejected structurally and can never broaden runtime
  capability; ToolProjector/security enforcement remain authoritative.
- **Typed intermediate artifacts** (`context/artifacts.ts`):
  `WorkflowArtifactIdentity` (artifactType, schemaVersion, revision,
  digest, producedUnder) references existing artifacts — no generic
  payload type. Major phases communicate through typed artifacts, not
  transcript history; conversation history may support execution but is
  never authoritative workflow state.
- **ArtifactDependencyManifest**: explicit high-value dependencies for
  TaskPlan, ReviewVerdict, AcceptanceResult, and PreparedChangeset using
  H1 digests, plus bounded lineage rendering. No generic dependency
  graph engine.
- **Targeted incremental staleness** (`context/staleness.ts`):
  staleness propagates ONLY along explicit dependency manifests —
  contract change → plan stale; applicable guidance change → dependents
  potentially stale; source revision change → prepared mutation stale;
  changeset change → review stale; validation evidence change →
  acceptance reevaluated. Unrelated repository changes never stale
  unrelated artifacts. Not an incremental build system.
- **Context provenance and why-diagnostics** (`context/provenance.ts`):
  `ContextProvenanceRef` on high-value items (authority constraints,
  validation requirements, acceptance requirements, review evidence,
  verified touchpoints) and deterministic why-diagnostics
  (why-validation-required from the H2 ValidationPlan rationale, stale,
  blocked, acceptance-failed) derived from structured state — never by
  asking another LLM to reconstruct reasoning.
- **Phase-driven projection** (`context/projection.ts`):
  `projectPhaseContext` builds the minimal sufficient ContextProjector
  segments from the active PhaseContract's declared classes;
  `toolSurfaceForPhase` maps the contract to a ToolProjector mode via a
  fixed host table (planning→planning, review→review, mutation→
  development, acceptance→inspection). ContextProjector/ToolProjector
  remain the actual authorities; a malicious contract can at most select
  a mode, never grant a tool.
- **Source-integrity signals** (`context/source-integrity.ts`):
  repeated downstream corrections (same architecture finding, same
  rejected file pattern, same omitted validation) are classified into
  bounded `CorrectionPattern`s and may produce a recording-only
  `SourceProblemCandidate` with a likely source class — **no automatic
  source-guidance modification** (controlled self-improvement belongs to
  Stage 6 `/evolve`).
- **Solaris-development context**: scoped Solaris implementation tasks
  resolve global guidance → scoped AGENTS → architecture domain →
  applicable ADRs → TaskContract/TaskPlan → WorkspaceScope → current
  PhaseContract → active working set via the existing deterministic
  selectors; no all-ADRs/full-roadmap/historical-handoff defaults.
- **Status**: `/status` renders the active PhaseContract identity
  (id/version/digest/authority) as a compact projection.

## Explicit distinctions

```text
filesystem organization ≠ workflow authority
Markdown              ≠ authorization
PhaseContract         ≠ TaskState
artifact dependency   ≠ generic workflow graph
context provenance    ≠ model chain-of-thought
```

## Explicit rejections

- Numbered-folder orchestration; Markdown approval/state.
- Recursive context loading; full repository context for every phase.
- Transcript as workflow memory (authoritative state is structured).
- Automatic source-rule rewriting (recording only until `/evolve`).
- Generic incremental build system; generic provenance graph; another
  workflow engine; full event sourcing.
- A second hashing system: H1 digests/manifests/deltas and H2
  determinism primitives are reused throughout.

## Consequences

- Every major phase declares its context needs and authority ceiling;
  provider context never defaults to repository-wide input.
- Equivalent authoritative inputs produce equivalent projections,
  staleness, and why-diagnostics (proven by the effect tests).
- Artifact lineage and provenance make final artifacts inspectable and
  reproducible without duplicating contents into model context.
- Stage 4 runtime preparation can consume the PhaseContract/artifact
  model without re-architecting context discipline.
