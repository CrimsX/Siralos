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

## Verification retraction — read this before trusting any CI claim

**No CI workflow in this repository has ever executed.** From the Stage 7 Godot
externalization until commit `60a56fa`, `siralos-godot` was a mandatory `../`
path dependency of the product workspace, so a fresh checkout could not resolve
its own Cargo workspace and **every** cargo-invoking CI step failed before doing
any work. `60a56fa` removed that edge, and the pipeline was audited and repaired
afterwards — but as of this file's last update, **no recorded run exists**.

Therefore: any statement implying that CI is green, passing, or verified — in
this repository's history, in a decision record, or in a commit message from
before that repair — is **unverified**, whatever it says. Local gate runs are
evidence for the machine that ran them, not for CI.

Settling this requires a push and a recorded run outcome, failures included.
Until that exists, treat CI status as unknown rather than as passed.

## Current position

- Stage 1 has a broad implemented surface but is not operationally complete.
  Workspace mutation, undo, command execution, private run directories, and Git
  inspection fail closed. The reason is a **ported-parity decision, not a
  language limitation**: the Rust implementation was ported to byte-match a
  frozen TypeScript oracle that also lacked an identity-bound commit primitive,
  and the corpus pins that outcome
  (`tests/differential/corpus/workspace-apply.apply-unavailable.json`,
  `tests/differential/corpus/workspace-prepare.unavailable.json`). Reopening any
  of these is a deliberate, reviewed oracle amendment. A footnote here once
  blamed Node; that was true of the removed TypeScript tree and is obsolete.
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
in `docs/archive/RUST_MIGRATION.md`.

Stage 5 is Verified at `c2c30f0` — ten slices across decisions 47–56 (5.1 Profiles be030e3, 5.2 Profile Composition 4c562c8, 5.3 Context Controls ce3e7dc, 5.4 siralos.lock 0a6d592, 5.5 Plugin Selection 5e1b3e0, 5.6 Skills fcf61c5, 5.7 Session Plugin Activation Gate 926ac71, 5.8 Session Context Controls 6dc830e, 5.9 Session Lock Verification 6e38804, 5.10 Session Skill Consumption 579f1e9) with differential parity 299/299 at corpus v48/304, 65 expectation records, pinned v32 oracle untouched, zero spawn paths; decisions 47–57 annotated. Stage 6 is Verified at `e2c3540` — four slices across decisions 58–59 (6.1 Evaluation Corpus a79f613, 6.2 Workflow 0ba256f, 6.3 Proposal ddb18a4, 6.4 Packaging e2c3540) with differential parity 315/315 at corpus v52/320, 81 expectation records, pinned v32 oracle untouched, zero spawn paths; decisions 58–59 annotated and map’s Not-yet-specified fog is empty.

### Stage 4 — Controlled execution (realized)

Stage 4 is **complete and Verified** at `9566eee`. The
`docs/archive/stage4-entry-gate.md` 17/17 PASS re-evaluation held (R12 retired); the entry sequence froze seven
steps (decision 08) and every step is implemented, entry-reviewed, and
evidence-backed: generic Controlled Runtime Execution — sandboxed, bounded
process supervision under Siralos authority that produces structured runtime
evidence without granting unrestricted desktop or network access (the
fail-closed posture is mechanically preserved) — the Godot runtime adapter
specialization on that host
boundary, visual evidence, controlled interaction, QA workflows, and
run-profiling sessions. The Godot domain lives in the in-repo Plugin crate
the external Godot plugin crate (extraction landed per decision 37); `siralos-core`
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

## 7. Godot externalization

Stage 7 — Godot externalization (lean vision, ADR 0036): the Godot domain and its host adapters move to the standalone siralos-godot repository, pinned in the monorepo as an external path dependency, keeping the core domain-neutral and adapters core-only.

Status: complete per decisions 60–65 — the plugin is fully self-contained at external 1bf2ca3 (41 domain files + host adapters, 234 tests), the monorepo pins it as siralos-godot = { path = "../siralos-godot" } (3-member workspace, shim removed at 87bfd35), and differential parity held (315/315 at v52). The plugin repository is pushed to GitHub and managed independently.

## 8. Real Model/Provider

Real Model/Provider (lean vision, ADR 0036): declarative provider/model/credential/endpoint in ProfileRecord with env-only credentials, Host-mediated bounded HTTP adapters, an all-purpose generic provider with provider-neutral placeholder defaults, and determinism-port replay recording of provider responses.

