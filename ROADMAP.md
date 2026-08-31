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
  Projection, Configuration, and CLI Parity) is **Verified**: R7A behavior
  extraction and provider-protocol remediation are complete, and R7.1
  (Provider Contract + Deterministic Fake Provider + Bounded Single
  Model Turn parity) is complete (corpus version 13, 120 scenario
  files, 18 `provider-turn` scenarios at differential parity); R7.2
  (Application Tool Loop parity) is complete (corpus version 13, 120 scenario files, 16 `tool-loop` scenarios at differential parity, including authorization, displayInput UTF-16, and Tool-result status matrices). R7.3 Projection parity is complete and evidence-backed (13 projection/application integration tests plus 11 required `context-projection` scenarios). R7.4 Configuration parity is complete and evidence-backed (2 required `user-config` scenarios, corpus version 15, 133 scenario files). R7.5 `/context` and `/tools` CLI rendering is complete and evidence-backed (deterministic real-session composition over the existing projection and Tool authority seams, 51 focused Rust CLI tests (10 sanitize) plus TypeScript oracle coverage; advisory P2 filed and closed); R7 is **Verified** at `61fbf997d781`. Stage 3R R8 — Optional Godot Stage-2 parity (discovery/profiling, recovery contracts, version-bound API knowledge, GDScript check-only diagnostics, bounded LSP, read-only scene/resource intelligence) is **complete and evidence-backed**: six surfaces ported across `siralos-core::godot` and `siralos-adapters::godot`, corpus **version 16, 155 scenario files**, all five frozen differential subjects (`godot-discovery` ×4, `godot-knowledge` ×5, `godot-diagnostics` ×4, `godot-lsp` ×4, `godot-scene-resolve` ×5) at required parity — **150/150 applicable required scenarios** (4 platform skips); the fail-closed posture is mechanically preserved (zero spawn paths in any Godot module); R8 is **Verified** at `c075b3cf5e52`. Stage 3R R9 — Optional Godot Stage-3 parity (review context & impact intelligence, prepare-only scene/resource mutation contracts, the deterministic unified `/develop` core) is **complete and evidence-backed**: three surfaces ported across `siralos_core::godot::{impact, scene_mutation, development}` and `siralos-adapters::godot::scene_mutation`, corpus **version 17, 167 scenario files**, all three frozen subjects at required parity (`godot-review-context` ×4, `godot-mutation-prepare` ×4, `godot-develop-plan` ×4) — **162/162 applicable required scenarios**; apply/checkpoints stay typed `unavailable`; R9 is **Verified** at `1623e800f8034`. Stage 3R R10 — H1/H2/ICM/H3 runtime-readiness parity — is **complete and evidence-backed** as one Verified milestone with three ordered, entry-reviewed sub-slices: R10a H1 content identity + H2 determinism/replay (`siralos_core::identity` extended, `siralos_core::determinism`; corpus **version 18, 182 scenario files**, 177/177 applicable required parity), R10b ICM phase contracts / dependency manifests / staleness / provenance (`siralos_core::context`; corpus **version 19, 195 scenario files**, 190/190), and R10c H3 runtime readiness — causal identity, manifest-bound budgets, the pure supervisor lifecycle and 13-kind failure taxonomy, harness-owned fault injection under the controlled clock, and the fail-closed readiness doctor (`siralos_core::runtime`; corpus **version 20, 210 scenario files**, 205/205); no real process is ever launched and effect-boundary/security/recovery/cross-platform closure remains R11; R10 is **Verified** at `a456afb71ab64c5504cd19e8eb7988d32d60a9dc`.
- Stages 4, 5 and 6 are Verified.

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

Current: **Stage 4 is Verified** at `9566eee` — the frozen seven-step Controlled-Execution sequence (decision 08) is fully consumed (generic runtime execution + evidence at the v32 reconciliation; Godot runtime adapter `5bedf57`; visual evidence `4a250d8`; controlled interaction `42ee5ab`; QA workflows `a83c2a4`; run-profiling sessions `b206a4a`), with differential parity 259/259 applicable required at corpus v38/264 files, 25 digest-bound post-freeze expectation records, the pinned v32 oracle untouched, and zero spawn paths (decisions 41–46; map: `docs/wayfinder/siralos-roadmap.md`). The Stage 3R sequence that preceded it is also Verified: Stage 3R R13 is **Verified** (R1–R11 Verified as recorded above; R13 — Remaining TypeScript Surface Parity — is complete and evidence-backed: five slices at corpus v31, 236 files, 231/231 applicable required parity; local gate passes; fail-closed postures preserved. R13 **Verified** at 72e20be. **TypeScript archive removal is complete** per decision 40 at `5da5cde` (freeze v32 234/234, pinned at `tests/differential/evidence/typescript-freeze-v32/`, corpus v33 `01ba53a…`; live `apps/` + `packages/` removed, `npm run check` is now Rust + pinned differential). Previous R11
R11 — full differential, effect-boundary, security, recovery, and
cross-platform parity — is complete and evidence-backed: `workspace-apply`
and `recovery-taxonomy` landed at corpus version 23, 222 scenario files,
217/217 applicable required scenarios; the Tier-1 `tier1-evidence.yml`
dispatch at eea0029e70aae7248b3e1022c3be1cb669fd5a09 returned three green
platforms with digest-bound audits (217/217 applicable required parity on
each) and truthful loud sandbox skips; all six Tier-1 findings are closed,
with the macOS `SSH_AUTH_SOCK` finding recorded as an accepted deviation).
The complete internal sequence is recorded
in `docs/development/RUST_MIGRATION.md`.

