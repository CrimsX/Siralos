# Siralos normative requirements

Status: authoritative requirements register.

This document owns the stable `CORE-*`, `HAR-*`, and `AP-*` identifiers.
Requirement text is normative. The status and evidence columns describe the
current repository; they do not weaken or silently rewrite the requirement.
An accepted ADR may refine an interpretation, but a superseding interpretation
must retain the requirement ID and cite the decision.

## Vision freeze interpretation (ADR 0036)

[ADR 0036](../adr/0036-lean-product-composition-and-extension-model.md) is the
authoritative interpretation for future-facing requirements. Under that
freeze: Profiles are the composition unit; Context is inspectable/controllable
state; Skills are declarative and authority-free; Plugins are the only
executable extension package (explicit install, capability-scoped); Tool is
the one callable operation abstraction; a Run uses one stable Effective
Profile; /evolve requires measured evaluation and prefers configuration over
Host complexity. Requirements implying now-rejected future architecture
(general Hooks, built-in multi-agent frameworks, TaskGraph, generic workflow
engines, agent teams/Fleet, distributed workers, plugin marketplaces, plugin
dependency graphs, automatic acquisition, model-router architecture, generic
Memory, GUI/TUI runtime ownership) are interpreted as deferred/superseded in
status below — their IDs remain stable and their requirement text remains
normative unless an implementation ADR supersedes it.

## Status vocabulary

- **VERIFIED**: implemented and supported by executable repository evidence.
- **PARTIAL**: meaningful implementation/evidence exists, but the full
  requirement is not yet satisfied.
- **ABSENT**: due now but not implemented.
- **NOT DUE**: deliberately sequenced to a later milestone.
- **INTENTIONAL DEVIATION**: an accepted decision explicitly differs from the
  wording and owns the rationale.

Only host-observed evidence may produce `VERIFIED`. A similarly named type,
test, prompt, or document is not sufficient by implication.

## Evidence owners

- [Architecture](../../ARCHITECTURE.md) and the
  [architecture index](../architecture/README.md) own dependency and subsystem
  placement.
- [Security](../../SECURITY.md) owns authority, trust, capability, sandbox, and
  fail-closed behavior.
- [Roadmap](../../ROADMAP.md) and the
  [Rust migration register](../development/RUST_MIGRATION.md) own milestone
  timing.
- [ADR 0028](../adr/0028-canonical-artifact-identity-and-semantic-deltas.md),
  [ADR 0029](../adr/0029-deterministic-execution-and-reproducibility.md),
  [ADR 0030](../adr/0030-interpretable-context-architecture.md), and
  [ADR 0031](../adr/0031-runtime-readiness-and-operational-resilience.md) own
  the H1/H2/ICM/H3 target contracts.
- [ADR 0032](../adr/0032-rust-migration-and-siralos-rename.md) and
  [ADR 0033](../adr/0033-differential-behavioral-harness.md) own migration and
  behavioral parity.
- [ADR 0036](../adr/0036-lean-product-composition-and-extension-model.md) owns
  the lean product, composition, and extension interpretation for
  future-facing requirements.
- `npm run check`, `npm run check:differential`, behavior tests, architecture
  ratchets, property tests, and fuzz targets are executable evidence. The
  [golden trace registry](../development/GOLDEN_TRACES.md) distinguishes
  implemented traces from future scenarios.

## Core requirements