Status: complete and **Verified** per decisions 66–71 — ProfileRecord fields + siralos.toml parsing (67 C1), env-only HostCredential (68), the registry with typed OpenAI/Anthropic adapters and the all-purpose GenericProvider, bounded 1 MiB sanitized HTTP adapters (2d6f5d9-era hardening), replay recording with typed Recorded/Unavailable availability and the recorded 68 §4 secret-hygiene sweep (70), the hermetic provider-generic subject at corpus v53/321 files (316/316 applicable required, 82 expectation records, pinned v32 oracle untouched), and the fresh full-gate run in the roll-up (71); zero spawn paths.

## 9. Release readiness (1.0)

Two things stand between this tree and a published 1.0 release. Both are recorded
here rather than implied, and neither is a coding task inside this repository.

- **The version identity is blocked outside this repository.** The workspace is
  versioned `0.0.0`. Moving it to `1.0.0` stops the external Godot plugin
  (`siralos-godot`) from resolving: that project declares
  `siralos-core = { path = …, version = "0.0.0" }`, and a caret requirement on
  `0.0.0` admits exactly `0.0.0`. The plugin is an independent repository with its
  own maintainers, so the workspace version stays behind its pin. The change is
  prepared and verified locally — removing the `version` key from that path
  dependency is enough — and deliberately not applied from here.
- **No license has been published.** Until one is chosen, every artifact is
  "all rights reserved"; the release workflow therefore refuses to publish while
  no license file exists. The tag-triggered workflow has also never executed, so
  the `unknown` CI status recorded at the top of this file applies to it exactly
  as it does to the existing workflows.

What does exist today: the release workflow (tag-triggered, digest-bearing,
publication authority isolated from ordinary validation jobs), the clean-clone
smoke test it runs before publishing anything, the
[stability contract](docs/development/STABILITY.md) for the 1.x line, and the
[changelog](CHANGELOG.md).

The differential reference set is a frozen oracle of 239 records plus 118
candidate-authored expectation records for scenarios introduced after the freeze,
plus any record retired through the reviewed supersession list (none today; the
mechanism exists for the version identity when it moves).

## 10. The 1.0 alignment work — state, blockers, and what remains

An owner-commissioned alignment audit of this harness produced a work breakdown
that is being executed step by step. This section is the durable record; the
working ledger in `.plan/STATUS.md` is local, gitignored scaffolding, so anything a
later session needs must appear here or in a commit.

### Shipped

| Commits                                           | What landed                                                                                                                                                                                                                                                     |
| ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `60a56fa`                                         | Step 1 — the differential harness extracted into its own excluded workspace, so a bare `git clone` of the product builds                                                                                                                                        |
| `04cdcf0` `5f6a069` `fdf6cb3`                     | Step 2 — CI honesty: the clean-clone resolution guard, the annotation of the pinned plugin revision, and the profile-config scratch-path race that failed a gate run once in six                                                                                |
| `d7e1a82`                                         | Step 3 — the secret-hygiene gate scans git-ignored files; ADR 0036 §10 reconciled                                                                                                                                                                               |
| `c52ca9d`                                         | Step 4 — headless mode, `siralos --print <prompt> [--json] [--cwd <dir>]`                                                                                                                                                                                       |
| `a24695f`                                         | Step 5 — documentation truth: `scripts/check-doc-truth.mjs`, the retraction ledger, 20 obsolete Node-rationale sites replaced with the ported-parity reason and its corpus pins                                                                                 |
| `2c884d4`                                         | Workstream 1 — `ARCHITECTURE.md` archived (112,644 bytes, digest captured before the move) and rewritten for the live tree                                                                                                                                      |
| `eb95bc5` `b241c69`                               | Workstream 2 — `README.md` rewritten as a user-facing front door; rule 3 of `check:docs-truth` extended to refuse milestone status outside this file; the completed Stage-4 gate archived                                                                       |
| `e3d83fc` `0eaa83e`                               | W3.1a — digest-bound supersession list, with a self-digest, a per-entry disclosure in the audit, and eight adversarial lists refused with named codes                                                                                                           |
| `49a0be1`                                         | The last false claims the removed TypeScript tree left in `ENGINEERING.md` and `README.md`, plus two stale CI labels                                                                                                                                            |
| `9e5fb6b`                                         | W3.2 — `docs/development/STABILITY.md` and `CHANGELOG.md`; the release blockers recorded in §9                                                                                                                                                                  |
| `fa68d0a` `d6e5140`                               | W3.3b/W3.3a — the clean-clone release smoke test and the tag-triggered release workflow                                                                                                                                                                         |
| `cf3e7df` `5c4e654` `2df3c89` `6a44f93` `382f6fc` | Block C1 — the command table stops claiming to be the product's vocabulary; four inert `.reasonix` references dropped; the `sha2` product ratchet and the zero-scenario subject assertion; `docs/development/REACHABILITY.md` plus `npm run check:reachability` |
| `a649ae9`                                         | Block C2 (first slice) — one atomic staged replacement, `crates/siralos-adapters/src/atomic.rs`, behind all six production writers                                                                                                                              |