Stage 5 is Verified at `c2c30f0` — ten slices across decisions 47–56 (5.1 Profiles be030e3, 5.2 Profile Composition 4c562c8, 5.3 Context Controls ce3e7dc, 5.4 siralos.lock 0a6d592, 5.5 Plugin Selection 5e1b3e0, 5.6 Skills fcf61c5, 5.7 Session Plugin Activation Gate 926ac71, 5.8 Session Context Controls 6dc830e, 5.9 Session Lock Verification 6e38804, 5.10 Session Skill Consumption 579f1e9) with differential parity 299/299 at corpus v48/304, 65 expectation records, pinned v32 oracle untouched, zero spawn paths; decisions 47–57 annotated. Stage 6 is Verified at `e2c3540` — four slices across decisions 58–59 (6.1 Evaluation Corpus a79f613, 6.2 Workflow 0ba256f, 6.3 Proposal ddb18a4, 6.4 Packaging e2c3540) with differential parity 315/315 at corpus v52/320, 81 expectation records, pinned v32 oracle untouched, zero spawn paths; decisions 58–59 annotated and map’s Not-yet-specified fog is empty.

### Stage 4 — Controlled execution (realized)

Stage 4 is **complete and Verified** at `9566eee`. The `stage4-entry-gate.md`
17/17 PASS re-evaluation held (R12 retired); the entry sequence froze seven
steps (decision 08) and every step is implemented, entry-reviewed, and
evidence-backed: generic Controlled Runtime Execution — sandboxed, bounded
process supervision under Siralos authority that produces structured runtime
evidence without granting unrestricted desktop or network access (the
fail-closed posture is mechanically preserved) — the Godot runtime adapter
specialization on that host
boundary, visual evidence, controlled interaction, QA workflows, and
run-profiling sessions. The Godot domain lives in the in-repo Plugin crate
`crates/siralos-godot` (extraction landed per decision 37); `siralos-core`
stays domain-neutral.

## 4. Controlled execution

Stage 4 — Controlled Execution (lean vision, ADR 0036): generic
host-authorized runtime execution and structured runtime evidence, followed by
the optional Godot runtime adapter, visual evidence, controlled interaction,
QA workflows, and performance profiling where domain-owned. The generic
boundary comes first; the Godot adapter is a specialization, never the
boundary itself (ADR 0035).

Status: complete and **Verified** at `9566eee` — all seven realized steps hold differential parity at corpus v38 (259/259 applicable required, 25 digest-bound expectation records, pinned v32 oracle untouched) with zero spawn paths; recorded in decisions 41–46 and the Wayfinder map.

## 5. Composition

Stage 5 — Composition (lean vision, ADR 0036): Profiles (declarative working
configuration; the composition unit), portable locking (`siralos.toml` /
`siralos.lock` semantics), Context controls (Live / Pinned / Frozen;
show/explain/diff), Skills and the Skill Creator, capability-scoped Plugins,
Tools, Views where justified, and optional Domains contributed by Plugins.
Multi-agent functionality is not part of Siralos Core and is not committed
roadmap work (ADR 0036).

Status: complete and **Verified** at `c2c30f0` — ten slices across decisions 47–56 (5.1 Profiles be030e3, 5.2 Profile Composition 4c562c8, 5.3 Context Controls ce3e7dc, 5.4 siralos.lock 0a6d592, 5.5 Plugin Selection 5e1b3e0, 5.6 Skills fcf61c5, 5.7 Session Plugin Activation Gate 926ac71, 5.8 Session Context Controls 6dc830e, 5.9 Session Lock Verification 6e38804, 5.10 Session Skill Consumption 579f1e9) with differential parity 299/299 at corpus v48/304, 65 expectation records, pinned v32 oracle untouched, zero spawn paths; recorded in decisions 47–57 and the Wayfinder map.

## 6. Evolution and stabilization

Stage 6 — Evolution & Stabilization (lean vision, ADR 0036): bounded,
measured `/evolve` workflows (baseline → candidate → evaluation →
comparison → reject or propose) over Profiles, Context, Skills, Plugins, and
Host, evaluation corpora/baselines, Profile/Context optimization, Skill
creation/refinement, Plugin/Host improvement proposals, compatibility,
performance, packaging, and stable release criteria. Evolve requires
evaluation; it prefers configuration over Host complexity and may recommend
deletion.

Status: complete and **Verified** at `e2c3540` — four slices across decisions 58–59 (6.1 Evaluation Corpus a79f613, 6.2 Workflow 0ba256f, 6.3 Proposal ddb18a4, 6.4 Packaging e2c3540) with differential parity 315/315 at corpus v52/320, 81 expectation records, pinned v32 oracle untouched, zero spawn paths; recorded in decisions 58–59 and the Wayfinder map.
