# Solaris roadmap

Solaris keeps six public product stages. A stage can have its contracts and
adapters implemented while still being operationally incomplete because an
unsafe filesystem or process boundary intentionally fails closed.

## Status vocabulary

- **Implemented surface** — contracts, adapters, commands, architecture rules,
  and deterministic tests exist.
- **Operational** — the capability executes end to end under its required
  security boundary.
- **Intentionally unavailable** — the entry point returns `unavailable` before
  execution, approval, mutation, checkpoint creation, or cleanup.

## Current position

- Stage 1 has a broad implemented surface but is not operationally complete.
  Workspace mutation, undo, command execution, private run directories, and Git
  inspection fail closed because Node does not provide the required
  identity-bound directory-relative create/replace/delete/launch primitives.
- Stage 2's Godot/GDScript contracts and orchestration are implemented, with
  static discovery and project inspection available. Engine execution,
  recovery mirrors, diagnostics, LSP startup, change application, validation,
  and quality execution remain intentionally unavailable for the same identity
  reasons.
- Stage 3 is active. Milestones 1–11 are implemented and tested. The
  cross-cutting Content Identity & Delta Verification milestone (ADR 0028)
  is implemented: typed canonical artifact digests, digest-bound
  TaskContract/TaskPlan identity and plan approvals, execution-input /
  guidance / tool-surface / review-input / acceptance-evidence manifests,
  semantic deltas, and explicit staleness rules.
- The cross-cutting executor briefing foundation (ADR 0022) is implemented:
  a versioned Execution Contract, milestone manifests (S3M8, S3M9, S3M10,
  and S3M11 have real validated manifests), evidence-backed milestone
  acceptance, the Executor Context Pack, the deterministic Executor Brief
  Compiler, and the `/brief` / `/milestone` inspection commands. It is not
  a roadmap stage.
- Stages 4–6 are not started.

## 1. Harness foundation

Goal: a provider-neutral interactive harness with explicit authority,
bounded data flow, deterministic diagnostics, and fail-closed host effects.

Implemented surface:

- npm workspaces, strict TypeScript, ESM, project references, Vitest, ESLint,
  formatting, architecture checks, and the interactive CLI
- provider port, deterministic fake provider, strict bounded provider/tool loop,
  transcript correlation, cancellation, and terminal sanitization
- read-only workspace inspection with canonical containment and traversal bounds
- capability policy, built-in profiles, approval contracts, sandbox backend,
  child-environment filtering, and live conformance commands
- mutation/checkpoint/undo, process, and Git inspection contracts plus their
  truthful diagnostics and fail-closed adapters

Operational exit remains blocked until Solaris can bind create, replace, delete,
cleanup, and executable launch to the exact objects validated and approved. The
current runtime must not re-enable pathname-based approximations.

## 2. Godot script-development MVP

Goal: safely understand and modify GDScript with engine-derived validation.

Implemented milestones:

1. Godot executable discovery, SHA-256 fingerprinting, deterministic selection,
   static `project.godot` profiling, and bounded executable-content inventory
2. Recovery-probe contracts, risk manifests, one-time approval model, diagnostic
   normalization, and truthful unavailable reporting
3. Version-bound Godot API knowledge models, dump parser/index, search, and lookup
4. GDScript check-only contracts, script hashing/enumeration, and diagnostic
   normalization
5. Bounded LSP framing/client, URI mapping, normalized language features, and
   session lifecycle contracts
6. Exact change-set development workflow with separate approvals, checkpoints,
   validation evidence, repair bounds, and integrity checks
7. Deterministic quality gates, warning/convention policy, independent
   fresh-context review, and bounded re-review

Available today: executable discovery and static project inspection without
opening, importing, or running the project.

Intentionally unavailable today: engine probes, recovery mirrors, API-dump
generation, check-only execution, LSP startup, change-set application, process
validation, and the quality stage. Stage 2's operational exit is therefore not
met, even though its contracts and injected-fake behavior are implemented.

## 3. Godot-native development MVP

Goal: move from script-oriented orchestration to structured Godot-native
understanding and, later, safely validated scene/resource changes.

Implemented foundations:

1. **Task Runtime** — bounded revisioned `TaskContract`, authoritative
   single-owner `TaskState`, evidence-backed completion, terminal-state
   invariants, progress/stuck detection, immutable runtime snapshots, and typed
   activity records
