# Architecture index

This file is a **map**, not an architecture document. It resolves a code
domain to its source paths, its architecture documentation, and its
applicable ADRs so that executor-context discovery is deterministic
(ADR 0023 Parts M–O). The authoritative architecture content stays in
[ARCHITECTURE.md](../../ARCHITECTURE.md), engineering rules in
[ENGINEERING.md](../../ENGINEERING.md), and the security contract in
[SECURITY.md](../../SECURITY.md); this index only points at them.

Machine-readable metadata lives in the ADR frontmatter
(`id` / `status` / `domains` / `paths` / `supersedes`) and the runtime
documentation index (`packages/core/src/executor/documentation-context.ts`).
Keep this page and the runtime index consistent; the architecture check
(`npm run check:architecture`) validates frontmatter against the runtime
index.

## Selection order

Normal executor-context discovery follows: root `AGENTS.md` → applicable
nested `AGENTS.md` → this index → mapped subsystem docs → applicable
accepted ADRs. Superseded/deprecated and `docs/archive/` material is
excluded unless historical reasoning is explicitly requested. Never
recursively ingest `docs/`.

## Scoped guidance

Path-scoped `AGENTS.md` files exist only where a directory has
meaningful domain-specific guidance; the runtime documentation index
maps them by source-path globs:

| Scoped file                             | Covers                                             |
| --------------------------------------- | -------------------------------------------------- |
| `packages/core/AGENTS.md`               | core domain contracts, executor-context discipline |
| `packages/adapters/AGENTS.md`           | adapter fail-closed discipline, port ownership     |
| `packages/adapters/src/godot/AGENTS.md` | Godot static-inspection domain rules               |
| `apps/cli/AGENTS.md`                    | CLI composition-root and terminal-boundary rules   |

## Domain map