### Settled by decision

- **W5.3 — lock honesty: verify-only.** Siralos verifies `siralos.lock` on the
  composition path and never writes one on any production path; the module doc of
  `crates/siralos-adapters/src/lockfile.rs` now records that. Its writer stays as
  the prepared implementation of `siralos profile lock`, one of the two explicit
  lock operations ADR 0036 §12 freezes the semantics of while deliberately leaving
  them unimplemented — "Normal execution must not silently modify `siralos.lock`."
  The absent production caller is that decision, not dead code: the writer is one
  of the six production `stage_atomic` call sites, and its tests exercise the
  write / load / verify roundtrip through the atomic path.

- **One secret-redaction owner in `siralos-core`.** `crates/siralos-core/src/doctor.rs`
  now owns the six ordered passes — `redact_secrets`, the ASCII `\b` boundary, the
  JavaScript `\s` set, and the greedy-run backtracking the reference's regular
  expressions rely on — and `crates/siralos-core/src/executor/brief.rs` re-exports
  it, so the 290-line duplicate and its thirteen functions are gone. The divergence
  shapes the four-seat panel enumerated are fixed against the recovered reference
  (`packages/core/src/doctor/safe-report.ts` at `5da5cde`), and the durable corpus
  in `doctor.rs` records its rows with expectations produced by executing that
  reference in Node rather than by reading it. Differential parity held at 352/352
  with no corpus amendment and no pinned record changed, and `#[allow(dead_code)]`
  is now refused under `crates/` by `scripts/check-rust-architecture.mjs`.

### Remaining

1. **W5.2 — `/cost`.** Reconciles to the accounting inputs on a fixture; the
   command does not exist today.
2. **W4.5 — provider-client consolidation.** The OpenAI, Anthropic, and generic HTTP
   paths behind a recorded-pair equivalence harness including error paths,
   explicitly not grep-equivalence.
3. **One rule model, three predicates — and three divergences found but not fixed.**
   The provider-id, model-id and credential-env-name rules now live once, in
   `crates/siralos-core/src/composition.rs` as `is_provider_id` (`:86`),
   `is_model_id` (`:101`) and `is_credential_env_name` (`:118`); the profile
   validator, the write boundary, the TUI form and the credential adapter all call
   them. Every inline copy of those three rules was replaced: core's
   `validate_provider_field`, `validate_model_field` and `validate_credential_field`
   (both the `env:` branch and the bare legacy branch), `write_profile_config`'s
   provider and model predicates (`crates/siralos-cli/src/interactive.rs:2934`) and
   `validate_live_model_id` (`:3266`), the credential `env:` and bare-legacy checks
   at the write boundary (`:2956`, `:2989`), the CLI's
   `validate_credential_env_name_inline` (deleted), `HostCredential::from_env_ref`
   and the bare legacy branch
   (`crates/siralos-adapters/src/provider/credential.rs`), and the TUI's
   `validate_provider_name` and `validate_model_name`. The _messages_ stay separate
   per boundary by design: the TUI carries a deliberate second register
   ("Human-readable error (D2) — validation rule unchanged"), so its
   `validate_api_protocol`, `validate_endpoint_value` and `validate_model_display_name`
   texts and `write_profile_config`'s endpoint and display-name guards were left
   alone, with hardcoded bounds replaced by the core constants where one exists.
   Three divergences were reproduced this round and deliberately **not** repaired,
   because each changes behaviour and needs its own reviewed decision:
   - **Credential ordering.** `env:` plus a 67-character name yields
     `The credential exceeds the 70-byte bound.` from `ProfileRecord::validate`
     (`crates/siralos-core/src/composition.rs:427`, bound before name) and
     `A credential env name must match [A-Z0-9_]{1,64} after "env:".` from
     `write_profile_config` (`crates/siralos-cli/src/interactive.rs:2956`, name
     before bound): one input, a different error on each path.
   - **The adapter's `key:` branch is laxer than core's.**
     `HostCredential::from_credential_str`
     (`crates/siralos-adapters/src/provider/credential.rs:29`) accepts a `key:`
     value of any length and one containing NUL, while `validate_credential_field`
     (`crates/siralos-core/src/composition.rs:438`) refuses both through the
     4096-byte bound and the NUL check.
   - **The writer rejects protocol values the loader accepts.** `Protocol::parse`
     (`crates/siralos-core/src/composition.rs:141`) accepts the legacy aliases
     `openai-compatible` and `anthropic`, but `write_profile_config`
     (`crates/siralos-cli/src/interactive.rs:3006`) and the TUI form
     (`crates/siralos-cli/src/tui.rs:3074`) accept only the three canonical names,
     so a value the loader reads back is one the writer refuses to store.