2. **Context, Tool, and Evidence Projection** — stable/contextual/volatile
   context, available/gated/hidden tool projection, bounded sanitized evidence,
   context pressure handling, and stale async-result rejection
3. **Workspace Revisions and Structural Reads** — opaque SHA-256 revision
   handles, stale-state rejection, exact/structural/summary reads, deterministic
   GDScript extraction, and revision-aware evidence
4. **Project Instructions and Knowledge** — scoped instruction precedence,
   protected behavioral configuration, immutable knowledge revisions,
   provenance/confidence/freshness, bounded retrieval, and authority separation
5. **References and Research** — structural workspace/reference/research
   separation, immutable reference identities, bounded reference tools,
   policy-gated HTTPS sources, service-enforced exact task/revision binding, and
   explicit provenance
6. **Self-Reference and Capability Doctor** — host-generated installed-runtime
   documentation, authoritative command catalog, typed capability snapshots,
   offline read-only diagnostics, safe reports, and trustworthy exit codes
7. **Host-Controlled Planning** — deterministic `none | light | full` routing,
   read-only fresh-context planner, strict bounded provider turns, immutable
   plan revisions, verified touchpoints, exact plan approval, and a pre-executor
   acceptance/staleness gate
8. **Read-Only Scene and Resource Intelligence** — bounded `.tscn`/`.tres`
   parsing (hand-written tokenizer + conservative Variant parser), revision-bound
   semantic models (`GodotSceneModel`/`GodotResourceModel`), distinct
   parent/owner and inheritance/instancing relationships, document-local
   subresources, preserved UID identity, signal connections, groups, script
   attachments, project settings/autoload/input-action intelligence, a small
   revision-aware relationship index, read-only `godot.inspect_scene` /
   `godot.inspect_resource` / `godot.dependencies` tools, `[Scene evidence]`
   context projection, and planning touchpoints with scene/resource evidence —
   all static and process-free
9. **Review Context and Impact Intelligence** — bounded evidence-backed
   `ReviewContextManifest` derivation (primary changes, related surfaces,
   inherited/instantiated impact, signal consumers/producers, test surfaces,
   autoload dependencies, regression areas, recommended validation with honest
   `runtime_evidence_unavailable` classification) feeding planning and
   independent review context (ADR 0025)
10. **Approved Scene and Resource Mutation** — typed scene/resource mutation
    operations, immutable prepared mutations bound to the exact source
    revision, complete previews, revision-bound one-time approval, checkpoints
    before mutation, deterministic structural serialization, post-apply
    reparse and semantic verification, prepare-only provider tools, and no raw
    `.tscn`/`.tres` text-edit fallback (ADR 0026)
11. **Unified Godot-Native Development Workflow** — one host-owned
    `/develop` loop for script-only, native-only, and bounded mixed tasks:
    deterministic surface routing, unified multi-target change sets with
    per-target revision/fingerprint/approval/verification retention, derived
    dependency-based apply ordering, one checkpoint-then-apply batch
    revalidating every target before any write, per-surface verification
    (GDScript parser/fresh-LSP; native reparse/semantic), cross-surface
    consistency with honest runtime-only disclosures, impact-driven
    validation, read-only independent review, bounded repair with fresh
    artifacts only, host-observed acceptance, and structured blocked
    dispositions (ADR 0027)

### Next: Stage 4 — Runtime and visual QA

Not started. The Stage 3 loop is complete; the first Stage 4 milestone is
Controlled Godot Execution: sandboxed, bounded project/runtime execution
that produces structured runtime evidence without granting unrestricted
desktop or network access. It is not implemented here.

## 4. Runtime and visual QA

Automated runtime testing, debugging, visual gameplay verification, and
performance profiling against an intentionally launched Godot project.

Status: not started.

## 5. Extensibility and optional agents

Skills, game-development-specific agent profiles, and explicitly user-invoked
multi-agent review/comparison.

Status: not started. Multi-agent functionality is not part of the current
runtime.

## 6. Controlled evolution and stable release

Controlled `/evolve` workflows, persistence where justified, security
hardening, compatibility policy, packaging, and stable release criteria.

Status: not started.
