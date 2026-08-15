# AGENTS.md

## New session / coding-agent bootstrap

1. Read [docs/development/PROJECT_CONTEXT.md](docs/development/PROJECT_CONTEXT.md).
2. Read the scoped `AGENTS.md` for files being modified.
3. Follow linked architecture, ADR, security, and style documents as applicable.
   For future-facing product or architecture decisions, read
   [ADR 0036](docs/adr/0036-lean-product-composition-and-extension-model.md)
   (the lean product, composition, and extension model).
4. Verify current milestone status from repository evidence before claiming completion.

## Repository

- npm workspace monorepo using ESM, strict TypeScript, and project references.
- `@siralos/core` owns application policy and domain contracts. It must not import adapters.
- `@siralos/adapters` implements provider, workspace, sandbox, Git, Godot, reference, research, and planning ports.
- `@siralos/cli` is the composition root and the only interactive terminal surface.
- Stage 3R adds the Rust candidate implementation: a Cargo workspace (`Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`) with `crates/siralos-core` (domain-neutral host semantics), `crates/siralos-adapters` (infrastructure), and `crates/siralos-cli` (the `siralos` binary). Dependency direction: cli → adapters → core; core must never depend on infrastructure or a domain. All Rust code follows [docs/development/RUST_STYLE.md](docs/development/RUST_STYLE.md) (authoritative; do not restate it). The TypeScript implementation is the behavioral reference/migration oracle (ADR 0032) — do not delete or redesign it.
- Treat [README.md](README.md) as the user-facing status, [ROADMAP.md](ROADMAP.md) as milestone status, [ARCHITECTURE.md](ARCHITECTURE.md) as dependency ownership, [SECURITY.md](SECURITY.md) as the security contract, and `docs/adr/` as the decision history. Do not duplicate those documents here. Use [docs/architecture/README.md](docs/architecture/README.md) as the architecture index; ADR metadata (id/status/domains/paths/supersedes) lives in each ADR's frontmatter.

## Current implementation

- Stage 3 is complete through milestone 11: task runtime, projections, workspace revisions and structural reads, project instructions and knowledge, references and research, self-reference and capability diagnostics, host-controlled planning, read-only Godot scene/resource intelligence, review context and impact intelligence, approved scene/resource mutation, and the unified `/develop` workflow (ADRs 0025–0027).
- Stage 3R R1 is complete: project renamed to Siralos, Rust engineering guide established, domain-neutral Rust workspace skeleton (see Repository above). Stage 3R R2 is complete: the Differential Behavioral Harness (ADR 0033) is the migration's audit remediation gate — it verifies a scenario- and corpus-digest-bound fixture set, supervises both runners symmetrically, semantically compares typed canonical outcomes, emits the migration audit, and gates remediation; its first subjects (state-dir resolution, product version identity) hold parity. Stage 3R R3 is complete: the domain-neutral host-owned task kernel in `siralos-core` (contracts, state, transitions, evidence, acceptance, completion gate, activity, progress) holds byte parity across 17 differential `task-contract` scenarios. Stage 3R R4 is complete: the generic workspace/project foundation — `siralos-core::workspace` (validated relative paths, bounds, revision handles/registry, prepared-effect models, checkpoint contracts/invariants/undo/reconciliation classification, Git disposition) and `siralos-adapters::workspace` (canonical root, containment-safe resolution, bounded complete exact reads, deterministic list/search, fail-closed mutation preparation, checkpoint storage inspection/reconciliation, typed unavailable Git) — holds differential parity across 23 `workspace-read`/`workspace-list`/`workspace-search`/`workspace-revision`/`workspace-prepare`/`checkpoint`/`git-inspection` scenarios. Structural/summary read modes remain typed unsupported dispositions (GDScript extraction is R5-owned); mutation application, new checkpoint creation, and Git inspection remain deliberately unavailable exactly as the reference reports them. Do not port major subsystems ahead of R5, and do not add a Godot package, placeholder domains, or a marketplace/plugin ecosystem.
- The cross-cutting executor briefing and context discipline (ADRs 0022–0023) is implemented: a versioned Execution Contract, milestone manifests with stable acceptance IDs (S3M8 has a real manifest), an evidence-backed AcceptanceEvaluator, the Executor Context Pack, the deterministic Executor Brief Compiler, `/brief` / `/milestone` inspection, task `WorkspaceScope` with verified/candidate files and budgets, current-step `ActiveWorkingSet` with inclusion reasons, new-file rationale/proliferation signals, and deterministic documentation selection (root + scoped AGENTS.md, architecture index, accepted ADRs; archive/superseded excluded). Briefs reference the execution contract by revision and never restate permanent rules; milestone acceptance is satisfied only by host-observed evidence, never executor claims; workspace scope and documentation selection are derived context that never grants capability.
- Working read-only surfaces include the deterministic fake provider, bounded workspace list/read/search, static Godot installation and project inspection, local-directory references, denied-by-default bounded research adapters, self-reference, capability diagnostics, and the interactive CLI.
- Task contracts, snapshots, plans, evidence, provider requests, tool definitions, and public result values must be detached from caller-owned mutable data. Task state is host-owned; model completion is only a request evaluated by host gates.
- Provider streams and tool loops are bounded, protocol-checked, cancellation-aware, transcript-paired, and sanitized at the terminal boundary. Provider output and external content are always untrusted data.
- Planning is host-routed and structurally read-only. Plan approval binds only to the exact plan and task revisions and never grants edit, command, checkpoint, sandbox, or research authority.
- Project instructions, project knowledge, evidence/history, references, and research are separate authority classes. None may be promoted implicitly, and none may override capability or sandbox policy.

## Fail-closed execution posture

