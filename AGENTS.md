# AGENTS.md

## New session / coding-agent bootstrap

1. Read [docs/development/PROJECT_CONTEXT.md](docs/development/PROJECT_CONTEXT.md).
2. Read the scoped `AGENTS.md` for files being modified.
3. Follow linked architecture, ADR, security, and style documents as applicable.
   For future-facing product or architecture decisions, read
   [ADR 0036](docs/adr/0036-lean-product-composition-and-extension-model.md)
   (the lean product, composition, and extension model).
4. Verify current milestone status from repository evidence before claiming completion.

**Standing rule: self-loop on every prompt** — every prompt (human or follow-up, Wayfinder or direct implementation) must invoke the `self-loop` skill and follow its `references/verification-protocol.md` ledger (criterion → evidence → pass/fail/unknown → challenge lenses → repair), even when another skill says one ticket per session. Skill load is per-prompt; loop budget is one coherent pass + up to two repairs unless the prompt explicitly changes budget. Also see `docs/wayfinder/siralos-roadmap.md:Notes` for the Wayfinder-persistent form of this rule.

5. If `docs/wayfinder/siralos-roadmap.md` exists on the filesystem, also read its **Destination** and **Notes** (the Wayfinder map). It names the decision-ready route R7.5 → R12 → Stage 4 and the current frontier; Decisions so far are the index of closed tickets (refer by name, never bare id). The map is local-markdown fallback (label `wayfinder:map` on a hosted tracker) — read the file directly, not the `docs/wayfinder/tickets/` children, unless a ticket is claimed.

## Repository

- Rust workspace monorepo (Cargo) with minimal Node harness; live TypeScript tree **removed** (decision 40, freeze at `5da5cde`, v32 234/234, pinned at `tests/differential/evidence/typescript-freeze-v32/`).
- Stage 3R adds the Rust implementation: a Cargo workspace (`Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`) with `crates/siralos-core` (domain-neutral host semantics), `crates/siralos-adapters` (infrastructure), and `crates/siralos-cli` (the `siralos` binary); `siralos-godot` now lives in standalone repo `https://github.com/CrimsX/siralos-godot` at `1bf2ca3` (was an in-repository crate between `5da5cde` and `5da2cab`, 41 files + host adapters at `1bf2ca3` self-contained, `siralos-godot → siralos-core` only via `path = "../siralos/crates/siralos-core"`), pinned in the monorepo as `siralos-godot = { path = "../siralos-godot" }` (3-member workspace, external path dep). Dependency direction: `cli → adapters → core`, `godot → core` (external); core must never depend on infrastructure or a domain. All Rust code follows [docs/development/RUST_STYLE.md](docs/development/RUST_STYLE.md) (authoritative; do not restate it). The TypeScript historical oracle is **archived** at `5da5cde` (freeze v32, 234/234, pinned) and retained only as digest-bound evidence; differential harness runs in **pinned mode** (historical replay requires worktree at freeze SHA); Rust is the sole source of truth per ADR 0032 (decision 40, honest trade-off C7).
- Treat [README.md](README.md) as the user-facing status, [ROADMAP.md](ROADMAP.md) as milestone status, [ARCHITECTURE.md](ARCHITECTURE.md) as dependency ownership, [SECURITY.md](SECURITY.md) as the security contract, and `docs/adr/` as the decision history. Do not duplicate those documents here. Use [docs/architecture/README.md](docs/architecture/README.md) as the architecture index; ADR metadata (id/status/domains/paths/supersedes) lives in each ADR's frontmatter.

## Current implementation

- Implementation status, verified milestones, and the current frontier are
  recorded only in [ROADMAP.md](ROADMAP.md), the canonical status source. This
  file states rules; it does not restate status.
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

These capabilities remain unavailable by a **ported-parity decision**, not by a language limitation. The Rust implementation was ported to byte-match a frozen TypeScript oracle that also lacked an identity-bound commit primitive, and the differential corpus **pins that outcome**: `tests/differential/corpus/workspace-apply.apply-unavailable.json` and `tests/differential/corpus/workspace-prepare.unavailable.json` are among the scenarios asserting a typed `unavailable`. Flipping any of these capabilities is therefore a deliberate, reviewed oracle amendment — never a code change alone. The earlier rationale in this file blamed Node; that was true of the removed TypeScript tree and is obsolete. Static preparation contracts and truthful diagnostics may exist, but they must refuse before approval, checkpoint creation, mirror/cache creation, deletion, or spawn.

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

- `npm run check` — format check (prettier, minimal), lint (eslint, minimal), doc links, project-context, identity, public-hygiene, Rust architecture check, and the full Rust gate (fmt, clippy with warnings denied, tests, pinned differential).
- `npm run check:identity` — no project-owned file may use the former identity (narrow documented exclusions only).
- `npm run check:rust` — Rust crate shape, dependency direction, binary identity, edition/toolchain/formatting policy, core domain neutrality, unsafe backstop.
- `npm run check:differential` — the differential behavioral harness (ADR 0033) in **pinned mode** (decision 40): runs the scenario corpus against the **pinned** TypeScript oracle (`tests/differential/evidence/typescript-freeze-v32/oracle.json`) and the Rust candidate, compares canonical outcome records, and emits the migration audit report (exit 0 = parity, 1 = deviation, 2 = harness error). Live oracle requires worktree at freeze SHA (`5da5cde`) and is never part of `check`.
- `npm run build:domain-conformance` — rebuilds the synthetic conformance domain components (Stage 3R R6) and refreshes the checked-in fixture bytes; run it whenever the production WIT or a conformance guest changes, and update the fixture digest constants in the same change.
- `npm run check:rust-format` / `npm run check:rust-clippy` / `npm run test:rust` — `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace`.
- `npm run siralos` — build and launch the Rust CLI (`cargo run --locked --bin siralos -- --help` / `--version`).

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
- The TypeScript implementation was the behavioral reference (archived at `5da5cde`, pinned at `tests/differential/evidence/typescript-freeze-v32/`); later Stage 3R milestones ported subsystems to idiomatic Rust under behavioral parity, refactoring-during-port, and evidence-driven optimization rules (ADR 0032, `docs/development/RUST_STYLE.md`). Rust is now the sole source of truth (decision 40).
- Current milestone status, verification records, and the current frontier live
  only in [ROADMAP.md](ROADMAP.md), the canonical status source. Do not restate
  them here.
- Do not add persistence, multi-agent functionality, or `/evolve` execution outside their planned milestones — `/evolve` display-only discovery exists since decision 111 while execution remains host-gated/out-of-milestone; real provider integrations are **Verified** per decisions 66–71 (user-directed 2026-08-31) — the earlier AGENTS.md “no real provider” bullet is superseded.