| ID       | Normative requirement                                                                                       | Current status | Current evidence or owner                                                                                        |
| -------- | ----------------------------------------------------------------------------------------------------------- | -------------- | ---------------------------------------------------------------------------------------------------------------- |
| CORE-001 | Siralos Core contains no Godot-specific behavior.                                                           | PARTIAL        | Rust core neutrality is ratcheted; the TypeScript behavioral reference still contains Godot contracts.           |
| CORE-002 | Domains are optional extensions.                                                                            | PARTIAL        | ADR 0034 fixes the boundary; the Rust domain package lifecycle is due at R6.                                     |
| CORE-003 | Domains are never silently enabled.                                                                         | PARTIAL        | Security and ADR 0034 prohibit it; install/enable state does not exist yet.                                      |
| CORE-004 | Godot projects are not automatically detected for domain activation/acquisition.                            | PARTIAL        | No package acquisition exists; R6 must prove the lifecycle behavior.                                             |
| CORE-005 | Domain installation and domain enablement are separate states/operations.                                   | NOT DUE        | Stage 3R R6.                                                                                                     |
| CORE-006 | Models cannot directly mutate authoritative state.                                                          | VERIFIED       | Task runtime, application policy, evidence-backed acceptance, and Security.                                      |
| CORE-007 | Every authoritative mutation is represented by an explicit typed delta or equivalent transaction operation. | PARTIAL        | ADR 0028 and typed change sets exist; generalized Rust deltas begin at R3.                                       |
| CORE-008 | Deltas are validated before application.                                                                    | PARTIAL        | TypeScript preparation/revalidation exists; effectful application is fail-closed.                                |
| CORE-009 | Committed changes are represented in append-only authoritative history/event evidence.                      | PARTIAL        | Task activity/evidence is append-only; full authoritative event journaling is later migration work.              |
| CORE-010 | Recorded runs are replayable without repeating model inference or uncontrolled external side effects.       | PARTIAL        | R2 replays its bounded subjects; full replay parity is R10-R11.                                                  |
| CORE-011 | Nondeterministic external inputs are injected, captured, or explicitly marked unreplayable.                 | PARTIAL        | ADR 0029 and the nondeterminism ratchet cover current deterministic decisions; full observation capture is R10.  |
| CORE-012 | Deterministic code obtains time through an injected clock rather than arbitrary global reads.               | PARTIAL        | Current decision paths are ratcheted; complete H2 parity is R10.                                                 |
| CORE-013 | Randomness is injected and seeded/recorded where authoritative.                                             | PARTIAL        | Current authoritative paths avoid uncontrolled randomness; complete H2 parity is R10.                            |
| CORE-014 | Tool access is capability-scoped.                                                                           | VERIFIED       | Capability policy, ToolProjector, approval protocols, and architecture tests.                                    |
| CORE-015 | Context items carry provenance/authority information.                                                       | VERIFIED       | Projection, evidence, reference/research, and ICM contracts.                                                     |
| CORE-016 | Model claims are distinguishable from observed facts and authoritative state.                               | VERIFIED       | Task/evidence/knowledge authority classes and acceptance evaluation.                                             |
| CORE-017 | Domains cannot silently escalate capabilities.                                                              | PARTIAL        | ADR 0034 structurally constrains the selected ABI; production domain lifecycle is not implemented.               |
| CORE-018 | Durable state and protocols are schema/version identified.                                                  | PARTIAL        | Current schemas, execution contracts, corpus, WIT, and protocol docs are versioned; durable task state is later. |
| CORE-019 | Failed transactional operations cannot leave silently half-committed authoritative state.                   | PARTIAL        | Internal transaction/recovery tests exist; shipped mutation remains unavailable until enforceable.               |
| CORE-020 | Model-visible context and important runtime decisions are inspectable/explainable.                          | VERIFIED       | Briefs, projections, doctor, task activity, evidence, and structured dispositions.                               |

## Harness requirements