4. **W4.5 step one: the three provider clients are comparable, and the drifts it
   found are recorded rather than fixed.** The three identical client-build sites
   now share `provider::build_http_client`; a base-URL seam
   (`crates/siralos-adapters/src/provider/openai.rs:146`, `anthropic.rs:139`)
   lets an offline probe — a loopback fixture server in `provider/mod.rs` plus
   tests in each client's own test module — drive the real `call_*` paths with no
   live network: failure classification through the real path, the shared
   cancellation message, each client's own success event sequence, the request
   each client actually puts on the wire, and a `(status, body)` matrix over
   `{400, 401, 404, 429, 500, 503}` × {short, ~10 KB, HTML, ~10 KB HTML}. The
   probe records
   today's behaviour as a baseline, **not** approved parity. What it recorded and
   this round deliberately did not change:
   - **Error text shape.** The shared `run_chat_pipeline` builds the openai and
     anthropic HTTP-error text from a 512-character control-filtered snippet and
     embeds `reqwest`'s full status line (`openai error 400 Bad Request: ...`) at
     `provider/mod.rs:280`, while `generic.rs:832` builds
     `response failed: <code> at <url> - <body>`, cut at the first `<` to 240
     characters, and appends `RATE_LIMIT_HINT` on 429 only.
   - **No shared converter.** `openai.rs` and `anthropic.rs` never call
     `replay::completion_events_from_body`; only `generic.rs:530` and `:698` do.
   - **A response walk that stated an asymmetry — now one pass.** `anthropic.rs`
     used to walk `value["content"]` twice: the first block inline, then `skip(1)`
     over the rest, with the `tool_use` extraction written out in both arms. It is
     now a single pass over the array (`anthropic.rs:258`) that states the
     asymmetry at a named `index == 0` branch (`:269`), with the extraction written
     once (`tool_call_event`, `:311`). **The asymmetry itself is unchanged, because
     it is behaviour:** a `text` field carried on a `tool_use` block reaches the
     event stream only when that block is **first** — the first block is read for
     `text` before its type is considered, while a later `tool_use` block takes the
     tool-use branch and never falls through to the text arm. Text on ordinary
     blocks is collected wherever it sits, so "text is first-block-only" would be
     the wrong reading. Five probe cases pin it —
     `probe_records_first_block_text_before_its_tool_call`,
     `probe_records_the_tool_use_guard_dropping_only_the_push`,
     `probe_records_a_tool_call_with_no_input_key`,
     `probe_records_every_later_tool_use_block` and
     `probe_records_the_skip_first_boundary` — all five passing against the two-arm
     walk before the rewrite and against the single pass after it, unchanged; they
     join the round-4
     `probe_records_the_text_field_of_a_tool_use_block_only_when_it_is_first`.
   - **Auth follows the provider NAME, not the declared protocol.**
     `generic.rs:429` dispatches on `provider == "anthropic"`, so two requests
     that declare `AnthropicMessages` authenticate differently; recorded by the
     `probe_records_that_auth_follows_the_name_not_the_declared_protocol` test.
   - **Tool pairing is wire-different.** `openai.rs` round-trips
     `tool_calls`/`tool_call_id`; `anthropic.rs:188` and `:203` drop
     `AssistantToolCall` to an empty assistant message and flatten `ToolResult`
     into user text.
     A sixth reported drift **did not reproduce**: all three chat paths embed the
     same 512-character snippet in their parse-failure text — the shared pair at
     `provider/mod.rs:297`, the generic chat path at `generic.rs:513`. The
     genuinely different message is the models-listing probe at `generic.rs:784`,
     which carries no body text at all.
     The literals recorded here as out of scope have since been consolidated into
     constants in `provider/mod.rs`: `"2023-06-01"` at `:84`
     (`ANTHROPIC_VERSION`), `"no provider response observed yet"` at `:90`
     (`NO_PROVIDER_RESPONSE_OBSERVED`), and
     `"Host cancelled the turn before provider start"` at `:68`
     (`CANCELLED_BEFORE_PROVIDER_START`) — the last was not listed in this entry,
     because nothing here had flagged it — now owning all four of its sites,
     including `provider/replay.rs:253`. The first two literals survive only in
     test assertions; the third is referenced only through its constant.
     Step two then extracted the send-onward region the two chat clients shared —
     send, post-response cancellation, bounded read, non-success mapping and JSON
     parse — into `provider::run_chat_pipeline` (`provider/mod.rs:232`), leaving
     each caller its own request construction and its own parse. The two
     asymmetries recorded above (`safe`/`text` on the error path,
     `snippet`/`text` on the parse-failure path) were preserved exactly, and every
     probe assertion — including the recorded-outcome assertions that observe what
     `record_outcome` receives — passed unchanged before and after the extraction.

