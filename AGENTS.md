# AGENTS.md

## Repository

- npm workspace monorepo using ESM, strict TypeScript, and project references.
- `@solaris/core` owns application policy and domain contracts. It must not import adapters.
- `@solaris/adapters` implements provider, workspace, sandbox, Git, Godot, reference, research, and planning ports.
- `@solaris/cli` is the composition root and the only interactive terminal surface.
- Treat [README.md](README.md) as the user-facing status, [ROADMAP.md](ROADMAP.md) as milestone status, [ARCHITECTURE.md](ARCHITECTURE.md) as dependency ownership, [SECURITY.md](SECURITY.md) as the security contract, and `docs/adr/` as the decision history. Do not duplicate those documents here. Use [docs/architecture/README.md](docs/architecture/README.md) as the architecture index; ADR metadata (id/status/domains/paths/supersedes) lives in each ADR's frontmatter.

## Current implementation

- Stage 3 milestones 1–8 are implemented: task runtime, projections, workspace revisions and structural reads, project instructions and knowledge, references and research, self-reference and capability diagnostics, host-controlled planning, and read-only Godot scene/resource intelligence.
- The next narrow milestone is Stage 3 milestone 9. Do not start scene/resource mutation.
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
- Behavioral configuration (`AGENTS.md` at any depth and `.solaris/**`) is protected and cannot be changed through ordinary workspace mutation capability.
- Capability policy, one-time digest-bound approval, sandbox enforcement, checkpointing, and stale-state checks are independent gates. Success at one gate never implies another.
- Keep external references outside the workspace namespace. `@reference/<alias>` is not a filesystem path.
- Research is disabled by built-in profiles unless explicitly authorized. `ask` is refused where no approval protocol exists.
- Architecture checks are developer guardrails, not an OS security boundary.
- Never expose absolute workspace, cache, mirror, executable, or credential paths to providers or report-safe output.
- The terminal sanitizer is the single output boundary, the input queue is the single interactive-read owner, and the command catalog is the single command-vocabulary source.
- Self-reference and doctor collection are read-only and offline by default: no refresh, live probe, repair, permission broadening, mutation, checkpoint, or secret-bearing report. `ToolProjector` remains authoritative for model-visible tools and `SandboxBackend` for enforcement capability.
- Keep fixed Godot invocation tuples inside their architecture-owned runner modules. Project-independent probes never accept project arguments; recovery, check-only, and LSP-only flags remain structurally paired even while every runner is unavailable.

## Verification

- `npm run check` — format check, lint, typecheck, unit/integration tests, and architecture checks.
- `npm run check:architecture` — dependency and source guardrails.
- `npm run test:sandbox` — live sandbox conformance; unavailable setup skips loudly and is never a pass.
- `SOLARIS_TEST_GODOT="<absolute-path>" npm run test:godot` — opt-in live Godot probe conformance.
- `SOLARIS_TEST_GODOT="<absolute-path>" npm run test:godot-recovery` — verifies truthful fail-closed recovery behavior; execution and isolation probes remain unavailable/skipped.
- `npm run solaris` — build and launch the CLI.

Run checks relevant to each cohesive change before committing, then run `npm run check` before handoff. Use small Conventional Commit-style commits; do not put an entire multi-boundary task in one commit.

## Intended direction

- Solaris is an independent provider-neutral agent harness for Godot development.
- Prefer real Godot projects when project scaffolding begins and record new verification commands here when they become usable.
- Next: read-only `.tscn`/`.tres` parsing, UID and resource relationships, scene inheritance, node ownership, script attachments, signals, autoloads, project settings, structured inspection tools, and revision-aware evidence.
- Do not add real provider integrations, persistence, multi-agent functionality, or `/evolve` as part of that milestone.
