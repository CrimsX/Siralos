# Siralos roadmap

Siralos keeps six public product stages. Stages 1–3 are historical/current
product milestones; future stages 4–6 follow the lean vision freeze in
[ADR 0036](docs/adr/0036-lean-product-composition-and-extension-model.md)
(Stages 4–6 remain staged product direction subject to evidence, not
guaranteed implementation commitments). A stage can have its contracts and
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
- Stage 3 is complete: milestones 1–11 are implemented and tested. The
  cross-cutting Content Identity & Delta Verification milestone (ADR 0028)
  is implemented: typed canonical artifact digests, digest-bound
  TaskContract/TaskPlan identity and plan approvals, execution-input /
  guidance / tool-surface / review-input / acceptance-evidence manifests,
  semantic deltas, and explicit staleness rules. The cross-cutting
  Deterministic Execution & Reproducibility milestone (ADR 0029) is
  implemented: explicit clock/randomness/ordering ports, environment and
  reproducibility manifests, deterministic validation/acceptance/retry
  decisions, concurrency normalization, deterministic discovery with an
  ownership index, a nondeterminism audit, and a determinism doctor area. The Interpretable
  Context Architecture extension (ADR 0030) is implemented: formal context
  classes, typed PhaseContracts with narrowing-only authority, digest-bound
  artifact envelopes and dependency manifests, targeted incremental staleness,
  provenance with deterministic why-diagnostics, phase-driven projection, and
  recording-only source-integrity signals. The Runtime
  Readiness & Operational Resilience milestone (ADR 0031) is implemented:
  causal run identity, RunManifest, side-effect policy and run-owned
  boundaries, artifact budgets/retention, the failure taxonomy, process
  supervision, cancellation/reconciliation, the fail-closed readiness
  manifest, the deterministic fault-injection harness, and doctor
  readiness reporting — stopping at the Stage 4 execution boundary.
- The cross-cutting executor briefing foundation (ADR 0022) is implemented:
  a versioned Execution Contract, milestone manifests (S3M8, S3M9, S3M10,
  and S3M11 have real validated manifests), evidence-backed milestone
  acceptance, the Executor Context Pack, the deterministic Executor Brief
  Compiler, and the `/brief` / `/milestone` inspection commands. It is not
  a roadmap stage.