| ID      | Normative requirement                                                                                                               | Current status | Current evidence or owner                                                                       |
| ------- | ----------------------------------------------------------------------------------------------------------------------------------- | -------------- | ----------------------------------------------------------------------------------------------- |
| HAR-001 | Prefer structured patch/diff mutations over whole-file replacement when feasible.                                                   | PARTIAL        | Prepared edits/change sets exist; shipped writes remain unavailable.                            |
| HAR-002 | Maintain repository-aware context using symbols, references/dependencies, changed files and targeted retrieval.                     | VERIFIED       | WorkspaceScope, ActiveWorkingSet, structural reads, impact intelligence.                        |
| HAR-003 | Support an isolation boundary for risky command/code execution.                                                                     | PARTIAL        | Sandbox contracts/conformance exist; command execution is fail-closed.                          |
| HAR-004 | Provide checkpoints/rollback independent of Git being present.                                                                      | PARTIAL        | External checkpoint format/inspection exists; new checkpoint effects are unavailable.           |
| HAR-005 | Represent nontrivial task execution as explicit state, not an opaque recursive agent loop.                                          | VERIFIED       | Task runtime and workflow dispositions.                                                         |
| HAR-006 | Complex tasks may carry structured plans and acceptance criteria.                                                                   | VERIFIED       | TaskContract, host-controlled planning, AcceptanceEvaluator.                                    |
| HAR-007 | Verify meaningful mutations before final commitment where an appropriate verifier exists.                                           | PARTIAL        | Internal workflows enforce verification; shipped mutation is unavailable.                       |
| HAR-008 | Compiler, test, linter and static-analysis results can become structured observations for corrective reasoning.                     | PARTIAL        | Structured diagnostics/quality contracts exist; command runners are unavailable.                |
| HAR-009 | Corrective retries are bounded and terminate with a typed reason.                                                                   | VERIFIED       | Tool loop, repair loop, planner, and progress/stuck tests.                                      |
| HAR-010 | Detect identical proposals, repeated failures, state oscillation and no-progress cycles.                                            | VERIFIED       | Progress/stuck detection and bounded-loop behavior tests.                                       |
| HAR-011 | Bias toward minimal necessary edits.                                                                                                | VERIFIED       | Plans, workspace scope, active working sets, change-set preparation.                            |
| HAR-012 | Detect modifications outside declared/derived task scope and check workspace cleanliness before success.                            | VERIFIED       | Workspace revision, unexpected-change, scope, and acceptance evidence gates.                    |
| HAR-013 | Every tool has typed input/output schemas and explicit side-effect metadata.                                                        | VERIFIED       | Tool contracts, schemas, capability metadata, ToolProjector.                                    |
| HAR-014 | Tool access is capability scoped; unrestricted shell is not the universal privileged abstraction.                                   | VERIFIED       | Capability policy and fixed runner/tool catalogs.                                               |
| HAR-015 | Process execution is governed by command/path/environment policy and isolation rules.                                               | PARTIAL        | Contracts and tests exist; execution intentionally reports unavailable.                         |
| HAR-016 | Tool/model outputs have explicit size/resource limits and truncation semantics.                                                     | VERIFIED       | Provider, tool, search, Git, process, Godot, reference, and research bounds.                    |
| HAR-017 | Context compaction preserves provenance and explicitly indicates synthesized content.                                               | VERIFIED       | Projection and conversation-trim contracts.                                                     |
| HAR-018 | Context indexing/caching may be incremental, but cache state remains derived/reconstructable.                                       | PARTIAL        | Knowledge/context caches are derived; unsafe executable/materialization caches are unavailable. |
| HAR-019 | Context, tokens, model calls, tool calls, transactions, cost where measurable, and wall time support budgets.                       | PARTIAL        | Current context/tool/task/time budgets exist; cost and full transaction budgets are later.      |
| HAR-020 | Context compilation ordering is deterministic for the same authoritative inputs.                                                    | VERIFIED       | ContextProjector, Executor Brief Compiler, documentation selector tests.                        |
| HAR-021 | Core behavior is independent of any single model vendor.                                                                            | VERIFIED       | Provider port and deterministic fake adapter; no vendor concepts in core.                       |
| HAR-022 | Optional model routing is policy/orchestration rather than foundational state semantics.                                            | NOT DUE        | ADR 0036: a Profile selects a Provider; model-router architecture is not committed.             |
| HAR-023 | Prompt/context templates are versioned or content-addressed where identity matters.                                                 | PARTIAL        | Execution contract, plan/context fingerprints, and corpus digests exist.                        |
| HAR-024 | Runs record model identifier/profile and relevant generation configuration.                                                         | PARTIAL        | Provider identity is recorded in current requests/evidence; full run records are later.         |
| HAR-025 | Long-running tool/task execution is cancellable.                                                                                    | VERIFIED       | Abort-aware tool/provider/planner/service contracts and adversarial tests.                      |
| HAR-026 | Durable tasks can resume after process restart.                                                                                     | NOT DUE        | Persistence remains deferred.                                                                   |
| HAR-027 | Runtime emits structured events usable by CLI, TUI, IDE and API clients.                                                            | PARTIAL        | Task activity and model/tool events exist; durable multi-client runtime events are later.       |
| HAR-028 | Errors and diagnostics contain machine-readable categories/context rather than only display strings.                                | VERIFIED       | Typed outcomes and doctor/Godot/tool diagnostics.                                               |
| HAR-029 | Execution traces are serializable and machine-readable.                                                                             | PARTIAL        | R2 records and task/evidence snapshots are serializable; full traces are R10-R11.               |
| HAR-030 | Artifacts record provenance: creator/cause/run/input revisions where applicable.                                                    | VERIFIED       | Evidence, knowledge, references, plans, tasks, and change sets carry provenance/revisions.      |
| HAR-031 | Immutable large artifacts and replay inputs are content addressed where practical.                                                  | PARTIAL        | R2 corpus and canonical identities are digest-bound; a general artifact store is later.         |
| HAR-032 | Model-generated mutations carry revision preconditions to prevent stale writes.                                                     | VERIFIED       | Prepared mutation/change-set contracts and stale-state tests.                                   |
| HAR-033 | Concurrent external changes are detected rather than silently overwritten.                                                          | VERIFIED       | Revision/hash conflicts and fail-closed effect posture.                                         |
| HAR-034 | Filesystem access resists traversal, symlink escape and scope confusion.                                                            | PARTIAL        | Read surfaces are bounded/no-follow; unsafe effectful surfaces are unavailable.                 |
| HAR-035 | Repository text/tool output is potentially hostile data and cannot automatically redefine system policy.                            | VERIFIED       | Authority classes, sanitizer, protected instructions, and projection rules.                     |
| HAR-036 | Context distinguishes authority/trust classes such as system, user, observed repository data, tool observation and model synthesis. | VERIFIED       | ICM/projection/evidence/reference/research contracts.                                           |
| HAR-037 | Secrets stay out of normal context whenever tools can consume credentials out of band.                                              | VERIFIED       | Credential isolation and safe-report tests; real providers are not yet integrated.              |
| HAR-038 | Network access has explicit policy: offline/restricted/allowlisted/unrestricted.                                                    | VERIFIED       | Built-in profiles and research/network policy.                                                  |
| HAR-039 | Domain packages declare requested capabilities before activation.                                                                   | NOT DUE        | Stage 3R R6 under the ADR 0036 Permission -> CapabilityRequest -> CapabilityGrant model.        |
| HAR-040 | Third-party package ecosystems require digest/provenance/signature policy before being treated as trusted.                          | PARTIAL        | Supply-chain and ADR 0034 identity policy exist; Plugin ecosystem is not due (ADR 0036).        |
| HAR-041 | Core/domain behavior is validated with fixture-driven integration tests.                                                            | VERIFIED       | Behavior suites, Godot fixtures, R2 corpus, ABI conformance.                                    |
| HAR-042 | Representative full runs are stored as golden traces and replayed in CI when that replay layer is due.                              | NOT DUE        | Registry exists; full replay layer is R10-R11.                                                  |
| HAR-043 | Core algebra/parsers/protocols receive property testing and fuzzing proportional to risk.                                           | PARTIAL        | Version property/fuzz infrastructure exists; coverage expands with migrated subsystems.         |
| HAR-044 | Maintain an adversarial evaluation suite.                                                                                           | PARTIAL        | Adversarial unit/behavior/conformance suites exist; full evaluation program is later.           |
| HAR-045 | Run equivalent evaluation tasks across multiple models/providers where economically practical.                                      | NOT DUE        | Real providers are not implemented; Stage 6 /evolve evaluation under ADR 0036.                  |
| HAR-046 | Track task success, invalid proposals, unnecessary edits, rollback rate, context efficiency and intervention rate over time.        | PARTIAL        | Current task/projection metrics exist; longitudinal evaluation storage is later.                |
| HAR-047 | Dogfood Siralos on Siralos with recorded metrics once usable.                                                                       | NOT DUE        | Requires later usable effect/runtime milestones.                                                |
| HAR-048 | CI enforces formatting, Clippy, compilation and tests; warnings normally fail project code.                                         | VERIFIED       | `npm run check`, Rust matrix, warnings-denied Clippy.                                           |
| HAR-049 | Rust APIs favor explicit strong types, simple ownership, clear invariants and measured optimization.                                | VERIFIED       | Rust style guide, architecture gate, Clippy, benchmarks.                                        |
| HAR-050 | Rust migration removes obsolete/duplicated legacy paths instead of indefinitely maintaining parallel architectures.                 | NOT DUE        | Enforced as an R3-R12 porting rule; no major subsystem has been ported yet.                     |
| HAR-051 | Documentation/RFC/schema drift is detected during appropriate verification/release review.                                          | PARTIAL        | Documentation, architecture, identity, public-hygiene, and differential ratchets exist.         |
| HAR-052 | Keep the stable foundational core small; features belong in higher layers/extensions where possible.                                | VERIFIED       | Dependency architecture and Rust core neutrality ratchets.                                      |
| HAR-053 | Do not let Godot, a model provider, a UI or a transport protocol define Siralos Core semantics.                                     | PARTIAL        | Rust core is neutral; the TypeScript oracle still owns historical Godot contracts.              |
| HAR-054 | Defer multi-agent sophistication until deterministic single-agent execution satisfies quality gates.                                | VERIFIED       | Multi-agent functionality is absent and is not core architecture (ADR 0036).                    |
| HAR-055 | Avoid hidden magic: implicit detection, implicit permissions, invisible context injection or silent state mutation.                 | VERIFIED       | Explicit profiles, projections, task state, capability decisions, and diagnostics.              |

