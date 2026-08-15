# Architecture

## Overview

Siralos is a modular monolith: one repository, one npm workspace, one process, and clearly separated layers. See `docs/adr/0001-modular-monolith.md` for the decision record.

## Product vision and conceptual ownership (ADR 0036)

The committed product vision (ADR 0036) is a small privileged Host composed
with declarative Profiles, inspectable Context, declarative Skills,
capability-scoped Plugins, bounded Runs, and a measured Evolve workflow:

```text
+---------------------------------+
|      USER CONFIGURATION         |
| Profile · Context · Skills      |
+----------------+----------------+
                 |
                 v
+---------------------------------+
|         SIRALOS HOST            |
| State · Revision · Capability   |
| Tools · Effects · Evidence      |
+----------------+----------------+
                 |
                 v
+---------------------------------+
|       OPTIONAL PLUGINS          |
| Tools · Views · Domains         |
+---------------------------------+
```

Status vocabulary used throughout this document:

- **CURRENT** — implemented and wired in the repository today (the
  TypeScript behavioral reference, the Rust candidate foundation, and
  their verified surfaces).
- **TARGET** — committed product direction for a future stage/milestone
  (Profiles, Context controls, Skills, Plugins, Tools, Views, Domains,
  Runs, /evolve). Target items are documented as direction; none are
  described as existing code until implemented.
- **FUTURE / NOT DUE** — deliberately not committed (general Hooks,
  built-in multi-agent frameworks, TaskGraph, generic workflow engines,
  agent teams/Fleet, distributed workers, plugin marketplaces, plugin
  dependency graphs, automatic Skill/Plugin acquisition, model-router
  architecture, generic Memory subsystem, GUI/TUI runtime ownership).
  These may be reconsidered only from concrete demand and evidence.

The Host is deliberately not a plugin and cannot be replaced by
configuration. Orchestration is not a foundational ownership layer; higher-
level schedulers may consume Siralos Runs without defining Run semantics. The
permanent lean constitution is recorded in ADR 0036.

```text
@Siralos CLI (apps/cli)
    │  input parsing, rendering, process lifecycle
    │
    ├──→ @siralos/core  (provider port + application)
    │
    └──→ composition root (bootstrap/)
             │
             └──→ @siralos/adapters (concrete provider)
                       │
                       └──→ @siralos/core ports
```

## Core (`packages/core`)

Core owns Siralos application behaviour and its external contracts:

- Conversation model (`ConversationItem` union)
- Provider request/event contracts and the `ModelProvider` port
- Tool contracts and the immutable tool registry
- Application events and the bounded provider/tool loop
- The security model: `Capability`/`CapabilityPolicy`, built-in `SandboxProfile`s (`inspect`, `develop-offline`, plus the internal `validation-offline` used only for commands and `godot-probe-offline` used only for engine probes), the pure `evaluatePermission` function, the `SandboxBackend` port, classified `SandboxError` codes, and the `SiralosSecurity` facade (`evaluateCapability`, `checkSandbox`)
- The provider-neutral development-command contracts: `CommandRunner` preparation/execution contracts, the immutable `CommandRunnerRegistry`, `COMMAND_LIMITS`, the canonical command digest, the opaque single-use `PreparedCommand`, the `PreparedCommandTool` contract for `process.run`, and the `CommandApplicationEvent`/audit model

Core imports no Node infrastructure modules, no adapters, no CLI code, no terminal libraries, and no OS sandbox runtime. It never inspects the parent environment and never spawns processes. Architecture checks enforce all of this.

## Application layer

`createSiralosApplication({ provider, tools, maxToolRounds? })` returns the application: `sendPrompt(text, signal?)` streams `ApplicationEvent`s and `getStatus()` reports provider, state, and item count. State is private; only immutable views are exposed. The application owns conversation history, which providers must not.

The security facade (`createSiralosSecurity({ backend, policy, profile })`) sits beside the application in the composition root: `evaluateCapability` applies the pure permission evaluator, and `checkSandbox()` streams `sandbox_check_started` / `sandbox_check_completed` events from the backend's `inspect()`. The facade is consumed today by CLI diagnostics; future process and write tools will be gated through it before any backend call.

## Security model

- **Capability policy** (`CapabilityPolicy`) maps `workspace.read`, `workspace.write`, `process.execute`, and `network.outbound` to `allow` / `ask` / `deny`. Missing rules fail closed; explicit denies win; a profile can never broaden a denied policy; no built-in profile enables network.
- **Profiles**: `inspect` (read-only, no processes, no network — the default), `develop-offline` (workspace writes and process execution both require one-time approval, network denied, protected metadata paths, minimal environment, timeouts, output limits), `validation-offline` (internal: command execution is bound to a read-only workspace regardless of the user profile — no command can execute at this stage because both runners fail closed as unavailable), and `godot-probe-offline` (internal: the effective profile for engine probes — workspace excluded from readable roots, never writable — though engine probing is intentionally unavailable at this stage).
- **Backend port** (`SandboxBackend`): `inspect()` reports truthful per-platform status and capabilities; `execute(request)` runs a trusted `SandboxedProcessRequest` (executable + arguments, never a raw shell string; optional bounded output streaming, explicit timeout, and per-stream hard limits) and returns a bounded `SandboxedProcessResult` with violations; `close()` resets backend state and is idempotent. Errors normalize into `SandboxError` codes.
- **User configuration**: `~/.siralos/config.json` selects the profile and backend (defaults: `inspect`, `auto`). Unknown profiles/backends fail validation; project repositories cannot broaden these settings; `validation-offline` is not selectable.
- **Conformance**: `npm run test:sandbox` runs fixed internal probes (workspace read/write, outside-write denial, secret denial, network denial, descendant confinement, output limits, timeout, cancellation) plus validation-command probes (read-only workspace enforcement for root/child/grandchild/npm scripts, network and loopback denial, credential and `NODE_OPTIONS` absence, disabled npm pre/post hooks, closed stdin, output-limit termination, descendant termination on timeout and cancellation, no workspace artifacts, run-directory cleanup) against the real backend using temporary directories and fake secrets. Unavailable backends are reported loudly and never treated as secure. At this stage private run-directory creation and cleanup fail closed, so the suite cannot construct verified per-run directories and every probe reports skipped loudly with that reason — skipped is never treated as passed.

## Command execution

`process.run` is a `PreparedCommandTool` owned by adapters, built on core contracts and ports. No command can execute at this stage: both runners report `unavailable` during preparation, before any approval; the flow below describes the designed execution path.

```text
Provider requests a development command
   ↓
Runner validates structured input (both node-script and npm-script
fail closed as unavailable)
   ↓
Runner prepares an immutable plan: working directory, script file
(bounded, no symlinks, SHA-256 recorded), trusted Node identity,
argument array, timeout
   ↓
Application requests one-time approval with the exact preview and digest
   ↓
CLI reviewer shows every boundary; approval binds to the digest
   ↓
Tool revalidates all preconditions, recomputes the digest, checks the
sandbox is available and fully enforcing
   ↓
Private run-directory creation fails closed as unavailable (no
directory-relative primitive); the tool reports unavailable before any
execution, approval, or lock acquisition
   ↓
[Designed path, not offered:] creates a sandbox-private run directory
(no-follow verified, exclusive creation, canonical re-verification),
acquires the shared mutation lock, records Git status
   ↓
Runner stages the exact approved script bytes into the run's private
script cache and verifies the copy's hash; the child executes the
immutable private copy
   ↓
Sandbox backend executes under validation-offline with the request's own
per-execution configuration (host-read allowlist, read-only workspace,
denied network, closed stdin, minimal environment, bounded output)
   ↓
CLI streams sanitized bounded output; result maps to a structured
command result (nonzero exit is a completed command)
   ↓
Git status compared; run directory removed (cleanup outcome observed);
lock released
```

- Runners are prepared/executed through the immutable runner registry; the concrete plans are opaque single-use objects that only their creating runner can translate back into an executable request (revalidated and rehashed). The digest covers runner id, executable identity and version, script path and complete SHA-256, arguments, working directory, profile, environment policy, timeout, output limits, stdin and network policy.
- Command execution never spawns a process from core, the CLI, providers, the runners, or the Git adapter; only the sandbox adapter uses process APIs (Git always executes through the sandbox backend), and the architecture check rejects `shell: true`/`exec`/`execSync`/`spawnSync` in runtime code.
- Private run directories are **unavailable at this stage**: the design (each run lives under `~/.siralos/runs/<workspace-fingerprint>/<run-id>/` with private `home/`, `tmp/`, `npm-cache/`, and a script cache; every path component verified with no-follow semantics before anything is created beneath it, exclusive creation, the runs root outside the workspace, and the sandbox granted exactly the current run directory) is not offered because Node offers no directory-relative (openat/mkdirat-style) or delete-by-handle primitive. The provider performs zero filesystem operations: creation reports `unavailable` before creating anything, and cleanup reports a truthful failure while preserving anything that exists; cleanup failures are always observed and reported.
- Git structured status before/after execution is a verification signal: a workspace change detected despite read-only enforcement marks `workspace_violation` and disables further commands for the session.
- See `SECURITY.md` and ADR 0007 for the full model.

See `SECURITY.md` for the threat model and platform-specific behaviour.

## Tool loop

`sendPrompt` runs a bounded provider/tool loop:

```text
User prompt
   ↓
Provider turn
   ├── final text → complete
   └── tool calls
          ↓
      Execute tools sequentially
          ↓
      Add tool results to history
          ↓
      Next provider turn (until a turn completes without tool calls)
```

- The provider receives only the tools the capability policy permits: under `inspect`, write tools are absent from the request.
- Assistant text, tool calls (`assistant_tool_call`), and tool results (`tool_result`) are stored as distinct `ConversationItem`s in chronological order. File contents stay classified as tool data.
- Tool activity surfaces as `tool_started`, `tool_completed`, `tool_failed`, and `tool_cancelled` application events with bounded display summaries, plus `tool_awaiting_approval`, `approval_requested`, and `approval_resolved` events for the approval flow.
- Unknown tools and duplicate call ids produce failed tool results without executing anything; the provider gets a subsequent turn to respond.
- A configurable maximum tool-round count (default 8, `DEFAULT_MAX_TOOL_ROUNDS`) stops the loop with a clear failure instead of an infinite loop.
- Cancellation flows from the application signal into the provider stream and each tool execution; later tool calls never start after cancellation, and no false completion is stored.

## Approved mutations

> **Status note (fail-closed).** The mutation flow below is **not offered at this stage**: every entry point of `workspace.create_file`, `workspace.edit_file`, `workspace.delete_file`, and `/undo` fails closed as `unavailable` before any write, approval, or checkpoint, because Node offers no directory-relative (openat/renameat) primitive and a same-user process can swap a parent or target at any instruction boundary. The tools are unavailable entry points; the retained layer is the core contracts plus the reusable tested primitives and the filesystem checkpoint store, while the former preview/approval/application logic below was largely deleted and is documented as future work; no approval for mutations is ever requested, and no new checkpoint is ever created at this stage (historical checkpoint data from earlier sessions, if any, may still be listed).

Write tools (`workspace.create_file`, `workspace.edit_file`, `workspace.delete_file`) are `PreparedMutationTool`s registered alongside the immediate read-only tools:

1. The provider requests a write tool; the application evaluates the capability policy (`workspace.write` is `ask` under `develop-offline`, `deny` under `inspect`).
2. The tool `prepare`s the change in memory: validates input, resolves the target with component-aware path safety, checks protected paths, reads and hashes the current file, applies exact text replacements (for edits), and builds a complete deterministic bounded unified diff.
3. Under `ask`, the application calls the core-owned `ApprovalReviewer` with the bounded preview. The CLI implements the interactive reviewer; denial is the default; EOF, reviewer failure, timeout, and cancellation all prevent mutation; decisions are one-time.
4. On approval, the tool `apply`s exactly once: re-acquires the serialized mutation lock, revalidates the path, file type, symlink state, and hash, stages content in an exclusive temp file (updates), enters a short non-cancellable commit section, replaces or deletes the target, verifies the final bytes and hash, and cleans up temp artifacts.
5. Conflicts (stale hash, disappeared or appeared target, symlink races, replaced parents) return `conflict` without touching the changed external state; a revised proposal requires a new approval.

Prepared mutations are opaque single-use objects; core never sees their contents and the tools never accept another tool's mutation. See `SECURITY.md` and ADR 0005 for the full model.

Prepared command tools follow the same pattern: `prepare` (validate + build the immutable plan), approval (the exact preview and digest), then `executePrepared` exactly once with the approved digest; the tool's payload map makes reuse impossible and the runner's revalidation makes changed plans conflict. See "Command execution" above.

## Ports

External capabilities the application needs are narrow interfaces owned by core: the `ModelProvider` port (`stream(request): AsyncIterable<ModelEvent>` with an optional `AbortSignal`), the `SandboxBackend` port, the `ApprovalReviewer` port, the `GitInspector` port, the `CheckpointStore` port, the `UndoService` port, the `CommandDigestService` port (hashing is injected so core stays free of Node imports), the `GodotProbeRunner` port (fixed project-independent engine probes executed through a backend), and the `GodotInspector` port (engine and project inspection results). The command tool consumes the sandbox, approval, git, lock, runner-registry, and run-directory ports; runners are core contracts implemented in adapters.

## Adapters (`packages/adapters`)

Adapters implement core-owned ports. Providers, concrete tools, configuration, environment building, and the sandbox backend live here:

- `DeterministicFakeProvider` (id `deterministic-fake`): streams text responses in chunks, supports cancellation, and has synthetic tool scenarios (`list files`, `read README.md`, `search <text>`) that request registered tools and respond truthfully to their results. It never touches the filesystem or executes tools.
- Read-only workspace tools: `workspace.list`, `workspace.read`, `workspace.search`. All three share one canonical containment implementation (`resolveWorkspacePath`), the explicit exclusion list (`node_modules`, `.git`, `dist`, `coverage`), and the `WORKSPACE_LIMITS` output limits. `workspace.read` returns the complete-file SHA-256.
- Approved mutation tools: `workspace.create_file`, `workspace.edit_file`, `workspace.delete_file` — **all fail closed as `unavailable` before any write, approval, or checkpoint** (Node offers no directory-relative primitive; the tools are unavailable entry points). Their former preview/approval/application logic was largely deleted; what remains is the core contracts (prepared-command/digest/approval ports) and reusable tested primitives: write-path safety and protected-path enforcement (`mutation-paths.ts`, still used by the Git adapter for diff-path validation), the serialized mutation lock, exclusive temp-file staging (`mutation-temp.ts`), hash-based conflict detection (`mutation-hash.ts`), deterministic bounded diffs (`diff.ts`), and the identity-bound safe-replacement helpers (`safe-replacement.ts`) — the identity-bound commit design is documented as future work, not offered.
- User configuration (`loadUserConfig`): reads `~/.siralos/config.json`, defaults to `inspect`/`auto`, and rejects unknown profiles or backends.
- Child environments (`buildChildEnvironment`, `buildCommandEnvironment`): allowlist-based construction with denied credential patterns and fixed safe command values; the only sanctioned way to build a child environment.
- Command layer (`src/process`): trusted npm CLI resolution (used by the conformance suite), the `node-script` and `npm-script` runners (both fail closed as unavailable: `isAvailable()` returns false for both, because the pinned Node runtime cannot bind execution to the approved script bytes — the script can reach internal surfaces such as `process.binding` (e.g. `spawn_sync`) to spawn an unconstrained interpreter, and the staged private copy can be substituted by a same-user process in the verify-to-launch window), the private run-directory provider (creation and cleanup both fail closed as `unavailable`: Node offers no directory-relative (openat/mkdirat-style) or delete-by-handle primitive, so a same-user process could substitute a verified parent between identity verification and a pathname-based create, and cleanup could delete a substituted object — the provider performs zero filesystem operations and nothing is ever created or deleted), and the `process.run` tool that would execute approved plans through the `SandboxBackend` under `validation-offline`, granting the sandbox exactly the current run directory when identity-bound run directories exist. No module in this layer spawns processes.
- `AnthropicSandboxRuntimeBackend`: the first concrete `SandboxBackend`, wrapping `@anthropic-ai/sandbox-runtime@0.0.70` (pinned exactly). Only this module may import the runtime package. It enforces a deny-by-default host-read allowlist (deny `/` and re-allow the current run directory, the workspace when the profile allows it, the trusted runner executables, and the minimum system runtime paths on Linux/macOS; never reported generally available on Windows), gives every request its own per-execution filesystem/network configuration for its exact profile and run directory so no request can inherit a broader earlier profile and no request sees a shared sandbox home/temp or another run (requests with an explicit read-roots list receive exactly those identity-bound paths), serializes the complete global sandbox lifecycle (config selection, reset, initialize, wrap, spawn, execute, violations, cleanup) one request at a time with `close()` draining the queue, resets and reinitializes the shared manager when the effective configuration changes, streams bounded decoded output accounted on raw bytes with the hard limit enforced inside the crossing chunk, terminates the process tree on timeout/cancellation/output-limit, runs cleanup in `finally` on every path, and isolates failing output callbacks.
- Conformance runner (`runSandboxConformance`): writes fixed fixture programs into a temporary workspace and executes them through the backend, reporting pass/fail per probe. Host-read probes use existing regular files in representative unapproved locations selected independently of the deny surface, plus cross-run isolation and bidirectional profile-isolation probes.
- Godot adapters (`src/godot`): discovery (configured user installations — absolute paths only, edition hints — plus fixed-name PATH search with safe PATHEXT handling and macOS `.app` bundle resolution), the fail-closed probe runner (reports `unavailable` for every probe and never spawns the executable — the backend re-opens the staged copy's pathname at spawn time and a same-user process can substitute bytes between final verification and launch; no exec-by-handle primitive — so no engine profile can be produced), the engine-profile cache as an explicitly unavailable no-op component (never initialized, created, read, or written: `load()` is always a miss, `store()` returns a typed unavailable result, `count()` is 0, and the doctor reports it disabled — the earlier storage implementation was removed rather than retained as an unsafe surface), executable fingerprinting/version parsing/edition classification/selection ranking (designed to consume probe results), static project detection and profiling (rescans the complete bounded project on every inspection — no profile cache), and the `godot.inspect_engine` / `godot.inspect_project` tools. Only this adapter could ever invoke the engine, always through the sandbox backend; at this stage nothing spawns Godot, and providers never run Godot.

The workspace root is the canonicalized directory Siralos was launched from; it is stored privately by the tools and displayed in `/status`. Provider adapters never import sandbox, environment, tool, checkpoint, git, or process modules — the architecture check enforces that boundary.

## CLI (`apps/cli`)

The CLI is an input/output adapter:

- Reads terminal input and renders terminal output through a small `SessionIO` interface
- Parses every command in `COMMAND_CATALOG` (the `SlashCommand` union derives from it, and the session switch is exhaustiveness-checked against it) in a pure module separate from rendering
- Renders application events incrementally
- Handles process startup, EOF, `Ctrl+C`, and shutdown
- Exposes the `siralos` binary and the composition root

The CLI does not own conversation state, provider behaviour, or application policy. It never imports a concrete provider outside the composition root.

## Composition root

`apps/cli/src/bootstrap/create-application.ts` is the only module that imports both core and concrete adapters:

```ts
export async function createCliApplication(): Promise<CliApplication> {
  const config = await loadUserConfig(getDefaultUserConfigPath());
  const profile = getBuiltInProfile(config.sandbox.profile);
  const policy = createDefaultPolicy(config.sandbox.profile);
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const sandbox = createAnthropicSandboxRuntimeBackend({ workspaceRoot, ... });
  const security = createSiralosSecurity({ backend: sandbox, policy, profile });
  const workspaceTools = [createWorkspaceListTool(workspaceRoot), /* ... */];
  const application = createSiralosApplication({
    provider: createDeterministicFakeProvider(),
    tools: createToolRegistry(workspaceTools),
  });
  // ...
}
```

No dependency-injection container, service locator, or reflection. The registry rejects duplicate tool names at startup and never mutates afterwards. `apps/cli/src/bootstrap/sandbox-doctor.ts` hosts the `--sandbox-doctor` diagnostic (also the only place that runs probes from the CLI).

## Current dependency direction

```text
CLI ───────────────→ Core
 │
 └── composition ─→ Adapters ─→ Core ports
```

- Core imports nothing from the workspace and no OS sandbox runtime.
- Adapters import only core contracts; adapter providers never import adapter tools, sandbox, environment, checkpoint, git, or process modules; sandbox adapters never import providers.
- The CLI imports core anywhere, and concrete adapters only in the composition root (tests may import adapters directly).
- `process.env` is never inspected in package source; child environments are built from an explicit allowlist, and the sandbox wrapper's runtime-required environment is merged under strict rules (wrapper-only variables added, Siralos-controlled values win collisions, denied patterns fail closed, Windows keys normalized case-insensitively).
- Direct `node:child_process` usage is limited to the sandbox backend module, the conformance runner, and test files (the `node:` prefix and the bare `child_process` spelling normalize to one rule); command runners and the Git adapter never spawn processes — Git executes through the sandbox backend.
- Raw process execution (`shell: true`, `exec(`, `execSync(`, `spawnSync(`) is prohibited in runtime code, with documented exemptions only for test fixtures and the embedded conformance probe sources; the checks are a developer guardrail using structural TypeScript parsing (imports, re-exports, static dynamic imports, aliases, and call sites) plus regex/text checks for constructs parsing cannot represent.
- Destructive filesystem APIs (`writeFile`, `unlink`, `rename`, `appendFile`, `createWriteStream`, `rm`, `rmSync`, and friends, including aliased and namespace forms) are limited to the workspace mutation modules, the conformance runner, the process adapter (run directories), and tests. Path-based recursive deletion (`rm`/`rmSync` with `recursive: true`) is prohibited in all production code even inside approved mutation directories: the rule resolves import bindings structurally, so direct, aliased, and namespace imports of `fs`, `node:fs`, `fs/promises`, and `node:fs/promises` are all caught, and the only exemptions are the exact host-side conformance runner file and test-support files — never a whole directory: Node offers no directory-handle-relative deletion primitive, so recursive removal cannot be identity-bound and is never offered. Non-recursive `rm` stays governed by the destructive-API location rule.
- Git mutation commands are rejected in runtime code both as string tokens and structurally in spawn argument lists.
- `npm run check:architecture` (see `scripts/check-architecture.mjs`) enforces these rules mechanically: prohibited imports, prohibited package dependencies, provider/sandbox isolation, process, environment, and write boundaries, and workspace dependency cycles all fail the check. It is a developer guardrail, not an OS security boundary; the checks use structural TypeScript parsing plus regex/text checks, and runtime-constructed module specifiers and string contents are documented limitations.

## Rust candidate workspace (Stage 3R, ADR 0032)

The Rust candidate implementation follows the same inward pattern with
its own dependency direction, enforced by `npm run check:rust`:

```text
siralos-cli ─────────→ siralos-adapters ─→ siralos-core
```

- `siralos-core` owns domain-neutral host semantics and types. It must
  not depend on adapters, on infrastructure, or on any optional domain
  (Godot), and it compiles with the Godot domain completely absent.
  Domain neutrality is enforced by a forbidden-symbol scan over core
  sources plus Cargo.toml dependency rules (`scripts/check-rust-architecture.mjs`).
  The `workspace` module owns the generic R4 foundation: validated
  workspace-relative paths and protected-path classification, the
  reference operation bounds, deterministic revision handles and the
  bounded session registry, prepared-effect models, checkpoint
  contracts/invariants/undo-planning/reconciliation classification, and
  the read-only Git error/disposition contract — no filesystem or
  process semantics. The `language` module owns the generic R5
  language-intelligence foundation: one-based positions/ranges with
  typed validation (0-based LSP conversion happens at the adapter
  boundary, never silently), the bounded sanitized diagnostic model
  with closed severities, deterministic dedup/ordering, and explicit
  truncation, generic symbol/definition/reference query models with
  deterministic ordering and bounds, the language-neutral
  structural-document representation with the deterministic
  byte-bounded advisory summary formatter, typed validation result
  semantics (source-invalid never conflated with infrastructure
  failure), reference-extracted generic limits, and R4 revision
  binding — no GDScript grammar, LSP transport, process execution, or
  domain-registry concepts (enforced by the architecture check). The
  `domain` module owns the generic R6 lifecycle/capability foundation:
  validated package identity (stable id, exact SHA-256 digest, versioned
  ABI), the explicit absent/installed/enabled/active lifecycle state
  machine with typed transitions, declared capability requests and the
  Host-authoritative grant decision (enablement never implies
  authority), exact activation binding, typed recovery-ready failure
  outcomes with stable codes, and the explicit absence of implicit
  acquisition (workspace contents are opaque to the lifecycle) — no
  component runtime, filesystem, or process semantics.
- `siralos-adapters` owns infrastructure/adapters; it may depend only on
  core. Its `workspace` module implements the R4 filesystem surface:
  canonical root resolution, containment-safe path resolution,
  bounded exact reads, deterministic bounded listing and search, the
  fail-closed mutation-preparation boundary, checkpoint storage
  inspection and reconciliation, and the typed unavailable Git
  inspection disposition (Git is never spawned without an enforcing
  process boundary). Its `language` module owns the generic R5
  language-service boundary: URI mapping from service URIs to
  workspace-relative paths (out-of-workspace URIs rejected, decoded
  escapes refused, native separators normalized), with no process
  execution and no LSP transport. Its `domain` module owns the R6
  executable Domain boundary (ADR 0034): the versioned
  `siralos:domain-abi@1.0.0` WIT world, component loading and
  instantiation with the versioned export-identity check
  (unknown/incompatible ABI versions fail closed), exact-byte package
  digest verification at install and activation (the host computes the
  digest itself; stale or wrong bytes fail before any semantic work),
  fuel/memory/input/output/host-call bounds, guest-call supervision with
  typed trap outcomes and session stop, and the host-mediated effect
  boundary (grant-checked bounded workspace reads; process execution
  denied) — the component receives no ambient filesystem/network/process
  authority.
- `siralos-cli` is the composition boundary and the `siralos` binary; it
  may depend only on core and adapters.
- Exactly three crates exist; no placeholder or hypothetical domain
  crates, no marketplace/plugin infrastructure.
- `unsafe` Rust is forbidden by workspace lint
  (`unsafe_code = "forbid"`) and by the architecture check; edition 2024,
  a pinned toolchain, and rustfmt/clippy gates are enforced.
- The authoritative engineering rules live in
  `docs/development/RUST_STYLE.md`.

The TypeScript implementation remains the behavioral reference
(migration oracle) until later Stage 3R milestones retire it; behavior
is preserved across migration, structure is deliberately redesigned
(ADR 0032).

## Differential behavioral harness (Stage 3R R2, ADR 0033)

The migration's audit remediation gate: a deterministic, offline harness
that runs a versioned, digest-bound scenario corpus against both
implementations and machine-compares canonical outcome records.

- Corpus: `tests/differential/corpus/` — typed scenario inputs only
  (never expected outputs), each with a stable id and SHA-256 digest.
- Oracle runner (TypeScript) and candidate runner (Rust, `siralos-harness`
  binary in `siralos-cli`) emit identical canonical record formats;
  environment-sensitive scenarios execute in probe subprocesses with
  scrubbed environments.
- Comparator: exact canonical match; `required` scenarios gate (exit 1
  on deviation), `informational` scenarios are recorded only. Every run
  emits `audit.json` — coverage, per-scenario status, and the deviation
  inventory that drives remediation.
- Wired as `npm run check:differential` into `npm run check` and the
  GitHub Actions Rust workflow.

The first subjects are the state-dir resolution (TS `os.homedir()` path
versus `siralos-adapters::paths::state_dir`) and the product version
identity (package.json versus Cargo.toml). Every later ported subsystem
adds scenarios; a port is accepted only when its required scenarios
compare clean.

## Why a modular monolith

One process with explicit module boundaries is the smallest structure that keeps UI, application logic, and infrastructure separable without introducing distributed orchestration, message buses, or deployment complexity. Siralos can grow its later stages inside this boundary and can extract packages later if a real need appears.

## Why the fake provider is an adapter

Provider neutrality is a stated product requirement, so the provider contract belongs in core and concrete implementations live behind it. The fake provider exercises the port end to end without credentials or network access, which keeps development and tests self-contained.

## Why UI code does not own application behaviour

The application must remain usable without a terminal (headless tests, future Godot-facing surfaces). Conversation policy and state transitions live in core; the terminal only translates between user intent and application events.

## Godot engine discovery and profiling

Godot discovery and profiling follow the same inward pattern as the other capabilities: neutral contracts in core, a single implementation in adapters, thin commands in the CLI. The CLI never runs Godot itself; only the Godot probe adapter in `@siralos/adapters` invokes the engine, always through the sandbox backend. Providers never run Godot. Core remains Node-free and process-independent.

```text
packages/core/src/godot/                 Godot models, classification, selection
                                         policy, compatibility, inspector ports,
                                         probe and selection events
packages/adapters/src/godot/
  discovery/                             configured installations + fixed-name PATH
                                         search, .app bundle resolution
  process/                               probe invocation through the SandboxBackend
  api-dump/                              --dump-extension-api execution, metadata
                                         extraction, dump cleanup
  profile/                               fingerprints, version parsing, edition
                                         classification, selection ranking
  cache/                                 engine-profile cache (explicitly
                                         unavailable no-op; never initialized,
                                         created, read, or written)
  project/                               static project detection and profiling,
                                         language profile, content inventory
  tools/                                 godot.inspect_engine, godot.inspect_project
apps/cli/                                Godot commands (/godot, /godot-installations,
                                         /godot-project, /godot-doctor), godot doctor,
                                         --godot-path / --godot-installation startup
                                         flags, SIRALOS_GODOT /
                                         SIRALOS_GODOT_INSTALLATION overrides
```

- **Core owns**: the Godot models (engine profile, version and release channel, edition, capability sets, support classification), classification rules, the deterministic selection policy with recorded rationale, compatibility assessment, the `GodotProbeRunner` and `GodotInspector` ports, and probe/selection events. Core never discovers, invokes, parses, or stores anything itself.
- **Adapters own**: discovery (configured installations and fixed-name PATH search), the fail-closed probe runner and its designed invocation of the exact probe argument arrays, output parsing, project scanning, inventory, and cache storage. The probe adapter is the only code that could ever spawn Godot; at this stage probing fails closed, so nothing spawns Godot.
- **CLI owns**: command parsing, rendering, and composition — `/godot-installations` displays the recorded selection rationale, `/godot-project` renders static project findings, `/godot-doctor` reports discovery/selection/cache diagnostics, and the startup flags and environment overrides seed selection at the highest precedence.

**Probe argument discipline.** Probe invocation uses exactly three fixed tuples constructed by the single `fixedProbeArguments` constructor private to the Godot probe adapter: `--version`, `--help`, and `--dump-extension-api`. Project-affecting arguments (`--path`, `--upwards`, `--import`, `--scene`, `--script`) and `--editor` are never passed; there is no project path and no project working directory. The architecture check mirrors the runtime boundary in probe invocation code: non-fixed `--` tokens, concatenated construction, imported argument arrays, and construction outside the fixed runner are rejected (a developer guardrail using structural parsing plus regex/text checks; the runtime boundary is the private constructor). At this stage engine probing fails closed as `unavailable`, so no probe is ever invoked and nothing spawns Godot.

## Godot API knowledge and GDScript diagnostics

The knowledge and diagnostic layers follow the same inward pattern: neutral contracts in core, adapters own every process/filesystem/parse concern, the CLI renders. Providers and the CLI never spawn Godot; only the fixed runners in `@siralos/adapters` could, and at this stage they fail closed (ADR 0010).

```text
packages/core/src/godot/
  knowledge.ts                            knowledge-profile model, cache
                                          validation policy, manual-channel
                                          classification, knowledge port
  api.ts                                  API symbol model, deterministic
                                          symbol identities, query and
                                          result models
  gdscript.ts                             GDScript diagnostic model, severity,
                                          aggregation policy, check results,
                                          diagnostics port, prepared-check
                                          digest contract
packages/adapters/src/godot/
  knowledge/                              with-docs dump parser, bounded index
                                          builder, search/lookup, no-op cache,
                                          knowledge service
  diagnostics/                            script enumeration and validation,
                                          check preparation, output
                                          normalization, diagnostics service,
                                          prepared-check store
  process/godot-knowledge-runner.ts       fixed --dump-extension-api-with-docs
                                          runner (fail-closed)
  process/godot-check-only-runner.ts      fixed --headless --path <mirror>
                                          --script <mirror-script> --check-only
                                          runner (fail-closed)
  tools/                                  godot.api_search, godot.api_lookup,
                                          godot.check_script,
                                          godot.check_project_scripts
apps/cli/                                 /godot-knowledge, /godot-api,
                                          /godot-knowledge-refresh,
                                          /gdscript-check, /gdscript-diagnostics
```

- **Core owns**: the knowledge-profile model, API symbol model, query/search/lookup result models, deterministic symbol identities, the GDScript diagnostic model, severity and aggregation policy, the knowledge and diagnostics ports, and the provider-neutral prepared-diagnostic tool contract. Core never spawns Godot and never reads API cache files.
- **Adapters own**: the fixed runners, dump parsing, index building/storage/querying, script enumeration and hashing, output normalization, and both services (fail-closed orchestration).
- **CLI owns**: command parsing, rendering, and composition.
- **Check-only argument discipline (architecture-enforced)**: `--script` is legitimate only inside the check-only runner structurally paired with `--check-only`, `--headless`, and mirror-only `--path`; scene/editor/import/LSP/DAP/recovery options never appear; the source workspace never becomes the diagnostic `--path`; no literal or concatenated argument construction; no imported argument arrays. The knowledge runner passes exactly `--dump-extension-api-with-docs`.
- **Approval protocol**: `godot.check_script`/`godot.check_project_scripts` are `prepared_diagnostic` tools sharing the one-time approval flow with the project probe; `godot.diagnose` is `ask` in every user-facing profile (no public `allow`), while `godot.api_search`/`godot.api_lookup` are `allow`. While execution is unavailable, preparation returns typed `unavailable` results before any approval is requested.

## GDScript language session (LSP)

The language-session layer follows the same inward pattern: neutral contracts in core, adapters own sockets/framing/normalization/process concerns, the CLI renders. Core knows no TCP, no port numbers, no mirror paths, no JSON-RPC framing (ADR 0011).

```text
packages/core/src/godot/
  lsp.ts                                  session port, models, limits,
                                          preview + prepared-session digest,
                                          language events
packages/adapters/src/godot/
  lsp/frame-parser.ts                     incremental Content-Length framing
  lsp/json-rpc.ts                         bounded JSON-RPC client
  lsp/file-uri.ts                         mirror URI <-> relative path mapping
  lsp/normalizers.ts                      diagnostics/hover/completion/
                                          definition normalization
  lsp/port-allocator.ts                   loopback-only dynamic port allocation
  lsp/godot-lsp-service.ts                session prepare/start/status/staleness
  process/godot-lsp-runner.ts             fixed recovery LSP tuple (fail-closed)
  tools/                                  godot.lsp_session + 4 query tools
apps/cli/                                 /gdscript-lsp, -stop, -hover,
                                          -complete, -definition; /status
```

- **Core owns**: the session port, status/request/result models, session preview and digest contract, capability model, and events. Core never opens sockets and never spawns Godot.
- **Adapters own**: framing, JSON-RPC correlation, URI mapping, normalization, port allocation, the fixed runner (fail-closed), and session orchestration. Only `src/godot/lsp` and `src/sandbox` may import `node:net` (architecture-enforced).
- **CLI owns**: command parsing, rendering, and composition.
- **Approval protocol**: `godot.lsp_session` is a `prepared_lsp_session` tool sharing the one-time approval flow; `godot.lsp` is `ask` in every user-facing profile (no public `allow`); query tools require an active session. While startup is unavailable, preparation returns typed `unavailable` results before any approval is requested.
- **LSP runner discipline (architecture-enforced)**: `--lsp-port` is legitimate only inside the LSP runner structurally paired with `--headless --editor --recovery-mode --path <mirror>`; DAP/debug/scene/import/quit options never appear; path and port values come from Siralos-owned inputs; `workspace/applyEdit`/`workspace/executeCommand` are never implemented in runtime adapter code.

## GDScript development workflow

The development-workflow layer follows the same inward pattern: contracts in core, orchestration and change-set machinery in adapters, rendering and entry points in the CLI. The workflow composes existing primitives (read-only inspection, API knowledge, LSP intelligence, prepared-mutation approval, checkpoints, `--check-only` diagnostics, Git inspection) and never bypasses them (ADR 0012).

```text
packages/core/src/godot/development/
  development-model.ts                    phases, statuses, session, evidence,
                                          result, immutable limits, events,
                                          workflow-start preview + digest
  development-change-set.ts               exact text change-set contract,
                                          limits, canonical digest
  development-change-set-apply.ts         checkpoint-then-apply protocol,
                                          partial-failure recovery outcomes,
                                          file-primitives seam
  development-service.ts                  GDScriptDevelopmentService port
packages/adapters/src/godot/development/
  change-set-preparation.ts               read-only preparation: paths,
                                          protected paths, hashes, resulting
                                          content, diffs, after-hashes, digest
  change-set-executor.ts                  apply protocol with fail-closed
                                          platform gate + in-memory-primitives
                                          tested recovery
  gdscript-development-service.ts         workflow orchestration (LSP
                                          suspension, checkpointing, parser
                                          gate, fresh LSP, settling, evidence,
                                          repair budget)
  tools/                                  workspace.apply_text_changeset,
                                          godot.development_status
apps/cli/                                 /develop, /development-status,
                                          workflow-aware /cancel
```

- **Core owns**: the vocabulary (phases, terminal statuses, validation normalization), the session/evidence/result models, immutable limits, the change-set contract and digest, the apply protocol with recovery outcomes, and the service port. Core remains Node-free.
- **Adapters own**: read-only change-set preparation (filesystem reads only), the change-set executor whose platform gate fails closed as unavailable (the checkpoint/apply/recovery protocol is tested internal code exercised through injected in-memory file primitives), and the workflow orchestration. The orchestrator and the executor import no `node:fs`, `node:net`, `node:child_process`, or `node:path` (architecture-enforced); the workflow never spawns Godot and never opens sockets.
- **CLI owns**: `/develop` (workflow start through the one-time approval protocol, then the conversational provider loop), `/development-status`, and workflow-aware `/cancel`; `/gdscript-lsp` and `godot.lsp_session` defer to the workflow's session ownership while a workflow is active.
- **Approval protocol**: the workflow start is one one-time approval (`godot.development` capability, CLI-mediated) binding the request, project fingerprint, engine fingerprint, and limits, covering only the read-only validation context; every change set and repair still goes through the existing prepared-mutation one-time approval flow (`workspace.write` is `ask` under `develop-offline`). While the change-set applier is unavailable, preparation returns typed `unavailable` results before any approval is requested.
- **Architecture enforcement**: the workflow orchestrator and change-set executor may not import Node infrastructure modules; core stays Node-free; the LSP/parser/runner disciplines of ADRs 0010 and 0011 remain structurally enforced; no runtime/game or DAP invocation exists.

## Development quality gates and independent review

The quality layer (ADR 0013) follows the same inward pattern: contracts and the deterministic gate policy in core, orchestration and reviewer implementations in adapters, rendering and entry points in the CLI. Deterministic gates and model-based review are separate layers: gates are authoritative for measurable conditions; the reviewer is an additional reasoning signal that can never replace a gate.

```text
packages/core/src/godot/quality/
  quality-model.ts                     gate vocabulary, report, status mapping,
                                       immutable limits, quality events
  quality-warnings.ts                  warning baseline/delta with stable
                                       normalized identities
  quality-conventions.ts               read-only convention analyzer over
                                       changed lines (advisory by default)
  quality-validation.ts                validation-plan discovery policy,
                                       ValidationPlanDiscovery +
                                       QualityValidationExecutor ports
  quality-review.ts                    reviewer contracts, finding
                                       normalization, blocking policy,
                                       deterministic finding ids, chunking
packages/adapters/src/godot/quality/
  validation-plan-discovery.ts         bounded root package.json reading
  quality-validation-executor.ts       drives the existing process.run tool
                                       through one-time process approval
  quality-stage-runner.ts              quality-stage orchestration (gates,
                                       plan, review, report)
  provider-change-reviewer.ts          fresh-context provider reviewer with a
                                       strict JSON output contract
  fake-change-reviewer.ts              deterministic fake reviewer scenarios
  reviewer-tools.ts                    read-only reviewer tool registry
apps/cli/
  /quality, /review-change, quality rendering in /development-status
  bootstrap/review-provider.ts         review-provider resolution (default
                                       active provider; explicit configured
                                       profile fails clearly when missing)
```

- **Core owns**: the gate vocabulary and classification, the report model and deterministic status mapping, the warning-delta policy, the convention-analysis rules, the validation-plan selection policy, the reviewer contracts and finding normalization (bounds, safe paths, deduplication, blocking policy), and the immutable limits. Core stays Node-free.
- **Adapters own**: package.json discovery, the validation executor over the existing prepared-command approval flow (each project-defined command still requires its own exact one-time process approval and runs sandboxed with a read-only workspace, denied network, and closed stdin), the quality-stage orchestration, and the reviewer implementations. The quality/reviewer adapter must not import workspace-mutation, process, checkpoint, sandbox, or child-environment adapters (architecture-enforced): the reviewer is strictly read-only, cannot approve, execute, mutate, checkpoint, or alter sandbox rules or provider credentials; deterministic quality gates must not import reviewer implementations.
- **CLI owns**: `/quality` (current/final report), `/review-change` (fresh read-only review of the tracked change — no approval, no modification, no automatic repair), the quality sections of `/development-status` and `/status`, and review-provider resolution. Reviewer construction happens only in the composition root; the reviewer's read-only tool registry is composition-root owned.
- **Integration**: `/develop` automatically enters the quality stage after a cleanly validated change set; blocking review findings return the workflow to the provider for a focused, separately approved repair (at most 2 review-repair rounds) that is fully revalidated and re-reviewed. The workflow itself is fail-closed at this stage (change-set applier unavailable), so the quality stage, review, and validation commands never run in the shipped product; the opt-in `npm run test:godot-quality` conformance verifies that truthfully and always reports the live quality-stage isolation probe as skipped, never passed.

## Task runtime

The task runtime (Stage 3 milestone 1, ADR 0014) is the host-owned structured task foundation that later context projection, planning, persistence, and `/evolve` build upon (multi-agent orchestration is not core architecture; ADR 0036). It lives in core (`packages/core/src/tasks/`) and is provider-neutral, sandbox-neutral, and Node-free: it observes typed host facts and never imports provider or sandbox ports (architecture-enforced).

```text
packages/core/src/tasks/
  task-contract.ts      revisioned TaskContract (request, constraints,
                        acceptance criteria, pause policy), digest
  task-model.ts         TaskState, phases, steps, evidence records/references,
                        findings, validation/review status, progress,
                        WorkflowDisposition
  task-events.ts        typed append-only TaskActivityEvent union + allowlists
  task-snapshot.ts      immutable TaskRuntimeSnapshot captured at task start
  task-runtime.ts       host-owned TaskRuntime + TaskHandle (single mutation
                        path: closure-private state, snapshots on read)
  task-development.ts   development bridge: /develop request -> contract,
                        steps, snapshot, event mapping, host completion gate
  task-contract.test.ts unit tests for the contract model
tests/behavior/         deterministic behavior fixtures (behaviors 1-15)
```

- **Single-owner state rule**: every authoritative mutable Siralos domain has exactly one runtime owner. `TaskState` is owned exclusively by the `TaskRuntime` created in the CLI composition root; CLI, providers, adapters, and the UI receive immutable snapshots, projections, or events. The runtime's mutable state is closure-private; task contracts, runtime snapshots, step specifications, plan revisions, evidence sources, and returned snapshots are detached from caller-owned objects, and revisioned artifacts are deeply frozen. Duplicate task ids are rejected rather than replacing history, and terminal tasks refuse further authoritative mutation. Provider adapters cannot import the task runtime surface at all (architecture-enforced); the CLI is a read-only renderer of snapshots plus the completion-gate evaluation.
- **TaskContract** distinguishes what the user requested, constraints, individually addressable acceptance criteria (`deterministic` / `review` / `user`), and the pause policy. Contracts are bounded, deeply immutable, and revisioned: a material change produces revision N+1 with the same task id, never a mutation of revision N. Later milestones bind plan/mutation approvals and workflow continuation to contract revisions.
- **TaskState** is a materialized, serializable object: phase (`prepared | working | validating | reviewing | blocked | completed | cancelled | failed`), bounded steps with evidence references, acceptance states, evidence-backed findings, validation/review status, iteration, host-observed progress, and terminal timestamps. It never stores private chain-of-thought, provider continuation internals, secrets, or raw adapter output — evidence references point at already-owned artifacts (change-set ids, checkpoint ids, counts, digests) with a 4 KiB source bound.
- **Evidence-backed completion**: a step completes only through `completeStep(stepId, evidenceRefs)` with refs that exist, belong to the task, and match the step's declared evidence kinds (`research` steps accept read/lookup evidence; review steps accept reviewer results; no hard-coded "every step needs a mutation"). Evidence count/id/source bytes are bounded, and every declared evidence kind is runtime-bound to its corresponding source shape (for example, `review_result` cannot carry a `workspace_read` source). `WorkflowDisposition` is a structured _request_: a model-issued `complete` is a completion request that still passes the host completion gate (steps completed, criteria satisfied, validation/review clean, no unresolved critical/high findings). A model asserting "done" in text never reaches the runtime.
- **Progress**: the host feeds typed observations (`action` + canonical result fingerprint); identical repeated actions with identical results do not repeatedly count as progress. The bounded window surfaces `healthy / degraded / stalled` deterministically; the runtime never swaps models, spawns advisors, or hands off — those are future milestones.
- **TaskRuntimeSnapshot** is captured once at task start (runtime version, provider profile id, sandbox profile id, capability-policy fingerprint, workspace identity, Godot engine fingerprint, workflow identity + prepared-operation digest). Ordinary global configuration changes affect future tasks, never a running task's snapshot; a security revocation terminates/restricts existing work only where existing Siralos policy already requires it.
- **Activity log**: typed append-only `TaskActivityEvent` records (deterministic per-task sequence, host timestamps) for auditability/debugging/future persistence/UI projection — not event sourcing, not a competing state, and never a generic event bus. Events carry no provider-private continuation state (allowlisted field types).
- **/develop integration**: the CLI's `/develop` handler creates the task through the development bridge (`createDevelopmentTaskFlow`): request → revisioned `TaskContract` (user approval, applied mutation, workspace scope, parser, fresh-LSP, independent review criteria) → `TaskState` → the existing Stage 2 development workflow → the existing validation/review gates → the host completion gate. The Stage 2 quality gates remain authoritative; the task gate references the same deterministic results instead of duplicating them, and infrastructure failures stay `validation_incomplete` (never criterion failure, never success).
- **CLI**: `/task <request>` starts a host-owned ad-hoc task (completion honestly requires host verification — with no integrated workflow the gate refuses completion), `/task-status` renders task id, phase, contract revision, criteria status, active step, progress state, and whether completion is currently allowed; `/develop` prints the task status at start and after the workflow terminalizes, and `/cancel` finalizes the task as cancelled. The CLI feeds host-observed tool outcomes into progress.

The milestone does not implement planning, ContextProjector/ToolProjector/EvidenceProjector, knowledge retrieval, scenes/resources, multi-agent TaskGraph, persistent background jobs, an ACP/runtime server, plugins, or `/evolve`.

## Context, tool, and evidence projection

The projection layer (Stage 3 milestone 2, ADR 0015) is the application-owned
boundary between authoritative Siralos state and what a model sees, calls,
and consumes. It lives in core (`packages/core/src/projection/`) and is
provider-neutral, Node-free, and mutation-free (architecture-enforced: no
provider ports, no task-runtime mutation surface, no sandbox implementations,
no Godot modules). The CLI composition root wires one `ProjectionService`
into the application; provider adapters receive already-projected
provider-neutral inputs.

```text
packages/core/src/projection/
  context-capacity.ts      route working budgets (advertised vs verified vs
                           working maximum)
  context-estimator.ts     deterministic token estimator (UTF-8 bytes / 4)
  context-pressure.ts      normal | warn | auto | hard classification
  context-projector.ts     stability classes, stable fingerprint, system
                           prefix serialization, core instructions
  tool-projector.ts        available | gated | hidden, mode allowlists,
                           stable ABI fingerprint
  evidence-projector.ts    deterministic transforms, secret redaction,
                           truncation disclosure, never-worse rule
  conversation-trim.ts     pair-preserving conversation reduction
  watermark-cache.ts       high/low watermark hysteresis for disposable caches
  stale-result.ts          revision-bound async results
  projection-service.ts    composition: project -> estimate -> classify ->
                           fit/reduce -> provider (pre-flight)
```

- **Context**: `ContextProjector` projects stable (core instructions),
  contextual (task contract, task state, acceptance, phase, findings), and
  volatile (latest evidence) segments. The provider `system` prefix is the
  deterministic serialization of stable + contextual segments; its
  `stableFingerprint` and stable bytes are unaffected by ordinary volatile
  changes (prompt-cache stability). Context is disposable: deleting a
  projection never loses task knowledge, and context is reconstructed from
  authoritative TaskContract/TaskState each turn.
- **Budgets and pressure**: the route `workingMaximum` is authoritative
  over advertised maximums. Every turn runs the pre-flight pipeline;
  `auto` performs deterministic pair-preserving conversation reduction
  (tool call + result pairs are dropped whole, the active request
  survives, TaskContract stays in the system prefix), and `hard` blocks
  the provider call entirely — the provider is never invoked with
  knowingly over-budget context, and provider rejection is never used as
  flow control. `warn` surfaces `context_pressure` events and status.
- **Tools**: `ToolProjector` classifies every registered tool as
  available / gated / hidden from (mode ∩ capability policy ∩ provider
  capability). Hidden tools are absent from the provider schema — never a
  runtime "permission denied". The projected ABI has a stable fingerprint;
  ordinary task progress does not reorder it. Modes: `generic` (session
  default), `development` (exactly the GDScript workflow's tool set),
  `review` (read-only: no mutation/process tools even if registered),
  `inspection`. A route without tool calling fails clearly for
  tool-requiring modes instead of silently degrading. Projection is not
  the security boundary: every invocation still passes capability policy,
  approvals, scope checks, and sandboxing.
- **Evidence**: `EvidenceProjector` is the boundary between authoritative
  raw evidence and the bounded model view. Model views are disposable
  copies: ANSI stripped, repeated lines collapsed, configured secrets
  redacted (the single core redaction primitive; never reverted by size
  rules), lines bounded, and total size truncated with an explicit marker
  and byte metadata plus a reference to the raw evidence. The never-worse
  rule retains the bounded original when a reduction would inflate it.
  Structured diagnostics/process results remain structured; only textual
  summaries are transformed. Disposable model-evidence views live in a
  high/low-watermark cache that never evicts durable task evidence.
- **Async**: async projection/helper results are revision-bound
  (`RevisionGuard`/`awaitCurrent`); results completing after a state
  advance are discarded, never injected into a newer turn (deterministic
  fake scheduling in tests). Provider streaming stays bounded by the
  existing cumulative per-turn limits — a drip-feeding stream cannot live
  forever; idle-timeout/hard-lifetime timers are deferred until a real
  provider requires them.
- **Integration**: `/develop` prompts run in `development` mode through
  the projection service; the independent reviewer runs in `review` mode
  (final-boundary tests prove mutation tools are absent from the actual
  reviewer provider request, and the reviewer never receives implementer
  private state, secrets, approval capability, or the implementer
  transcript). CLI observability: `/context` (segment sizes, working
  budget, pressure, tool ABI fingerprint), `/tools` and
  `/development-status` projection lines.

## Workspace revision and structural reads

The workspace revision layer (Stage 3 milestone 3, ADR 0016) gives Siralos a
stronger model-facing file identity system and cheaper structural
exploration. Identity logic and GDScript structural extraction live in core
(`packages/core/src/workspace/`) — provider-neutral, Node-free, and
mutation-free (architecture-enforced: no provider ports, no task-runtime
mutation surface, no sandbox implementations, no checkpoint/mutation
machinery, no Godot modules).

```text
packages/core/src/workspace/
  workspace-revision.ts    opaque rev_ handles, session-scoped bounded
                           registry, invalidation, observed-read tracking
  workspace-read-mode.ts   exact | structural | summary
  gdscript-structure.ts    lightweight deterministic GDScript scanner
                           (declarations, signatures, dependencies)
  workspace-summary.ts     bounded structure-first advisory summaries
```

- **Revision handles**: an exact/structural/summary read issues a
  `rev_<32 hex>` handle bound to `(workspace fingerprint, path,
whole-file SHA-256)`. The handle is an ergonomic reference, never
  authority: the host resolves it to the trusted SHA-256 before any
  mutation, capability policy and containment remain authoritative, and the
  same relative path in a different workspace never resolves. The registry
  is session-scoped, in-memory, and bounded; a successful mutation issues
  the new post-edit revision and invalidates the previous current binding,
  while old revisions stay resolvable as historical evidence. A
  session-local observed-reads record (`path, revision, mode`) is
  groundwork for future multi-agent stale-read detection — a historical
  note: multi-agent machinery is not core architecture (ADR 0036).
- **Read modes**: `workspace.read` supports `exact` (authoritative source,
  revision handle, SHA-256, bounded text/range — the only basis for text
  mutation), `structural` (deterministic GDScript declarations with
  string/comment awareness, bounded output, honest `partial` results for
  invalid syntax), and `summary` (bounded advisory overview that always
  states its revision and its advisory status; the footer is never
  truncated away). Non-GDScript files get an explicit unsupported result;
  binary/special files are never decoded as UTF-8; path containment,
  excluded directories, and protected paths apply identically to all
  modes.
- **Revision-aware mutations**: change-set edits/deletes accept either the
  legacy raw `expectedSha256` or an opaque `expectedRevision` (resolved by
  the host to its SHA-256); the existing prepare/apply revalidation is
  unchanged. A mismatch yields a structured `stale_revision` result
  (path, expected/current revision) and guidance — no fuzzy merge, no
  silent retry, and a second edit requires a fresh post-edit revision.
  Multi-file change sets fail as a whole when any member revision is
  stale.
- **Revision-aware evidence**: workspace-derived evidence can carry the
  revision handle; the development bridge attaches the post-edit revision
  to mutation evidence so parser/LSP validation is bound to the resulting
  revision where the architecture naturally supports it; EvidenceProjector
  model views preserve the revision; the ContextProjector volatile segment
  shows it.

## Project instructions and knowledge

The instruction/knowledge layer (Stage 3 milestone 4, ADR 0017) is the
host-owned foundation that keeps behavioral guidance, durable project
facts, and historical evidence strictly separate before References,
planning, or scene/resource intelligence exist. The authority order is:

```text
Security Policy  >  Instructions  >  TaskContract  >  Knowledge  >  History/Evidence
```

Security policy is outside the instruction resolver entirely; project
instructions can never broaden it; knowledge can never grant capability or
override policy; history is never promoted to knowledge without the
coordinator. All three are distinct sections in the projected provider
context, never concatenated into one authority class.

```text
packages/core/src/instructions/   structured model, single resolver,
                                   precedence, conflicts, revision identity
packages/core/src/knowledge/      fact model, KnowledgeCoordinator (single
                                   writer), bounded retrieval + traces,
                                   conservative Godot seeding, rendering
packages/core/src/security/
  behavioral-config.ts            shared protected-behavioral-config classifier
packages/adapters/src/instructions/
  instruction-discovery.ts        bounded AGENTS.md discovery (containment-
                                   enforced, revision-bound)
```

- **Instructions**: `ProjectInstruction` carries source
  (`project_root`/`project_directory`; `managed`/`user`/`task` slots
  reserved), scope (workspace-relative), deterministic precedence
  (smaller = more authoritative: host > TaskContract > managed > user >
  project root > directory scope), and a revision bound to the exact
  `AGENTS.md` file revision. One pure resolver owns resolution semantics:
  `resolveForPath`, multi-path union preserving scope, deterministic
  ordering (root → nested directories), and structural conflict detection
  that surfaces same-layer same-scope contradictions (both sides
  preserved, never silently dropped). The adapter service discovers
  `AGENTS.md` files with the same containment as workspace reads (canonical
  root, symbolic links never traversed, bounded depth/files/bytes,
  `node_modules`/`.git`/`.siralos` excluded); URLs in instruction content
  are plain text — no remote instructions are ever fetched. The resolver
  is architecture-enforced to stay provider-neutral and mutation-free.
- **Protected behavioral configuration**: `AGENTS.md` (any depth) and
  `.siralos/**` are classified by one core predicate shared by the pure
  change-set validator and the adapter write-path guards. Ordinary
  `workspace.write` never covers them: a change set touching a protected
  path is rejected before any write, approval, or checkpoint with a typed
  message, and the standalone create/edit/delete write targets include the
  same classifier. The dedicated protected-configuration authorization
  path (the `/evolve`-style surface) is future work and not offered.
- **Knowledge facts**: subject-keyed (`project.godot.version`, ...), one
  active immutable revision per project scope + subject, history retained
  (restoring an old value creates a new revision), provenance as evidence
  or exact workspace-file references, `low|medium|high` confidence,
  `volatile|normal|stable|evergreen` volatility with simple host freshness
  rules, optional expiry (expired facts drop out of automatic retrieval,
  never deleted), and `pinned|retrieved` activation. Candidates are
  validated before acceptance: subject-key shape, existing provenance,
  content/history bounds, known secrets, and conservative rejection of
  policy-shaped claims.
- **KnowledgeCoordinator**: the single application-owned writer. Providers
  and the CLI never write fact structures directly; they propose
  candidates. Exact normalized equality produces no revision churn;
  retirement removes the current pointer and retains revisions. In-memory
  and serializable with documented schema version `knowledge-1`; future
  persistence must define quotas, history retention, pruning, and archive
  behavior.
- **Pinned and retrieved knowledge**: a small bounded pinned set (fact and
  byte budgets) enters stable/contextual context automatically; everything
  else is retrieved on demand by a deterministic explainable scorer
  (exact/prefix subject match, keyword overlap, provenance path
  relevance, confidence + freshness weights; documented constants) with
  count/byte budgets, deterministic tie-breaking, and omissions recorded
  in the retrieval trace. The trace is for debugging, tests, user
  inspection, and future `/evolve` — never model authority.
- **ContextProjector integration**: the projected context carries distinct
  titled sections — `[Siralos instructions]`, `[Project instructions]`,
  `[Project knowledge]` (pinned), `[Task-relevant knowledge]` (retrieved,
  task-stable basis), `[Task contract]`, `[Task state]`, `[Latest
evidence]` — with knowledge framed as factual context that never grants
  permissions, changes policy, or overrides the task contract. Only small
  explicitly pinned knowledge enters the stable/contextual prefix;
  retrieval is keyed to the task request and the paths the task actually
  read, so incidental facts never churn the cacheable prefix.
- **Deterministic seeding**: the CLI seeds a few high-confidence facts
  from the static project profile (engine version, language profile,
  has-dotnet, project name) with exact `project.godot` provenance.
  Architectural ownership is never inferred from weak evidence.
- **Task provenance**: task runtime snapshots record the instruction
  inventory revision and knowledge-state revision at task start
  (`instructionSetRevision`, `knowledgeStateRevision`), so historical task
  provenance identifies the guidance and knowledge that influenced it.
- **CLI surface**: `/instructions` lists discovered instruction files with
  their revisions; `/knowledge` lists current facts (revision, confidence,
  volatility, pinned status, retired subjects); `/knowledge why` shows the
  latest retrieval trace. Both are read-only.

## Workspace, reference, and research resource classes

The external-reference and research layer (Stage 3 milestone 5, ADR 0018)
gives Siralos first-class, read-only access to upstream material — local
directories outside the workspace and remote repositories pinned to
immutable commits — plus bounded, host-coordinated fetching of external
evidence (repository files, Godot documentation). Three resource classes
are separated explicitly:

```text
WORKSPACE   editable project state (the canonical launch directory)
REFERENCE   read-only external material (a local directory outside the
            workspace, or a remote repository pinned to a commit)
RESEARCH    transient external evidence fetched through bounded source
            ports (repository files, Godot documentation)
```

The governing statement block:

```text
Reference content is read-only untrusted data.
Research content is transient external evidence.
Neither is instruction authority. Neither grants capability.
Neither becomes project knowledge automatically.
```

Namespace separation is structural, not advisory:

- `@reference/<alias>` names are **never filesystem paths**. They are
  model-facing labels for declared reference slots; the reference
  namespace is separate from the workspace namespace, and workspace tools
  resolve only workspace-relative paths.
- **Reference roots must be outside the workspace.** The registry refuses
  any local-directory reference that resolves inside the workspace
  namespace at resolution and at refresh, and the access layer re-verifies
  every root before serving content (defense in depth).
- **Workspace mutation tools cannot target reference roots.** The
  workspace mutation surface never covers reference content: reference
  paths are rejected before any write, approval, or checkpoint, and there
  is no reference mutation surface at all (behavior fixture 51).
- **Managed cache paths are never model-facing.** Repository
  materialization would live in Siralos-owned private storage outside the
  workspace (`~/.siralos/references/<fingerprint>/`); no absolute cache
  path ever reaches the model, and cache content is never presented as
  workspace material. At this stage repository materialization is
  `unavailable` (it requires sandboxed Git execution, which does not
  exist) — nothing is spawned, fetched, or created; local-directory
  references are direct read-only roots with zero filesystem operations.

```text
packages/core/src/reference/      reference model, declaration parsing
                                   (strict, bounded, unknown keys rejected),
                                   ReferenceRegistry (SINGLE owner of
                                   reference identity), ports, evidence views
packages/core/src/research/       research model + bounds, source/transport
                                   ports, ResearchService (policy gate,
                                   validation, timeouts, cancellation,
                                   provenance, stale-result binding)
packages/adapters/src/reference/  resolvers (local-directory manifest
                                   fingerprint; repository backend reports
                                   unavailable), materializer (fail-closed),
                                   cache store (no-op), root-relative
                                   containment, access port, reference tools
packages/adapters/src/research/   node:https transport (bounded, https-only),
                                   GitHub + Godot-docs source adapters,
                                   bounded normalization (shared)
packages/adapters/src/tools/reference/
                                   reference.list / reference.read /
                                   reference.search tools
```

- **Read-only reference registry**: the `ReferenceRegistry` is the single
  application-owned owner of reference identity. Declarations are parsed
  from the untrusted `reference` config section (aliases
  `^[a-z][a-z0-9._-]{1,63}$`, bounded count/description; unknown keys
  rejected), resolved at creation through the resolver port, and recorded
  as immutable revisions; the only way a revision changes is an explicit
  `refresh`. Mutable refs (branch / absent ref) are refused unless
  `allowMutableRefs` is set, and resolution always records the resolved
  commit. A failed refresh invalidates the current revision (fail closed)
  but historical revisions stay reachable through task bindings and
  evidence. Declined/unresolvable references remain listed with precise
  reasons. Local-directory identity is a canonical path plus a bounded
  manifest fingerprint (every regular file SHA-256'd, per-file and
  manifest caps; symlinks never traversed, special files fail the
  manifest — a non-fingerprintable reference is never silently marked
  "unhashed").
- **Reference access**: `reference.list` / `reference.read` /
  `reference.search` under the `reference.inspect` capability (allowed in
  every built-in profile). Paths are reference-root-relative with
  containment equal to workspace paths (`..` rejection after segment
  normalization, symlink escapes rejected against the canonical root,
  null bytes rejected). Read modes mirror `workspace.read` — `exact`
  (SHA-256, the only authoritative source), `structural` (deterministic
  GDScript declarations via the core parser), `summary` (bounded
  advisory) — with caps (1 MiB files, 64k content chars, non-UTF-8
  `unsupported`) and search with independent global traversal budgets and
  explicit truncation. Every result carries the registry-owned revision
  anchor (commit or fingerprint) — the adapter never resolves, refreshes,
  or infers identity itself.
- **Research**: the `ResearchService` is the single gate: the
  `research.fetch` capability must evaluate to `allow` before any source
  port is invoked (`ask` is refused — no approval protocol exists), every
  built-in profile denies it, and hidden research tools are absent from
  the provider schema. Requests are validated against the bounded model,
  must name a configured source, and race the caller's abort signal and a
  timeout with caps at download (2 MiB), document (256 KiB), section, and
  link layers. Provenance records requested vs resolved identity
  (`requestedRef`/`resolvedRevision`, `requestedVersion`/`usedVersion`,
  explicit `fallback` marking — e.g. Godot docs patch → minor → stable).
  Results are stale-result-bound inside the service to the exact active
  task id and TaskContract revision. The service snapshots that identity
  before invoking a source and checks it again before returning or retaining
  a document; stale results are discarded before entering evidence or
  context, so callers cannot omit the check. The real adapters cover a narrow read-only scope —
  GitHub known-file content and latest-release notes
  (`research.repository`) and Godot documentation class/search pages
  (`research.godot_docs`) — through a single bounded https-only
  node:https transport whose exact DNS-host allowlist applies to the initial
  URL and every redirect under one absolute deadline.
- **Evidence and knowledge integration**: task evidence gains
  `reference_read`, `reference_search`, and `research` kinds with bounded
  sources; the ContextProjector renders volatile `[Reference evidence]`
  and `[Research evidence]` sections after `[Latest evidence]` (bounded,
  truncated explicitly, never stable/contextual, never absolute paths);
  a `KnowledgeCandidate` may cite `research_evidence` provenance only
  through an explicit host-verified `propose` — never automatically.
- **CLI surface**: `/references` lists configured references with
  status/materialization/trust/revision; `/reference <alias>` shows one
  reference's identity and availability; `/research-status` shows the
  research capability decision, configured sources, and recent evidence;
  `/status` adds a research line. All read-only.

## Deferred: persistence

Sessions are in-memory only. No SQLite, transcript storage, or session restoration exists. TaskState may remain in memory by design for this milestone: the types are serializable, no runtime handles are embedded in domain state, and the persistent-state schema/versioning requirements are documented in ADR 0014 (a future persistence milestone can rely on `runtimeVersion`, the revisioned contract history, and the append-only activity log). A persistence port will be added only when a real requirement demands it.

## Deferred: multi-agent functionality

One Run is one selected Profile and one primary model/tool loop (ADR 0036).
Multi-agent machinery — subagents, agent teams, Fleet, TaskGraph, worker
hierarchies, distributed workers — is not core architecture and is not
committed roadmap work; it may be reconsidered only from concrete demand and
evidence. `/evolve` remains a committed future feature in Stage 6.

## Deferred: process and write tools

The sandbox boundary, profiles, policy evaluator, environment filtering, and conformance suite exist; the workspace-mutation entry points (`workspace.create_file`/`workspace.edit_file`/`workspace.delete_file` and `/undo`) fail closed as `unavailable` before any write, approval, or checkpoint, and what remains is the core contracts, reusable tested primitives (path validation, diffing, hashing, the mutation lock, safe-replacement helpers), and the filesystem checkpoint store with startup reconciliation — Node offers no directory-relative (openat/renameat) primitive, so no approval for mutations is ever requested and no new checkpoint is ever created at this stage (historical checkpoint data from earlier sessions, if any, may still be listed; the former preview/approval/application logic was largely deleted and the identity-bound commit design is documented as future work). Read-only Git inspection (`git.status`, `git.diff` through a trusted allowlisted adapter) is unavailable at this stage: the adapter requires verified Siralos-owned private run directories for every sandboxed Git process, and run-directory creation and cleanup fail closed (no directory-relative or delete-by-handle primitive); Git can only ever execute inside an enforcing sandbox backend and is never spawned outside it. The sandboxed validation-command surface (`process.run`, `/commands`, `/cancel` — structured arguments only, read-only workspace, denied network, minimal environment, closed stdin, bounded streamed output, digest-bound one-time approval, timeouts, and process-tree cancellation) exists, but no command can execute at this stage: both the `node-script` and `npm-script` runners fail closed as unavailable — the pinned Node runtime cannot mechanically bind execution to the approved script bytes (the script can reach internal surfaces such as `process.binding` (e.g. `spawn_sync`) to spawn an unconstrained interpreter, and the staged private copy can be substituted by a same-user process in the verify-to-launch window) — and private run-directory creation is unavailable. No general shell, arbitrary executable runner, writable command execution, package installation, interactive stdin, background process, or normal Godot project execution exists; Siralos does not open, import, execute, or run any Godot project (the recovery-mode project-probe surface — ADR 0009 — is restored as contracts, bounded static preparation, a one-time approval protocol with expiring single-use prepared probes, diagnostics, CLI reporting, and structural architecture enforcement of the fixed recovery invocation, but execution fails closed as unavailable on every platform: the disposable mirror and the recovery runner are fail-closed no-ops that never create, delete, or launch anything, because Node offers no exec-by-handle, no directory-relative create, and no delete-by-handle primitive), and Godot engine probing is itself intentionally unavailable at this stage; those remain deferred and, when added, must execute under the same enforcement.

## Git inspection

Core owns the Git-neutral contracts (`GitInspector` port, status/diff models, `GitError` categories) and knows nothing about processes or Git syntax. The Git adapter (`packages/adapters/src/git/cli`) owns exact invocation: a fixed allowlist of subcommands (`version`, `rev-parse`, `status`, `diff`), fixed argument arrays with no shell, a sanitized environment with Git safety variables, independent output bounds, timeouts, cancellation, and launch-time re-verification of the resolved executable (canonical identity, regular non-link file). **Git can only ever execute inside the sandbox backend under `validation-offline`** (network denied, writes limited to the exact private run directory, host reads limited to the repository root and trusted Git runtime roots, confined process tree — repository-configured helper code such as clean filters could only ever run inside that confinement, never on the host); the command-line overrides are defense in depth (the enumerable mechanisms are disabled; clean/smudge/process filters are not disabled from the command line and their only containment is the sandbox). When the backend cannot enforce the boundary, the adapter reports Git unavailable and never spawns Git. **Git inspection is unavailable at this stage**: the adapter requires verified Siralos-owned private run directories for every invocation, and run-directory creation and cleanup fail closed (no directory-relative or delete-by-handle primitive in Node), so nothing Git-related executes until that primitive exists. The adapter itself never spawns processes (enforced structurally by the architecture check; only the sandbox backend executes the Git process). It parses `--porcelain=v2 -z` status records and unified diff patches into the core models; providers receive only these structured results through the `git.status` and `git.diff` tools, and the CLI renders them without parsing raw Git output. The repository root must equal the workspace root; anything else is a structured failure, and non-Git workspaces remain fully supported.

## Recovery checkpoints

Core owns the checkpoint model, lifecycle rules, the `CheckpointStore` port, undo planning, and undo conflict rules; it never touches the filesystem. The filesystem checkpoint store (`packages/adapters/src/checkpoints/filesystem`) owns storage at `~/.siralos/checkpoints/<workspace-fingerprint>/<checkpoint-id>/`: atomic metadata replacement, preimage persistence, hash validation, symlink rejection, fail-closed retention limits (no automatic pruning: reaching the count or byte limit — or failing to prove capacity because any checkpoint's metadata or preimage is unreadable, invalid, oversized, linked, or size-inconsistent, or because content beyond the exact `metadata.json`/`preimage.bin` layout (unknown files, nested directories, temporary files, case-variant duplicates, links, special files) is present — refuses new checkpoints with a typed storage-limit error before any write and deletes nothing; the byte limit measures actual regular-file bytes beneath the checkpoint directory including metadata and preimages, declared preimages are content-verified through a shared handle-bound bounded verifier (exact bytes read must match the metadata byte length and SHA-256 via an explicit-offset read loop — a short read is never treated as EOF — with the opened handle and pathname identity proven against the pre-open lstat snapshot and a pre-read/final stability snapshot (identity, size, and mtime/ctime nanoseconds) captured from the handle before reading and re-verified after it, so a same-size corrupted preimage, a same-content hard-link/rename/symlink/junction substitution, or a same-inode in-place rewrite during verification is a refusal; `O_NOFOLLOW` is applied on POSIX while Windows carries the binding through the identity/stability comparisons, and unusable identity or stability fields fail closed), configured preimage limits are capped at 64 MiB with larger values rejected at store creation, checkpoint operations are bound to their before/after existence states (create: absent→present; update: present→present; delete: present→absent) by one shared validator for prepared and stored records, with the proposed checkpoint's exact serialized metadata and preimage bytes counted before any write, and unexpected content is never repaired, renamed, truncated, or quarantined; no checkpoint entry is skipped during retention; existing checkpoints are preserved), and the `reconcileWorkspaceCheckpoints` startup pass. **No new checkpoint is ever created at this stage** — every mutation fails closed as `unavailable` before recording — and `/checkpoints` may still list historical checkpoint data from earlier sessions if any exists. The undo service fails closed as `unavailable` (restoring requires pathname-based displacement, and Node offers no directory-relative primitive); the former reverse-diff/approval/restore machinery was largely deleted and the identity-bound design is documented as future work, while the store itself is tested internal code. The CLI's `/git-status`, `/diff`, `/checkpoints`, and `/undo` commands are thin renderers over these core-owned ports; the CLI never parses Git output and never restores files directly.

## Self-reference and capability diagnostics (Stage 3 milestone 6, ADR 0019)

Siralos explains its own installed behavior through host-owned surfaces
instead of model memory:

```text
Installed Siralos Runtime
        │
        ├── SelfReference (@siralos)
        │       ↓
        │   exact current docs/config/capabilities
        │
        └── CapabilityDoctor
                ↓
           deterministic read-only diagnosis
```

### SelfReference (`packages/core/src/self`, `packages/core/src/commands/command-catalog.ts`)

- `COMMAND_CATALOG` is the single source for the interactive command
  vocabulary: `parse-input.ts` derives the `SlashCommand` union from it,
  `/help` renders its descriptions, and the self-reference documents it.
  A command cannot exist in the session without being catalogued, and it
  cannot be catalogued without being documented — no hand-maintained
  command list can drift.
- `createSelfReference` builds bounded sections from authoritative
  metadata: runtime identity (installed package version, Node major,
  platform), commands, configuration surface (`CONFIG_SCHEMA_SUMMARY`,
  conformance-tested against `schemas/user-config.schema.json`),
  capability ids (`CAPABILITY_IDS`), sandbox profiles
  (`SANDBOX_PROFILE_IDS`), the registered tool surface, Godot capability
  status, references/research configuration, Task Runtime concepts, and
  the doctor surface.
- A stable revision fingerprints the installed surface (version +
  command catalog revision + config schema revision + capability schema
  revision + tool ABI revision). No Git metadata is invented when
  unavailable in packaged builds.
- The self-reference is retrieved on demand via the read-only
  `self.read` / `self.search` tools (`self.inspect`, allowed in every
  built-in profile). Full documentation is never injected into prompts.
  There is no mutation tool for it.

### CapabilitySnapshot (`packages/core/src/doctor/doctor-model.ts`, `capability-state.ts`)

- `CapabilityState` distinguishes available / configured / unavailable /
  unsupported / degraded / blocked_by_policy / requires_approval /
  unknown. `CapabilitySnapshot` is a typed observation of the current
  runtime (providers, sandbox, workspace, godot, references, research,
  tools); it grants nothing — SandboxBackend, ToolProjector, and the
  security layer stay authoritative.

### CapabilityDoctor (`packages/core/src/doctor`)

- `DoctorSources` is the single port through which the doctor queries
  authoritative subsystem owners (sandbox backend inspect, Godot
  inspector doctor, reference registry, research service, ToolProjector,
  task runtime, config loader, Git, checkpoint store). The doctor never
  re-implements subsystem logic.
- Ten areas (runtime, configuration, providers, sandbox, workspace,
  godot, project, references, research, capabilities), typed checks with
  pass/warn/fail/skip, per-check timeouts, deterministic ordering, and
  documented exit codes (0/1/2; warnings never fail).
- The doctor is read-only and offline by default: no network, no live
  probes, no refreshes, no mutations, no checkpoints. Required sandbox
  enforcement failures are `fail`, never "warn but usable". Recovery /
  LSP / check-only operations are reported as "available but requires
  approval" and never triggered. Task runtime snapshots are compared as
  diagnostic facts and never mutated.
- Safe reports (`--report-safe`) drop details/remediations and sanitize
  summaries (absolute paths and credential-shaped tokens); they keep OS
  family, Node major, and version and are NOT anonymous. JSON output
  (`--json`) is schema-versioned and deterministic.

### Dependency direction

Core doctor/self modules never import network modules, fs, mutation /
undo / checkpoint machinery, default-policy construction, or projection
internals; projection never imports the doctor; safe-report rendering is
separate from diagnostic collection; self-reference tool adapters carry
only the fixed `self.inspect` capability. The CLI composition root wires
everything: `siralos --doctor [area] [--json] [--report-safe]`, `--self`,
`/doctor [area]`, `/siralos`.

## Host-controlled planning (Stage 3 milestone 7, ADR 0020)

Planning is a runtime-owned phase. The model may propose a plan, but the
host decides whether planning is needed, how deep it is, and whether it
is approved:

```text
TaskContract
    ↓
PlanningPolicy (deterministic host routing)
    ├── none   → existing Task Runtime path (no planner call)
    ├── light  → read-only plan → host validation → execution
    └── full   → read-only plan → host validation → plan approval → execution
```

### Ownership

- **PlanningPolicy** (`packages/core/src/planning/planning-policy.ts`)
  — pure deterministic depth routing from host-visible task facts;
  identical inputs produce identical decisions; never a model call.
- **Planner** (`packages/adapters/src/planning/planner-executor.ts`) —
  advisory and read-only; proposes structured plan content only; fresh
  provider context per attempt; bounded budget with stall detection.
- **TaskPlan** (`packages/core/src/planning/planning-model.ts`) —
  immutable revisioned planning artifact bound to the exact TaskContract
  revision; identity is host-assigned; revisions only advance by one.
- **TaskState** (`packages/core/src/tasks/task-runtime.ts`) — owns
  execution progress and carries a bounded plan reference (id, revision,
  depth, state, approval, stale reason). Plan steps never become
  competing mutable progress.
- **Approval system** (`packages/core/src/security/approval.ts`) —
  authorizes plan acceptance and mutations separately. Plan approval
  binds to the exact plan revision and TaskContract revision; it never
  authorizes source edits or commands.

### Read-only planner

The planner capability profile is structurally read-only, enforced three
ways: the composition-root registry contains only read-only tools
(workspace inspection, Godot inspection/API knowledge, references,
policy-gated research, self-reference); the executor refuses every
prepared or non-read-only tool at the runtime boundary; and the
ToolProjector `planning` mode hides mutation, process, and approval-grant
tools from the provider-visible schema. A visible but gated tool is returned
as a failed tool result and never executes because planning has no approval
protocol. The planner cannot approve its
own plan, approve edits, broaden capabilities, mark validation complete,
or mark the task complete, and it cannot choose planning depth.

### Plan lifecycle

```text
planner output (untrusted)
    ↓ validatePlanCandidate (depth match, bounds, paths, revisions,
    │   acceptance refs, secrets, policy-shaped claims)
    ↓ createTaskPlan / reviseTaskPlan (host-owned identity)
    ↓ handle.setPlan  → plan_created
    ↓ handle.approvePlan (exact revision binding) → plan_approved
    ↓ TaskContract revision change → stale + approval invalidated
```

### Plan approval semantics

```text
approve plan rev N  ⇒  only plan rev N is approved
plan becomes rev N+1 ⇒  rev N approval is invalid (refused)
TaskContract rev advances ⇒  plan stale, approval invalid (refused)
```

Plan approval does not approve edits: every source mutation still
requires prepared exact diff → one-time mutation approval → checkpoint →
apply. Plan requirements are descriptive and grant nothing.

### CLI

- `/plan <request>` — plan-only mode: read-only planning, structured plan
  printed, zero workspace changes, zero mutation checkpoints, no
  execution follows.
- `/develop [--plan|--plan-light] <request>` — host-controlled routing
  before the executor provider call; full plans ask for plan approval
  through the interactive reviewer; verified-touchpoint staleness and the
  full-plan acceptance-and-exact-approval gate surface before execution;
  denial/cancellation terminates the workflow, and a stale verified
  touchpoint invalidates the plan before the executor boundary.
- `/development-status`, `/status`, `/task-status` — planning depth, plan
  revision, plan state, approval state, staleness.

### Dependency direction

Core planning modules never import provider ports, security/capability/
approval machinery, mutation/checkpoint/development machinery,
projection, or Godot modules (generic digest allowed). The planner adapter
never imports workspace-mutation, process, checkpoint, sandbox, or
environment adapters, and never imports planning policy/flow surfaces.
Provider adapters never import planning policy/flow identifiers —
providers never choose depth. TaskState remains the execution authority;
the approval subsystem remains the authorization authority; ToolProjector
remains the model-visible tool authority.

## Read-only Godot scene and resource intelligence (Stage 3 milestone 8, ADR 0021)

Godot project files become derived, read-only, revision-bound semantic
state — never a new source of truth:

```text
Godot source text (.tscn / .tres / project.godot)
    ↓
revision-aware parser (static, process-free)
    ↓
derived semantic model (GodotSceneModel / GodotResourceModel)
    ↓
relationship index + inspection/query tools
    ↓
EvidenceProjector → ContextProjector ([Scene evidence]) → planner / developer / reviewer
```

Explicitly stated:

- **semantic model ≠ source of truth** — source files, workspace
  revisions, and Godot itself remain authoritative; models are disposable
  derived projections bound to the exact revision they were parsed from.
- **inspection ≠ execution** — no Godot process, no `@tool` scripts, no
  plugin activation, no imports, no project loading.
- **inspection ≠ mutation** — `godot.inspect_scene` / `godot.inspect_resource`
  / `godot.dependencies` are read-only under the `godot.inspect`
  capability; no scene/resource mutation tools exist, and `/develop`
  refuses `.tscn`/`.tres` change sets at the validation boundary.
- **scene relationship ≠ runtime state** — serialized parent/owner,
  inheritance/instancing, groups, and connections are serialized facts;
  no runtime state is implied.
- **serialized signal connection ≠ verified runtime behavior** — missing
  endpoints are structural diagnostics; semantic validity is never
  claimed without Godot/script evidence.

### Ownership

- **Parsers + models** (`packages/core/src/godot/scene/`) — pure
  Node-free domain: `text.ts` (bounded lexer), `variant.ts` (conservative
  Variant values), `scene-parser.ts`, `resource-parser.ts`,
  `scene-tree.ts`, `models.ts`, `limits.ts`, `resolution.ts`,
  `relationship-index.ts`, `intelligence.ts` (port). Nothing here imports
  adapters, providers, security, tasks, projection, or tool machinery.
- **Intelligence service** (`packages/adapters/src/godot/intelligence/`) —
  the single application-owned subsystem for current parsed state: bounded
  workspace reads, revision issuance, parse binding, index recording,
  bounded cycle-safe dependency traversal, and project relationship
  resolution. It never imports mutation, process, checkpoint, sandbox,
  environment, provider, or Godot engine-execution machinery.
- **Tools** (`packages/adapters/src/godot/tools/`) — read-only tool
  surfaces bound to the service; ToolProjector stays the model-visible
  tool authority (tools added to the development/review/inspection/
  planning mode surfaces under `godot.inspect`).
- **Projection** — `[Scene evidence]` is a contextual, bounded,
  project-data section (never instructions); evidence views carry
  path + revision + parse status + a bounded structural summary.
- **Planning** — verified scene/resource touchpoints carry the exact
  revision and `scene:`/`resource:` evidence references; scene/resource
  involvement is a deterministic complexity signal (never an automatic
  full-plan trigger).

### Supported format scope

Supported: Godot 4 text scene/resource syntax as listed in ADR 0021 —
headers (`format`, `load_steps`, `uid`), ext/sub resources, nodes
(name/type/parent/owner/instance/groups), connections, `[editable]`,
ordinary properties, conservative Variant forms (null, booleans,
integers, floats, strings, StringName, NodePath, arrays, dictionaries,
bounded vectors/colors, packed arrays, `ExtResource`/`SubResource`/
`Resource` references).

Not supported / honest limits: unknown Variant forms are preserved as
bounded opaque/raw data; partial parses are labeled `partial` with
diagnostics; UID resolution is limited to identity preserved in project
files (no editor-mode UID cache loading); binary `.res`/`.scn` are not
supported; parser bounds (nodes, resources, connections, nesting depth,
raw length, dependency depth/files, diagnostics) truncate explicitly and
never crash.

### Dependency direction

Core scene modules never import provider ports, security/task/checkpoint/
projection/tool machinery, or adapters. The intelligence adapter never
imports workspace-mutation, process, checkpoint, sandbox, environment,
provider, or Godot engine-execution adapters (runners, mirror, probe,
diagnostics, LSP, knowledge, development, quality). ContextProjector
consumes bounded semantic-model views, not raw parser internals. The
relationship index is application-owned and never holds source-of-truth
file contents. No scene/resource mutation API exists anywhere in the
milestone surface.

## Structured executor briefing and milestone acceptance (executor briefing foundation, ADR 0022)

Executor invocations are compiled from permanent rules plus a
milestone-specific delta instead of giant hand-maintained prompts. The
model has seven distinct artifacts; each stays host-owned and none of
them grants capability:

- **Execution Contract** (`packages/core/src/executor/execution-contract.ts`)
  — versioned, immutable permanent executor rules (Git discipline,
  security, architecture, standard validation, testing, reporting).
  Every rule references its real enforcement (`enforcedBy`); the
  contract never re-implements or bypasses it. `DEFAULT_EXECUTION_CONTRACT`
  is revision 1; changing it affects future tasks, never an active
  task's snapshot.
- **Milestone Manifest** (`packages/core/src/executor/milestone-manifest.ts`,
  `s3m8-manifest.ts`) — versioned, immutable milestone delta: goal,
  deliverables, invariants, non-goals, acceptance requirements with
  stable ids (`S3M8.PARSE.TSCN`), required tests, and deterministic
  architecture-concern tags. `S3M8_MILESTONE_MANIFEST` is the first real
  manifest (read-only Godot scene/resource intelligence, ADR 0021).
- **Acceptance IDs and Evaluator** (`packages/core/src/executor/acceptance.ts`,
  `standard-acceptance.ts`) — stable ids decoupled from test filenames;
  `STANDARD.*` reusable definitions; the evaluator maps requirements to
  host-attached evidence records and host-verified criteria only.
  Executor claims are structurally unrepresentable as evidence.
- **Executor Context Pack** (`packages/core/src/executor/context-pack.ts`)
  — derived, bounded context (contract/plan refs, path-scoped
  instruction refs, deterministic ADR selection, verified/candidate
  touchpoints, capability summary, findings, acceptance refs). Never a
  source of truth; never raw file contents.
- **Executor Brief Compiler** (`packages/core/src/executor/brief-compiler.ts`)
  — deterministic, bounded, fingerprintable compilation to a short
  brief that references `Execution Contract rev N` instead of restating
  permanent rules; bounds trim low-value context before
  invariants/acceptance/verified touchpoints.
- **Standard Validation Profile** (`packages/core/src/executor/validation-profile.ts`)
  — `standard-repo-validation` represents format/lint/typecheck/tests/
  behavior/architecture checks as a host-owned reference.
- **Briefing service and snapshot identity** (`packages/core/src/executor/briefing-service.ts`)
  — memoized per task-stable identity; the task runtime snapshot records
  execution-contract revision, milestone-manifest identity, and the
  initial brief fingerprint.

### Ownership

- **Model + compiler** (`packages/core/src/executor/`) — pure, offline,
  deterministic. Never imports provider ports, security/capability/
  approval machinery, task-runtime mutation, projection, knowledge,
  network, or Godot modules (generic digest excepted). Architecture
  checks enforce these boundaries.
- **Projection** — `[Executor brief]` is a contextual, bounded,
  project-data segment rendered from the compiled brief; only
  projection-service consumes the brief surface.
- **CLI** — `/brief` and `/milestone` render host-compiled artifacts
  (dry-run, provider-free); briefing semantics never live in the CLI.
- **Provider adapters** — never import or recreate executor briefing.

### Reproducibility

Brief compilation is deterministic: identical (contract, plan, contract
revision, milestone version, context) inputs produce byte-identical
briefs and fingerprints. Task runtime snapshots carry the
execution-contract revision, milestone identity, and initial brief
fingerprint, so an active task stays reproducibly tied to the artifacts
it started under even after the global contract advances.

### Milestone acceptance

Milestone acceptance is satisfied only by host-observed evidence
(attached evidence records, host-verified criteria). The milestone
manifest states what evidence kinds count; `STANDARD.*` references reuse
common definitions. The evaluator supports `pass`, `fail`,
`incomplete`, and `not_applicable`; a single incomplete requirement
keeps the milestone incomplete no matter what an executor claims.