### Blocked

- **The version identity (§9)** — the external plugin's `version = "0.0.0"`
  requirement. A prepared patch is verified locally and re-applies unchanged.
- **Publishing anything** — no license file exists, and the release workflow refuses
  to publish without one.

### Owner decisions pending

- **The hardlink-target refusal in `atomic.rs`.** Two options were weighed: refuse a
  target with more than one hard link (Unix-only, and disclosed as a behaviour
  change), or leave replacement as it is. Not implemented; roughly ten lines plus a
  test.
- **W2.5's AGENTS.md roll-call.** The plan recorded a "seat roll-call" to be moved
  into `docs/development/PROJECT_CONTEXT.md`; it exists in neither that file nor
  `AGENTS.md` today, and the 32k-character line it was meant to fix is absent
  (longest line 1,246). Not claimed as done.
- **A push**, so CI stops being `unknown`. No workflow has ever executed.
- **The O3/I3 credential teaching message.** `crates/siralos-cli/src/tui.rs` carried
  a second credential-env-name validator whose failure text taught the pattern
  instead of restating the rule — verbatim: `this looks like the key itself -
Siralos stores the NAME of the environment variable holding your key; create it
with setx YOUR_API_KEY_NAME "the-key" and enter YOUR_API_KEY_NAME here`. It could
  not reach a user: every caller was inside that file's `#[cfg(test)] mod tests`,
  and the provider-add form's `ApiKey` handler
  (`crates/siralos-cli/src/tui.rs:3510`) stores a non-`env:` value verbatim as
  `key:<value>` with no validation at all, so nothing displayed it. It was deleted
  in the round-3 consolidation, and the surviving production message is the terse
  `A credential env name must match [A-Z0-9_]{1,64} after "env:".`. Promoting that
  teaching branch into the provider-add path — and validating the field at all — is
  a user-visible product change the owner has not made.

### Deliberately not in 1.0

- **W4.2 option C** — deleting the 51-entry core command table and re-recording
  `cli-session-set` against the real CLI vocabulary. It is a corpus amendment with
  its own gate; the coverage guard added in C1 now makes it safe to attempt.
- **W1.6b, the Plugin/Domain terminology rename** — listed in the plan, never
  scheduled by an owner-approved step.
- Windows junction and case-variant tests for `atomic.rs`; the differential runner
  swallowing its build stderr; `harness/Cargo.lock` tracking the product lock.

### Post-1.0 by the plan

Governed workspace **mutation** — letting the model create or edit files at all —
is 2.0, semver-major, with live approvals and the replacement of the eighteen
`unavailable` corpus scenarios, each a reviewed oracle amendment. Until then the
model-facing surface is exactly three read-only tools and the closed effects stay
closed.

### Verification state

`npm run check` is green on the current working tree: differential parity at 352/352
applicable required scenarios with four explicit platform skips, the eight
adversarial supersession lists refused with their named codes, the reachability
ratchet holding over 70 of 175 product modules, and clippy, tests, and every
documentation gate clean. CI remains `unknown` — see the retraction at the top of
this file — and the release workflow has never executed.