- **Stage 3R is active.** R1 (Siralos rename + Rust engineering standard +
  domain-neutral Rust foundation, ADR 0032) is complete. R2
  (Differential Behavioral Harness, ADR 0033) is complete: the audit
  remediation gate runs the scenario corpus against the TypeScript
  reference and the Rust candidate under symmetric bounded supervision,
  semantically compares typed canonical outcome records, emits a
  digest-bound per-commit migration audit, and gates remediation —
  its first subjects (state-dir resolution, product version identity)
  hold parity, and the audit drove real drift remediation in
  `siralos-adapters::paths`. R3 (Domain-Neutral Core) is complete: the
  host-owned task kernel in `siralos-core` (revisioned contracts with
  the reference digest contract, authoritative task state, lifecycle
  transitions, bounded evidence, acceptance, completion gating, terminal
  immutability, activity, and progress) holds byte parity with the
  TypeScript reference across 17 differential `task-contract`
  scenarios. R4 (Generic Workspace / Project Foundation) is complete:
  `siralos-core` owns the validated workspace-relative path type, the
  bounded revision registry, prepared-effect and checkpoint contracts,
  and the typed unavailable Git disposition; `siralos-adapters` owns the
  canonical root and containment resolution, bounded exact reads,
  deterministic listing and search, fail-closed mutation preparation,
  checkpoint storage inspection/reconciliation, and the Git disposition
  boundary. The differential corpus gained 23 R4 scenarios across the
  `workspace-read`, `workspace-list`, `workspace-search`,
  `workspace-revision`, `workspace-prepare`, `checkpoint`, and
  `git-inspection` subjects; all required applicable scenarios match and
  the complete local repository gate passes (corpus schema 3, corpus
  version 7, 47 scenario files). R4 hardening made the bounded exact reads
  EOF-verified on both implementations (a short read is never treated as
  EOF, a partial prefix never becomes whole-file identity, and size
  boundaries are explicit) and made checkpoint source-path inspection fail
  closed on any escape. R5 (Generic Language Intelligence) is complete:
  `siralos-core::language` owns the one-based position/range model,
  the bounded sanitized diagnostic model with deterministic
  dedup/ordering and explicit truncation, generic symbol/definition/
  reference query models, the language-neutral structural-document
  representation with the deterministic advisory summary formatter,
  typed validation result semantics (source-invalid never conflated with
  infrastructure failure), reference-extracted generic limits, and R4
  revision binding; `siralos-adapters::language::uri` owns the generic
  language-service URI mapping. The TypeScript reference gained matching
  generic language modules that the Godot adapters now consume. The
  structural representation is language-neutral by construction
  (cross-language kinds, opaque attributes, generic summary wording; no
  GDScript/Godot semantics in `siralos-core::language`), and the
  GDScript scanner/summary remain the TypeScript reference for R8/R9.
  The differential corpus gained 16 scenarios across the
  `language-diagnostics`, `language-structure`, and
  `language-definition` subjects (corpus version 9, 63 scenario files);
  all required applicable scenarios match. R5 ports no GDScript/Godot
  parsing, no LSP transport, no process execution, no provider tools,
  and no Domain architecture. R6 (Minimal Domain Capability Architecture
  and Synthetic Conformance Domain) is complete: `siralos-core::domain`
  owns the domain-neutral lifecycle/capability semantics (validated
  package identity with exact digest and versioned ABI, the explicit
  absent/installed/enabled/active state machine, Host-authoritative
  capability grants, exact activation binding, typed recovery-ready
  failures, and no implicit acquisition), `siralos-adapters::domain`
  owns the production Component Model / WIT boundary (versioned
  `siralos:domain-abi@1.0.0` world, exact-byte digest verification,
  fail-closed ABI identity, resource bounds, trap containment, and
  host-mediated effects), and the deterministic product-neutral
  synthetic conformance Domain proves the boundary on the checked-in
  component bytes. The differential corpus gained 23 scenarios across
  the `domain-lifecycle` and `domain-capability` subjects (corpus
  version 11, 86 scenario files); all required applicable scenarios
  match, and the Rust Component conformance suite passes. R6
  remediation additionally bound every prepared activation to the
  lifecycle generation validated at preparation (a stale commit fails
  typed with `STALE_ACTIVATION`, mutating nothing and consuming no
  session id), so preparation can never outlive the lifecycle episode
  it validated. Activation identity is exact in all three dimensions:
  the request ABI must identify the installed package ABI (a
  Host-compatible request can never substitute for a differently
  declared package ABI) and must also satisfy Host compatibility, so
  every successful activation satisfies
  `ActivationBinding::matches(installed_package)` by construction.
  Prepared activations never carry authority across Host policy
  contexts: the final capability grant is recomputed at commit from
  the commit-time Host authority (a narrower final authority fails
  typed with zero mutation and zero session consumption; a wider one
  can never widen the request). R6
  implements
  no Plugin system and no Godot Domain. R7 (Provider, Tool-Loop,
  Projection, Configuration, and CLI Parity) is Active: R7A behavior
  extraction and provider-protocol remediation are complete, and R7.1
  (Provider Contract + Deterministic Fake Provider + Bounded Single
  Model Turn parity) is complete (corpus version 13, 120 scenario
  files, 18 `provider-turn` scenarios at differential parity); R7.2
  (Application Tool Loop parity) is complete (corpus version 13, 120 scenario files, 16 `tool-loop` scenarios at differential parity, including authorization, displayInput UTF-16, and Tool-result status matrices). R7.3 Projection parity contract is frozen and reconciled with terminal-marker precedence pinned locally; independent review has returned PASS — authorized as the next implementation slice and not yet implemented.
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

Operational exit remains blocked until Siralos can bind create, replace, delete,
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

## 3R. Rust migration

Goal: migrate the Siralos product to an idiomatic Rust implementation
while the TypeScript implementation remains the behavioral reference
(migration oracle) until later 3R milestones retire it.

Implemented (R1 — Siralos Rename + Rust Engineering Standard +
Domain-Neutral Foundation, ADR 0032):

- The project identity is **Siralos** everywhere (CLI `siralos`,
  environment prefix `SIRALOS_`, state directory `~/.siralos`, npm scope
  `@siralos`); an identity ratchet (`npm run check:identity`) prevents
  regressions, with narrow documented exclusions only for the
  verification mechanism itself.
- The TypeScript implementation is preserved and renamed; it is the
  Siralos behavioral reference. Behavioral parity is explicitly
  distinguished from structural parity; refactoring during porting and
  evidence-driven optimization are required policies.
- The authoritative **Siralos Rust Style & Engineering Guide**
  (`docs/development/RUST_STYLE.md`) governs all Rust code: edition
  2024, rustfmt (max_width 79) and Clippy (`-D warnings`) as required
  gates, typed errors, deterministic ordering, no-UTF-8-assumption path
  handling, `#![forbid(unsafe_code)]`, and explicit dependency/async/
  concurrency policy.
- The domain-neutral Rust workspace exists: `siralos-core`
  (domain-neutral host semantics; compiles with no Godot domain present,
  enforced by `npm run check:rust`), `siralos-adapters` (infrastructure
  ownership), `siralos-cli` (the `siralos` binary). Dependency direction
  `cli → adapters → core` is machine-enforced; no placeholder or
  hypothetical domain crates exist.
- Optional-domain product policy: Godot is not installed, enabled,
  auto-detected, auto-recommended, or auto-downloaded by default; the
  user must explicitly request it. No marketplace or plugin ecosystem is
  implemented.

Implemented (R3 — Domain-Neutral Core, ADR 0036):

- The host-owned task kernel in `siralos-core`: revisioned immutable
  TaskContract with the exact reference content-digest contract
  (revision = lifecycle identity, digest = material identity),
  materialized authoritative TaskState with an explicit phase transition
  table, terminal immutability, bounded evidence bound to the exact
  contract revision/digest, host-owned acceptance (deterministic/review/
  user verification kinds with successful-outcome cross-checks), the
  completion gate, append-only activity records, and host-observed
  progress.
