---
id: ADR-0028
status: accepted
domains: [identity, task-runtime, security]
paths:
  [
    packages/core/src/identity/**,
    packages/core/src/tasks/task-contract.ts,
    packages/core/src/planning/planning-model.ts,
    packages/core/src/knowledge/knowledge-model.ts,
    packages/core/src/research/research-model.ts,
  ]
supersedes: []
---

# ADR 0028 — Canonical Artifact Identity and Semantic Deltas

- Status: accepted (Stage 3 — Content Identity & Delta Verification)
- Date: current milestone
- Related: ADR 0014 (task runtime), ADR 0016 (workspace revisions), ADR
  0020 (host-controlled planning), ADR 0022 (executor briefing), ADR 0027
  (unified Godot-native development workflow)

## Problem

Siralos bound important decisions to revision numbers (TaskContract
revisions, TaskPlan revisions, plan approvals) and to ad-hoc
`JSON.stringify(...) → SHA-256` digests scattered across subsystems.
Revision numbers are chronological identity — they cannot prove that two
artifacts have identical content or that content changed. Ad-hoc hashing
had no domain separation, no schema versioning, and no shared semantics.
Context projection retransmitted unchanged structured state every
iteration, and reviews/acceptance could not be proven stale when the
content they were based on changed.

## Decision

Introduce one shared typed digest primitive and a small set of semantic
deltas, with the discipline:

```text
revision → chronological identity
digest   → exact content identity
delta    → what materially changed
```

- **Canonical digest primitive** (`identity/artifact-digest.ts`): typed
  `ArtifactDigest { algorithm: "sha256", artifactType, schemaVersion,
value }` over a domain-separated canonical payload:

  ```text
  SHA-256("siralos:<ArtifactType>:v<SchemaVersion>\0" + canonical JSON)
  ```

  Deterministic canonical JSON makes identical semantic values produce
  identical digests regardless of key order; the domain separator makes
  different artifact types unable to collide by representation reuse.
  Workspace stale-write protection keeps hashing exact source bytes —
  source bytes are never normalized before those hashes.

- **Digests are not authority**: `digest match ≠ trusted ≠ approved ≠
authorized`. Capability policy, approvals, provenance, sandbox, and
  evidence confidence remain authoritative. The plan-approval gate still
  requires the runtime approval record; a matching digest alone grants
  nothing (effect test 24).

- **TaskContract identity**: every revision carries `digest` (content
  only — revision excluded, so content-identical revisions share a
  digest). `TaskContractDelta` reports exactly which sections changed
  (request/context/constraints/acceptanceCriteria/pausePolicy).

- **TaskPlan identity**: every revision carries `digest` plus
  `taskContractDigest` binding the exact contract content. `PlanApproval`
  records both digests; the runtime approval gate verifies the plan's
  self-consistency and its contract binding, and any content change
  invalidates the old approval. `TaskPlanDelta` reports material section
  changes and contract-binding changes.

- **Manifests** (`identity/manifests.ts`): digest-backed references, not
  duplicated contents.
  - `GuidanceManifest`: the exact 3.7B-selected documentation (root +
    scoped AGENTS.md, architecture docs, ADRs), each with its content
    digest, plus an aggregate digest; `GuidanceDelta` surfaces changed
    documents.
  - `ToolSurfaceManifest`: per-role/per-phase actual provider-visible
    tool schemas with a canonical digest; `ToolSurfaceDelta` surfaces
    added/removed/changed/retained tools (developer→reviewer removals
    visible). Tool-surface identity never replaces ToolProjector
    authority.
  - `ExecutionInputManifest`: the exact effective input environment of
    one execution iteration by reference (TaskContract, TaskPlan,
    ExecutionContract, MilestoneManifest, GuidanceManifest,
    ToolSurfaceManifest, CapabilitySnapshot, source revisions) with a
    deterministic aggregate digest, recorded in the task runtime
    activity log and the task snapshot. `ExecutionInputDelta` projects
    only changed inputs.
  - `ReviewInputManifest`: binds every review to the exact contract
    digest, changeset identity, review-context digest, acceptance-set
    digest, validation-evidence digest, and source revisions; the verdict
    records `reviewedInputDigest`. Changed reviewed state → new digest →
    old review stale (never silently applies).
  - `AcceptanceEvidenceManifest`: binds acceptance to the exact
    evidence-set digest; changed evidence → acceptance reevaluated.
  - `ValidationResultIdentity` + `ValidationDelta`: stable result
    identity and newly-passing / still-failing / new-failure deltas; the
    authoritative result stays in the EvidenceStore.

- **Knowledge/research identity**: knowledge fact revisions carry a
  canonical content digest without collapsing provenance, confidence,
  freshness, or volatility into hash equality. Research documents carry a
  normalized-content digest (over the exact final sections) and a raw
  artifact digest of the exact fetched bytes; a URL alone is never stable
  evidence identity.

- **Staleness rules** (`identity/staleness.ts`): explicit high-value
  dependencies only — contract digest → plan potentially stale; guidance
  digest → execution context potentially stale; changeset digest →
  review stale; validation-evidence digest → acceptance may require
  reevaluation. No generic reactive dependency graph.

- **Status/doctor**: compact abbreviated digests (contract rev N /
  `abcd1234…`, plan rev M / `efgh5678…`) in task status; full values
  remain available in structured diagnostics.

- **Checkpoints**: reused as-is — checkpoint manifests already bind the
  workspace path, the exact pre-mutation content digest, and the expected
  applied content digest; content-addressed blob deduplication stays
  deferred.

- **ActivityLog**: digest references added to important events
  (`execution_input_recorded` with the manifest digest; plan approvals
  carry the plan digest) — no hash chaining of the whole log.

## Explicit rejections

- **Revision numbers alone** — digests add exact content identity.
- **Full context retransmission every iteration** — semantic deltas
  reduce repeated unchanged state where the projector can use them.
- **Delta-only state reconstruction** — deltas are derived
  communication; authoritative current state always remains the full
  artifact.
- **Subsystem-specific ad-hoc hashing** — one typed, domain-separated
  primitive.
- **Hashing all ephemeral state** — only artifacts with a concrete
  verification need are digest-bound.
- **Content hash as trust/authentication** — digests verify identity
  only.
- **Full ActivityLog hash chaining** at this stage.
- Blockchain/Merkle infrastructure, cryptographic signatures,
  remote-worker trust, content-addressing every runtime object, full
  event sourcing, delta-only authoritative state, a custom
  general-purpose patch language, distributed synchronization, and
  checkpoint blob deduplication.

## Consequences

- Plan approvals, reviews, and acceptance bind exact content; materially
  changed content invalidates stale approvals/reviews/acceptance.
- Execution iterations carry a reproducible input identity
  (`producedUnder: <ExecutionInputManifest digest>`), reusable by future
  runtime-execution manifests, runtime evidence, remote-worker snapshots,
  agent handoffs, and `/evolve` provenance.
- Existing revisions, approvals, evidence, and sandbox authority are
  unchanged; Stage 3 workflows remain green.
- **Enforcement staging**: the staleness rules and the review-input /
  acceptance-evidence / tool-surface / capability manifests are
  implemented as the derived identity vocabulary with unit and effect
  tests; production enforcement is wired where existing policy already
  invalidates (contract revision → plan invalidation, digest-bound plan
  approval, execution-input recording in `/develop`). The remaining
  rules are advisory derived context until their consumers adopt them —
  they never grant or weaken authority in the meantime.