| Domain                                                                   | Source paths                                                                                                                                 | Architecture                                                                                                       | ADRs                                                       |
| ------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------- |
| Task runtime                                                             | `packages/core/src/tasks/**`                                                                                                                 | ARCHITECTURE.md § Task runtime                                                                                     | ADR-0014                                                   |
| Content identity / deltas                                                | `packages/core/src/identity/**`                                                                                                              | ADR-0028                                                                                                           | ADR-0028                                                   |
| Deterministic execution / reproducibility                                | `packages/core/src/determinism/**`, `scripts/check-nondeterminism.mjs`                                                                       | ADR-0029                                                                                                           | ADR-0029                                                   |
| Interpretable context architecture                                       | `packages/core/src/context/**`                                                                                                               | ADR-0030                                                                                                           | ADR-0030                                                   |
| Runtime readiness / operational resilience                               | `packages/core/src/runtime/**`                                                                                                               | ADR-0031                                                                                                           | ADR-0031                                                   |
| Projection / context                                                     | `packages/core/src/projection/**`                                                                                                            | ARCHITECTURE.md § Context, tool, and evidence projection                                                           | ADR-0015, ADR-0002                                         |
| Workspace revisions / reads                                              | `packages/core/src/workspace/**`                                                                                                             | ARCHITECTURE.md § Workspace revision and structural reads                                                          | ADR-0016                                                   |
| Workspace mutations                                                      | `packages/core/src/workspace/**`, `packages/adapters/src/tools/workspace/**`                                                                 | ARCHITECTURE.md § Approved mutations                                                                               | ADR-0005                                                   |
| Instructions / knowledge                                                 | `packages/core/src/instructions/**`, `packages/core/src/knowledge/**`                                                                        | ARCHITECTURE.md § Project instructions and knowledge                                                               | ADR-0017                                                   |
| References / research                                                    | `packages/core/src/reference/**`, `packages/core/src/research/**`, `packages/adapters/src/reference/**`, `packages/adapters/src/research/**` | ARCHITECTURE.md § Workspace, reference, and research resource classes                                              | ADR-0018                                                   |
| Self-reference / doctor                                                  | `packages/core/src/self/**`, `packages/core/src/doctor/**`                                                                                   | ARCHITECTURE.md § Self-reference and capability diagnostics                                                        | ADR-0019                                                   |
| Planning                                                                 | `packages/core/src/planning/**`, `packages/adapters/src/planning/**`                                                                         | ARCHITECTURE.md § Host-controlled planning                                                                         | ADR-0020                                                   |
| Executor briefing                                                        | `packages/core/src/executor/**`                                                                                                              | ADR-0022, ADR-0023                                                                                                 | ADR-0022, ADR-0023                                         |
| Godot static inspection                                                  | `packages/core/src/godot/**`, `packages/adapters/src/godot/**`                                                                               | ARCHITECTURE.md § Godot engine discovery and profiling                                                             | ADR-0021, ADR-0008–0013                                    |
| Godot impact analysis                                                    | `packages/core/src/godot/impact/**`, `packages/adapters/src/godot/intelligence/**`                                                           | ARCHITECTURE.md § Godot sections                                                                                   | ADR-0025                                                   |
| Godot native mutation                                                    | `packages/core/src/godot/scene-mutation/**`, `packages/adapters/src/godot/scene-mutation/**`                                                 | ARCHITECTURE.md § Godot sections                                                                                   | ADR-0026                                                   |
| Godot discovery / recovery / knowledge / diagnostics / LSP / development | `packages/adapters/src/godot/**`, `packages/core/src/godot/**`                                                                               | ARCHITECTURE.md § Godot sections                                                                                   | ADR-0008, ADR-0009, ADR-0010, ADR-0011, ADR-0012, ADR-0013 |
| Providers / tool loop                                                    | `packages/core/src/ports/**`, `packages/adapters/src/providers/**`                                                                           | ARCHITECTURE.md § Tool loop, § Adapters                                                                            | ADR-0002                                                   |
| Security / sandbox / capability                                          | `packages/core/src/security/**`                                                                                                              | SECURITY.md, ARCHITECTURE.md § Security model                                                                      | ADR-0004, ADR-0007                                         |
| Git / checkpoints                                                        | `packages/adapters/src/git/**`, `packages/adapters/src/checkpoints/**`                                                                       | ARCHITECTURE.md § Git inspection, § Recovery checkpoints                                                           | ADR-0006                                                   |
| Process execution                                                        | `packages/adapters/src/process/**`                                                                                                           | ARCHITECTURE.md § Command execution                                                                                | ADR-0007                                                   |
| Overall architecture                                                     | `packages/**`                                                                                                                                | ARCHITECTURE.md                                                                                                    | ADR-0001                                                   |
| Rust candidate implementation (Stage 3R)                                 | `crates/**`                                                                                                                                  | ARCHITECTURE.md § Rust candidate workspace, `docs/development/RUST_STYLE.md`, `docs/development/RUST_MIGRATION.md` | ADR-0032                                                   |
| Differential behavioral harness (Stage 3R R2)                            | `tests/differential/**`, `crates/siralos-cli/src/harness/**`                                                                                 | ADR-0033                                                                                                           | ADR-0033                                                   |
| Domain host ABI (Godot domain boundary)                                  | `experiments/domain-abi/**`                                                                                                                  | ADR-0034                                                                                                           | ADR-0034                                                   |
| Controlled runtime boundary                                              | `packages/core/src/runtime/**`, `experiments/domain-abi/**`                                                                                  | `docs/development/PROJECT_CONTEXT.md`, `docs/development/RUST_MIGRATION.md`                                        | ADR-0031, ADR-0035                                         |
| Product vision / composition & extension model                           | README.md, ROADMAP.md, ARCHITECTURE.md, `docs/development/PROJECT_CONTEXT.md`, `docs/development/RUST_MIGRATION.md`                          | ARCHITECTURE.md § Product vision and conceptual ownership                                                          | ADR-0036                                                   |
| Migration and requirements traceability                                  | `crates/**`, `tests/differential/**`, milestone manifests                                                                                    | `docs/development/RUST_MIGRATION.md`, `docs/requirements/REQUIREMENTS.md`, `docs/architecture/RFC_INDEX.md`        | ADR-0032, ADR-0033                                         |
| Public context and repository hygiene                                    | `scripts/check-public-hygiene.mjs`, public documentation                                                                                     | `docs/development/PROJECT_CONTEXT.md`, `docs/development/GOLDEN_TRACES.md`                                         | ADR-0022, ADR-0023                                         |

## ADR status

All ADRs are `accepted` and current unless their frontmatter says
otherwise. ADR 0003 was never adopted; there are no superseded or
deprecated ADRs today. Obsolete historical material, when it exists,
lives in `docs/archive/` and is excluded from normal discovery.

ADR 0036 is the lean product, composition, and extension model (the
pre-R3 vision freeze). It narrows several earlier future-facing
statements without superseding their ADRs; earlier ADRs remain the
historical decision records.
