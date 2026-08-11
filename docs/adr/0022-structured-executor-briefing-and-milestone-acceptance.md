# ADR 0022 — Structured Executor Briefing and Milestone Acceptance

- Status: accepted (cross-cutting executor briefing foundation)
- Date: current milestone
- Related: ADR 0014 (task runtime foundation), ADR 0015 (context/tool/
  evidence projection), ADR 0017 (project instructions and knowledge),
  ADR 0020 (host-controlled planning), ADR 0021 (read-only Godot scene
  and resource intelligence)

## Context

Solaris executor prompts repeatedly restate permanent requirements in
prose: architecture boundaries, Git discipline, security posture,
validation commands, testing expectations, and reporting rules. Each
milestone brief is therefore a large hand-maintained document that

- costs tokens on every executor invocation,
- dilutes task-specific instructions with boilerplate,
- drifts from the enforcement that actually lives in the runtime,
  architecture checks, and AGENTS.md,
- makes milestone maintenance manual and error-prone, and
- gives completion claims no shared, evidence-backed vocabulary.

Solaris already owns the authoritative pieces: the revisioned
TaskContract and host-owned TaskState (ADR 0014), the bounded projection
boundaries (ADR 0015), path-scoped instructions and project knowledge
(ADR 0017), host-controlled planning with verified/candidate
touchpoints (ADR 0020), and capability diagnostics (ADR 0019). What is
missing is a structured, versioned layer that composes those systems
into a short, deterministic execution input and an evidence-backed
acceptance mechanism.

## Decision

Introduce a Solaris-owned structured execution model with seven distinct
artifacts:

1. **Execution Contract** — the versioned, immutable repository-owned
   statement of PERMANENT executor rules (Git discipline, security,
   architecture, standard validation, testing, reporting). Each rule
   references where its enforcement actually lives (`enforcedBy`); the
   contract describes invariants and never re-implements enforcement.
   It grants nothing and carries no capability surface.

2. **Milestone Manifest** — the versioned, immutable statement of ONE
   milestone's delta: goal, deliverables, invariants, non-goals,
   acceptance requirements with stable ids, required tests, and
   deterministic architecture-concern tags. It never restates permanent
   rules; standard reusable acceptance (`STANDARD.*`) is referenced by
   id from a small fixed library.

3. **Acceptance IDs and Evaluator** — stable requirement ids
   (`S3M8.PARSE.TSCN`, `CORE.GIT.NO_PUSH`, `STANDARD.NO_MUTATION`)
   decoupled from test filenames, plus a small host-owned evaluator that
   maps manifest requirements to HOST-attached evidence records and
   host-verified task criteria. There is no path for an executor claim:
   prose assertions are structurally unrepresentable as evidence.

4. **Executor Context Pack** — derived, bounded context gathered from
   existing subsystems (TaskContract, TaskPlan touchpoints, path-scoped
   instructions, a small explicit ADR tag index, CapabilitySnapshot,
   current findings). It is disposable input, never another source of
   truth, and never embeds raw giant file contents.

5. **Executor Brief Compiler** — a deterministic, provider-neutral
   compiler from (TaskContract + plan-context pack + Execution Contract
   - Milestone Manifest) to a bounded `ExecutorBrief` with a stable
     fingerprint. Permanent rules are referenced as `Execution Contract
rev N`, never restated; task-specific content (deliverables,
     verified/candidate touchpoints, invariants, non-goals, acceptance
     ids, relevant architecture references, capability limits) is kept;
     bounds trim low-value context before invariants/acceptance.

6. **Standard Validation Profile** — `standard-repo-validation`
   represents the repository's standard checks (format, lint,
   typecheck, tests, behavior tests, architecture) as a host-owned
   profile reference instead of prompt prose; manifests specify only
   extra/special validation.

7. **Briefing service and task snapshot identity** — the session-level
   service memoizes compilation per (task, contract revision, plan
   identity, milestone version) so unrelated volatile state never
   rewrites a compiled brief; the immutable task runtime snapshot
   records the execution-contract revision, the milestone-manifest
   identity, and the initial brief fingerprint for reproducibility.

The compiled brief becomes a first-class structured input to execution
context: ProjectionService renders it as a bounded contextual
`[Executor brief]` segment, keeping the stable prompt prefix cacheable.
A minimal developer surface (`/brief`, `/milestone`) shows the compiled
brief and the manifest's evidence-backed acceptance status; brief
compilation is always provider-free (dry-run safe). No generic workflow
DSL, task graph, or policy language is introduced, and no capability or
security authority moves into manifests or briefs.

## Rejected

- **Continuing with giant standalone milestone prompts** — token cost,
  dilution, drift, and manual maintenance remain.
- **One giant static system prompt** — cannot vary per task and would
  freeze task-specific content into the stable prefix.
- **A generic prompt-template DSL** — arbitrary templating invites
  injection and duplication of policy in prompt form.
- **Executor self-declared completion** — claims are not evidence; the
  evaluator accepts only host-observed evidence.
- **Duplicating policy rules in every milestone** — violates the single
  source of truth; the contract references enforcement instead.
- **Model-based task-context selection as the only mechanism** — context
  selection is deterministic (explicit tags), never semantic
  classification.

## Consequences

### Benefits

- Much shorter executor inputs: the S3M8 brief is a compact structured
  artifact instead of a repeated full milestone prompt.
- More deterministic enforcement: permanent rules exist once and are
  referenced by revision; acceptance is evidence-backed.
- Reduced prompt drift: rule changes revise the contract, not every
  prompt.
- Easier roadmap/milestone maintenance: a milestone is a manifest delta.
- Better reproducibility: brief fingerprints and snapshot identity make
  task inputs reproducible.
- Future benchmark/evolution support: stable ids and fingerprints give
  `/evolve`-style work a fixed reference surface.

### Costs

- Structured manifest maintenance: manifests must be authored and
  validated per milestone.
- Compiler complexity: bounded rendering and deterministic selection
  must stay host-owned and architecture-checked.
- Contract metadata must be kept aligned with the runtime enforcement it
  references; the `enforcedBy` pointers are documentation of authority,
  and the authoritative subsystem must remain the actual enforcer.

### Security posture

The Execution Contract, Milestone Manifest, and ExecutorBrief carry no
capability/policy surface; sandbox and capability policy stay
authoritative in the security layer. Architecture checks enforce that
briefing modules cannot import security, capability, approval, task
mutation, provider port, or network machinery, that provider adapters
never recreate briefing, and that only projection-service consumes the
brief surface. An executor claim can never satisfy acceptance.