- The R2 differential harness gained 17 `task-contract` scenarios
  executed by both implementations (the TypeScript oracle runs the real
  reference via Node's native type stripping); all required applicable
  scenarios match byte-for-byte and the complete local repository gate
  passes (corpus schema 3, corpus version 5).

Implemented (R4 — Generic Workspace / Project Foundation):

- The domain-neutral workspace foundation in `siralos-core`: workspace
  identity and the validated workspace-relative path type (NUL,
  absolute, drive, and parent-traversal rejection; protected-path and
  behavioral-configuration classification), the reference bounds,
  deterministic revision handles and the bounded session registry
  (workspace/path/content bound; handles grant no authority), the typed
  prepared create/edit/delete effect models, the checkpoint model with
  operation-state invariants, undo planning, and reconciliation
  classification, and the read-only Git error/disposition contract.
- The workspace adapters in `siralos-adapters`: canonical root
  resolution, containment-safe path resolution (symlink/junction
  escapes rejected), bounded complete exact reads (EOF-verified; a
  partial prefix is never returned as complete) with whole-file SHA-256
  identity, deterministic bounded listing and search with the reference
  exclusions and truncation dispositions, the fail-closed
  mutation-preparation boundary (prepare/apply report unavailable;
  nothing is written, approved, or checkpointed), checkpoint storage
  inspection and startup reconciliation over the reference metadata
  layout (creation and retention capacity remain unavailable), and the
  typed unavailable Git inspection boundary (no enforcing process
  sandbox exists in the Rust candidate; Git is never spawned).
- The differential harness (ADR 0033) gained the R4 subjects
  `workspace-read`, `workspace-list`, `workspace-search`,
  `workspace-revision`, `workspace-prepare`, `checkpoint`, and
  `git-inspection` with 23 scenarios driven through the real reference
  tools/store/registry and the real Rust adapters; all required
  applicable scenarios match (corpus schema 3, corpus version 7, 47
  scenario files). R4 hardening added differential coverage for bounded
  complete reads at the exact size boundary, whole-file suffix
  identity, symlink and parent-symlink escape, and checkpoint path
  escape, with deterministic short-read regression coverage on both
  implementations. Deliberately unavailable effects (mutation
  application, new checkpoint creation, Git inspection) report the same
  typed outcomes on both sides.

Current: Stage 3R R7 is Active (R7A behavior extraction and
provider-protocol remediation complete; R7.1 — Provider Contract +
Deterministic Fake Provider + Bounded Single Model Turn parity — complete at
differential parity); R7.2 — Application Tool Loop parity — complete and evidence-backed; R7.3 — Projection parity — contract frozen and reconciled, terminal-marker precedence pinned locally; independent review PASS — authorized as the next implementation slice, not yet implemented). The complete
internal sequence is recorded
in `docs/development/RUST_MIGRATION.md`.

### Next: Stage 4 — Controlled execution

Not started. Stage 4 begins only after the Stage 3R migration and the
pre-Stage-4 entry gate pass. Its first milestone is generic Controlled
Runtime Execution: sandboxed, bounded process supervision under Siralos
authority that produces structured runtime evidence without granting
unrestricted desktop or network access. The Godot runtime adapter is the
first specialization built on that host boundary; it is not the boundary
itself. Neither capability is implemented here.

## 4. Controlled execution

Stage 4 — Controlled Execution (lean vision, ADR 0036): generic
host-authorized runtime execution and structured runtime evidence, followed by
the optional Godot runtime adapter, visual evidence, controlled interaction,
QA workflows, and performance profiling where domain-owned. The generic
boundary comes first; the Godot adapter is a specialization, never the
boundary itself (ADR 0035).

Status: not started. The Stage 3R migration and the Stage-4 entry gate must
pass first.

## 5. Composition

Stage 5 — Composition (lean vision, ADR 0036): Profiles (declarative working
configuration; the composition unit), portable locking (`siralos.toml` /
`siralos.lock` semantics), Context controls (Live / Pinned / Frozen;
show/explain/diff), Skills and the Skill Creator, capability-scoped Plugins,
Tools, Views where justified, and optional Domains contributed by Plugins.
Multi-agent functionality is not part of Siralos Core and is not committed
roadmap work (ADR 0036).

Status: not started. These are staged product direction subject to evidence,
not guaranteed implementation commitments.

## 6. Evolution and stabilization

Stage 6 — Evolution & Stabilization (lean vision, ADR 0036): bounded,
measured `/evolve` workflows (baseline → candidate → evaluation →
comparison → reject or propose) over Profiles, Context, Skills, Plugins, and
Host, evaluation corpora/baselines, Profile/Context optimization, Skill
creation/refinement, Plugin/Host improvement proposals, compatibility,
performance, packaging, and stable release criteria. Evolve requires
evaluation; it prefers configuration over Host complexity and may recommend
deletion.

Status: not started.