## Anti-pattern register

These are permanent design prohibitions, not optional style preferences.

| ID     | Anti-pattern                                                                         | Current enforcement |
| ------ | ------------------------------------------------------------------------------------ | ------------------- |
| AP-001 | Model owns authoritative control loop/state.                                         | VERIFIED            |
| AP-002 | Accumulated chat history is treated as authoritative state.                          | VERIFIED            |
| AP-003 | Unrestricted shell is the universal privileged tool.                                 | VERIFIED            |
| AP-004 | Permissions are inferred merely from natural-language intent.                        | VERIFIED            |
| AP-005 | Domain installation/enablement is triggered silently from workspace contents.        | PARTIAL             |
| AP-006 | Git is required for transactional correctness or rollback.                           | VERIFIED            |
| AP-007 | Command/file-write success is equated with task success.                             | VERIFIED            |
| AP-008 | Prompt history grows indefinitely without provenance/budget-aware compaction.        | VERIFIED            |
| AP-009 | Retries are unbounded.                                                               | VERIFIED            |
| AP-010 | Multi-agent machinery compensates for an unreliable single-agent runtime.            | VERIFIED            |
| AP-011 | Prompt cleverness is optimized before evaluation infrastructure.                     | PARTIAL             |
| AP-012 | Legacy architecture is mechanically reproduced in Rust.                              | VERIFIED            |
| AP-013 | Cache state becomes hidden authoritative state.                                      | VERIFIED            |
| AP-014 | Repository/tool text can supersede system/security policy.                           | VERIFIED            |
| AP-015 | Package availability, installation, enablement and runtime activation are conflated. | NOT DUE             |
| AP-016 | Domain API is frozen around Godot-only assumptions.                                  | PARTIAL             |

`PARTIAL` and `NOT DUE` entries remain binding requirements. They are not
defect waivers and cannot be promoted without the applicable milestone's
executable evidence.