The following surfaces intentionally report `unavailable` and perform no filesystem mutation or process launch:

- workspace create/edit/delete application and safe undo;
- new checkpoint creation and automatic checkpoint pruning;
- private run-directory creation or cleanup;
- `node-script` and `npm-script` command execution;
- Git inspection;
- Godot engine probes, API-dump generation, recovery project probes, GDScript check-only diagnostics, and GDScript LSP startup;
- executable caches, Godot knowledge caches, recovery mirrors, and repository-reference materialization.

These capabilities remain unavailable because Node does not provide the directory-relative create/replace/delete or identity-bound executable launch primitives needed to resist same-user pathname substitution. Static preparation contracts and truthful diagnostics may exist, but they must refuse before approval, checkpoint creation, mirror/cache creation, deletion, or spawn.

Do not weaken this posture with another pathname recheck, hashing window, private filename, monkey patch, comment, warning, or documentation claim. A capability becomes available only when its security property is mechanically enforceable and covered by adversarial tests.

Historical checkpoint data may be inspected, but unverifiable or unexpected content blocks capacity checks and is never repaired or deleted automatically. The logical checkpoint byte limit counts exact metadata and preimage bytes; preimages are handle-bound, bounded, content-verified, and stability-checked.

The Anthropic Sandbox Runtime backend is pinned. Linux/macOS availability requires the enforced host-read allowlist and live conformance. Windows setup and host-read capability are distinct; the backend must never be reported generally executable when enforcement is unavailable. A skipped live probe is never a pass.

## Workspace and security rules

- Canonicalize the launch workspace once and contain every model-facing path within it. Never follow workspace symlinks for traversal.
- Behavioral configuration (`AGENTS.md` at any depth and `.siralos/**`) is protected and cannot be changed through ordinary workspace mutation capability.
- Capability policy, one-time digest-bound approval, sandbox enforcement, checkpointing, and stale-state checks are independent gates. Success at one gate never implies another.
- Keep external references outside the workspace namespace. `@reference/<alias>` is not a filesystem path.
- Research is disabled by built-in profiles unless explicitly authorized. `ask` is refused where no approval protocol exists.
- Architecture checks are developer guardrails, not an OS security boundary.
- Never expose absolute workspace, cache, mirror, executable, or credential paths to providers or report-safe output.
- The terminal sanitizer is the single output boundary, the input queue is the single interactive-read owner, and the command catalog is the single command-vocabulary source.
- Self-reference and doctor collection are read-only and offline by default: no refresh, live probe, repair, permission broadening, mutation, checkpoint, or secret-bearing report. `ToolProjector` remains authoritative for model-visible tools and `SandboxBackend` for enforcement capability.
- Keep fixed Godot invocation tuples inside their architecture-owned runner modules. Project-independent probes never accept project arguments; recovery, check-only, and LSP-only flags remain structurally paired even while every runner is unavailable.

## Verification

- `npm run check` — format check, lint, typecheck, unit/integration tests, architecture checks, identity ratchet, Rust architecture check, and the full Rust gate (fmt, clippy with warnings denied, tests).
- `npm run check:architecture` — TypeScript dependency and source guardrails.
- `npm run check:identity` — no project-owned file may use the former identity (narrow documented exclusions only).
- `npm run check:rust` — Rust crate shape, dependency direction, binary identity, edition/toolchain/formatting policy, core domain neutrality, unsafe backstop.
- `npm run check:differential` — the differential behavioral harness (ADR 0033): runs the scenario corpus against the TypeScript oracle and the Rust candidate, compares canonical outcome records, and emits the migration audit report (exit 0 = parity, 1 = deviation, 2 = harness error).
- `npm run check:rust-format` / `npm run check:rust-clippy` / `npm run test:rust` — `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace`.
- `npm run test:sandbox` — live sandbox conformance; unavailable setup skips loudly and is never a pass.
- `SIRALOS_TEST_GODOT="<absolute-path>" npm run test:godot` — opt-in live Godot probe conformance.
- `SIRALOS_TEST_GODOT="<absolute-path>" npm run test:godot-recovery` — verifies truthful fail-closed recovery behavior; execution and isolation probes remain unavailable/skipped.
- `npm run siralos` — build and launch the (TypeScript) CLI; the Rust binary is built with `cargo build` and run as `siralos --version` / `siralos --help`.

Run checks relevant to each cohesive change before committing, then run `npm run check` before handoff. Use small Conventional Commit-style commits; do not put an entire multi-boundary task in one commit.

## Intended direction

- The lean product, composition, and extension model
  ([ADR 0036](docs/adr/0036-lean-product-composition-and-extension-model.md))
  governs future-facing work; read it (and
  [PROJECT_CONTEXT.md](docs/development/PROJECT_CONTEXT.md)) before
  product/architecture decisions. Multi-agent machinery, general Hooks,
  TaskGraph, workflow engines, marketplaces, and automatic acquisition
  are not committed.
- Siralos is a minimal, declarative AI coding harness with an inspectable execution environment, a domain-neutral core, and explicitly installed optional domain intelligence (Godot is the first and only optional domain).
- The TypeScript implementation is the behavioral reference; later Stage 3R milestones port subsystems to idiomatic Rust under behavioral parity, refactoring-during-port, and evidence-driven optimization rules (ADR 0032, `docs/development/RUST_STYLE.md`).
- Next: Stage 3R R5 — Generic Language Intelligence (behavior extraction → idiomatic Rust redesign → parity → review → measurement, ADR 0032/0033). Do not implement R5+ porting work in the current milestone.
- Do not add real provider integrations, persistence, multi-agent functionality, or `/evolve` outside their planned milestones.
