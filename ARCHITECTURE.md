# Architecture

## Overview

Solaris is a modular monolith: one repository, one npm workspace, one process, and clearly separated layers. See `docs/adr/0001-modular-monolith.md` for the decision record.

```text
@Solaris CLI (apps/cli)
    │  input parsing, rendering, process lifecycle
    │
    ├──→ @solaris/core  (provider port + application)
    │
    └──→ composition root (bootstrap/)
             │
             └──→ @solaris/adapters (concrete provider)
                       │
                       └──→ @solaris/core ports
```

## Core (`packages/core`)

Core owns Solaris application behaviour and its external contracts:

- Conversation model (`ConversationItem` union)
- Provider request/event contracts and the `ModelProvider` port
- Tool contracts and the immutable tool registry
- Application events and the bounded provider/tool loop
- The security model: `Capability`/`CapabilityPolicy`, built-in `SandboxProfile`s (`inspect`, `develop-offline`, plus the internal `validation-offline` used only for commands and `godot-probe-offline` used only for engine probes), the pure `evaluatePermission` function, the `SandboxBackend` port, classified `SandboxError` codes, and the `SolarisSecurity` facade (`evaluateCapability`, `checkSandbox`)
- The provider-neutral development-command contracts: `CommandRunner` preparation/execution contracts, the immutable `CommandRunnerRegistry`, `COMMAND_LIMITS`, the canonical command digest, the opaque single-use `PreparedCommand`, the `PreparedCommandTool` contract for `process.run`, and the `CommandApplicationEvent`/audit model

Core imports no Node infrastructure modules, no adapters, no CLI code, no terminal libraries, and no OS sandbox runtime. It never inspects the parent environment and never spawns processes. Architecture checks enforce all of this.

## Application layer

`createSolarisApplication({ provider, tools, maxToolRounds? })` returns the application: `sendPrompt(text, signal?)` streams `ApplicationEvent`s and `getStatus()` reports provider, state, and item count. State is private; only immutable views are exposed. The application owns conversation history, which providers must not.

The security facade (`createSolarisSecurity({ backend, policy, profile })`) sits beside the application in the composition root: `evaluateCapability` applies the pure permission evaluator, and `checkSandbox()` streams `sandbox_check_started` / `sandbox_check_completed` events from the backend's `inspect()`. The facade is consumed today by CLI diagnostics; future process and write tools will be gated through it before any backend call.

## Security model

- **Capability policy** (`CapabilityPolicy`) maps `workspace.read`, `workspace.write`, `process.execute`, and `network.outbound` to `allow` / `ask` / `deny`. Missing rules fail closed; explicit denies win; a profile can never broaden a denied policy; no built-in profile enables network.
- **Profiles**: `inspect` (read-only, no processes, no network — the default), `develop-offline` (workspace writes and process execution both require one-time approval, network denied, protected metadata paths, minimal environment, timeouts, output limits), `validation-offline` (internal: command execution is bound to a read-only workspace regardless of the user profile — no command can execute at this stage because both runners fail closed as unavailable), and `godot-probe-offline` (internal: the effective profile for engine probes — workspace excluded from readable roots, never writable — though engine probing is intentionally unavailable at this stage).
- **Backend port** (`SandboxBackend`): `inspect()` reports truthful per-platform status and capabilities; `execute(request)` runs a trusted `SandboxedProcessRequest` (executable + arguments, never a raw shell string; optional bounded output streaming, explicit timeout, and per-stream hard limits) and returns a bounded `SandboxedProcessResult` with violations; `close()` resets backend state and is idempotent. Errors normalize into `SandboxError` codes.
- **User configuration**: `~/.solaris/config.json` selects the profile and backend (defaults: `inspect`, `auto`). Unknown profiles/backends fail validation; project repositories cannot broaden these settings; `validation-offline` is not selectable.
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
- Private run directories are **unavailable at this stage**: the design (each run lives under `~/.solaris/runs/<workspace-fingerprint>/<run-id>/` with private `home/`, `tmp/`, `npm-cache/`, and a script cache; every path component verified with no-follow semantics before anything is created beneath it, exclusive creation, the runs root outside the workspace, and the sandbox granted exactly the current run directory) is not offered because Node offers no directory-relative (openat/mkdirat-style) or delete-by-handle primitive. The provider performs zero filesystem operations: creation reports `unavailable` before creating anything, and cleanup reports a truthful failure while preserving anything that exists; cleanup failures are always observed and reported.
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
- User configuration (`loadUserConfig`): reads `~/.solaris/config.json`, defaults to `inspect`/`auto`, and rejects unknown profiles or backends.
- Child environments (`buildChildEnvironment`, `buildCommandEnvironment`): allowlist-based construction with denied credential patterns and fixed safe command values; the only sanctioned way to build a child environment.
- Command layer (`src/process`): trusted npm CLI resolution (used by the conformance suite), the `node-script` and `npm-script` runners (both fail closed as unavailable: `isAvailable()` returns false for both, because the pinned Node runtime cannot bind execution to the approved script bytes — the script can reach internal surfaces such as `process.binding` (e.g. `spawn_sync`) to spawn an unconstrained interpreter, and the staged private copy can be substituted by a same-user process in the verify-to-launch window), the private run-directory provider (creation and cleanup both fail closed as `unavailable`: Node offers no directory-relative (openat/mkdirat-style) or delete-by-handle primitive, so a same-user process could substitute a verified parent between identity verification and a pathname-based create, and cleanup could delete a substituted object — the provider performs zero filesystem operations and nothing is ever created or deleted), and the `process.run` tool that would execute approved plans through the `SandboxBackend` under `validation-offline`, granting the sandbox exactly the current run directory when identity-bound run directories exist. No module in this layer spawns processes.
- `AnthropicSandboxRuntimeBackend`: the first concrete `SandboxBackend`, wrapping `@anthropic-ai/sandbox-runtime@0.0.70` (pinned exactly). Only this module may import the runtime package. It enforces a deny-by-default host-read allowlist (deny `/` and re-allow the current run directory, the workspace when the profile allows it, the trusted runner executables, and the minimum system runtime paths on Linux/macOS; never reported generally available on Windows), gives every request its own per-execution filesystem/network configuration for its exact profile and run directory so no request can inherit a broader earlier profile and no request sees a shared sandbox home/temp or another run (requests with an explicit read-roots list receive exactly those identity-bound paths), serializes the complete global sandbox lifecycle (config selection, reset, initialize, wrap, spawn, execute, violations, cleanup) one request at a time with `close()` draining the queue, resets and reinitializes the shared manager when the effective configuration changes, streams bounded decoded output accounted on raw bytes with the hard limit enforced inside the crossing chunk, terminates the process tree on timeout/cancellation/output-limit, runs cleanup in `finally` on every path, and isolates failing output callbacks.
- Conformance runner (`runSandboxConformance`): writes fixed fixture programs into a temporary workspace and executes them through the backend, reporting pass/fail per probe. Host-read probes use existing regular files in representative unapproved locations selected independently of the deny surface, plus cross-run isolation and bidirectional profile-isolation probes.
- Godot adapters (`src/godot`): discovery (configured user installations — absolute paths only, edition hints — plus fixed-name PATH search with safe PATHEXT handling and macOS `.app` bundle resolution), the fail-closed probe runner (reports `unavailable` for every probe and never spawns the executable — the backend re-opens the staged copy's pathname at spawn time and a same-user process can substitute bytes between final verification and launch; no exec-by-handle primitive — so no engine profile can be produced), the engine-profile cache as an explicitly unavailable no-op component (never initialized, created, read, or written: `load()` is always a miss, `store()` returns a typed unavailable result, `count()` is 0, and the doctor reports it disabled — the earlier storage implementation was removed rather than retained as an unsafe surface), executable fingerprinting/version parsing/edition classification/selection ranking (designed to consume probe results), static project detection and profiling (rescans the complete bounded project on every inspection — no profile cache), and the `godot.inspect_engine` / `godot.inspect_project` tools. Only this adapter could ever invoke the engine, always through the sandbox backend; at this stage nothing spawns Godot, and providers never run Godot.

The workspace root is the canonicalized directory Solaris was launched from; it is stored privately by the tools and displayed in `/status`. Provider adapters never import sandbox, environment, tool, checkpoint, git, or process modules — the architecture check enforces that boundary.

## CLI (`apps/cli`)

The CLI is an input/output adapter:

- Reads terminal input and renders terminal output through a small `SessionIO` interface
- Parses slash commands (`/help`, `/status`, `/clear`, `/exit`, and the Godot commands `/godot`, `/godot-installations`, `/godot-project`, `/godot-doctor`) in a pure module separate from rendering
- Renders application events incrementally
- Handles process startup, EOF, `Ctrl+C`, and shutdown
- Exposes the `solaris` binary and the composition root

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
  const security = createSolarisSecurity({ backend: sandbox, policy, profile });
  const workspaceTools = [createWorkspaceListTool(workspaceRoot), /* ... */];
  const application = createSolarisApplication({
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
- `process.env` is never inspected in package source; child environments are built from an explicit allowlist, and the sandbox wrapper's runtime-required environment is merged under strict rules (wrapper-only variables added, Solaris-controlled values win collisions, denied patterns fail closed, Windows keys normalized case-insensitively).
- Direct `node:child_process` usage is limited to the sandbox backend module, the conformance runner, and test files (the `node:` prefix and the bare `child_process` spelling normalize to one rule); command runners and the Git adapter never spawn processes — Git executes through the sandbox backend.
- Raw process execution (`shell: true`, `exec(`, `execSync(`, `spawnSync(`) is prohibited in runtime code, with documented exemptions only for test fixtures and the embedded conformance probe sources; the checks are a developer guardrail using structural TypeScript parsing (imports, re-exports, static dynamic imports, aliases, and call sites) plus regex/text checks for constructs parsing cannot represent.
- Destructive filesystem APIs (`writeFile`, `unlink`, `rename`, `appendFile`, `createWriteStream`, `rm`, `rmSync`, and friends, including aliased and namespace forms) are limited to the workspace mutation modules, the conformance runner, the process adapter (run directories), and tests. Path-based recursive deletion (`rm`/`rmSync` with `recursive: true`) is prohibited in all production code even inside approved mutation directories: the rule resolves import bindings structurally, so direct, aliased, and namespace imports of `fs`, `node:fs`, `fs/promises`, and `node:fs/promises` are all caught, and the only exemptions are the exact host-side conformance runner file and test-support files — never a whole directory: Node offers no directory-handle-relative deletion primitive, so recursive removal cannot be identity-bound and is never offered. Non-recursive `rm` stays governed by the destructive-API location rule.
- Git mutation commands are rejected in runtime code both as string tokens and structurally in spawn argument lists.
- `npm run check:architecture` (see `scripts/check-architecture.mjs`) enforces these rules mechanically: prohibited imports, prohibited package dependencies, provider/sandbox isolation, process, environment, and write boundaries, and workspace dependency cycles all fail the check. It is a developer guardrail, not an OS security boundary; the checks use structural TypeScript parsing plus regex/text checks, and runtime-constructed module specifiers and string contents are documented limitations.

## Why a modular monolith

One process with explicit module boundaries is the smallest structure that keeps UI, application logic, and infrastructure separable without introducing distributed orchestration, message buses, or deployment complexity. Solaris can grow its later stages inside this boundary and can extract packages later if a real need appears.

## Why the fake provider is an adapter

Provider neutrality is a stated product requirement, so the provider contract belongs in core and concrete implementations live behind it. The fake provider exercises the port end to end without credentials or network access, which keeps development and tests self-contained.

## Why UI code does not own application behaviour

The application must remain usable without a terminal (headless tests, future Godot-facing surfaces). Conversation policy and state transitions live in core; the terminal only translates between user intent and application events.

## Godot engine discovery and profiling

Godot discovery and profiling follow the same inward pattern as the other capabilities: neutral contracts in core, a single implementation in adapters, thin commands in the CLI. The CLI never runs Godot itself; only the Godot probe adapter in `@solaris/adapters` invokes the engine, always through the sandbox backend. Providers never run Godot. Core remains Node-free and process-independent.

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
                                         flags, SOLARIS_GODOT /
                                         SOLARIS_GODOT_INSTALLATION overrides
```

- **Core owns**: the Godot models (engine profile, version and release channel, edition, capability sets, support classification), classification rules, the deterministic selection policy with recorded rationale, compatibility assessment, the `GodotProbeRunner` and `GodotInspector` ports, and probe/selection events. Core never discovers, invokes, parses, or stores anything itself.
- **Adapters own**: discovery (configured installations and fixed-name PATH search), the fail-closed probe runner and its designed invocation of the exact probe argument arrays, output parsing, project scanning, inventory, and cache storage. The probe adapter is the only code that could ever spawn Godot; at this stage probing fails closed, so nothing spawns Godot.
- **CLI owns**: command parsing, rendering, and composition — `/godot-installations` displays the recorded selection rationale, `/godot-project` renders static project findings, `/godot-doctor` reports discovery/selection/cache diagnostics, and the startup flags and environment overrides seed selection at the highest precedence.

**Probe argument discipline.** Probe invocation uses exactly three fixed tuples constructed by the single `fixedProbeArguments` constructor private to the Godot probe adapter: `--version`, `--help`, and `--dump-extension-api`. Project-affecting arguments (`--path`, `--upwards`, `--import`, `--scene`, `--script`) and `--editor` are never passed; there is no project path and no project working directory. The architecture check mirrors the runtime boundary in probe invocation code: non-fixed `--` tokens, concatenated construction, imported argument arrays, and construction outside the fixed runner are rejected (a developer guardrail using structural parsing plus regex/text checks; the runtime boundary is the private constructor). At this stage engine probing fails closed as `unavailable`, so no probe is ever invoked and nothing spawns Godot.

## Godot API knowledge and GDScript diagnostics

The knowledge and diagnostic layers follow the same inward pattern: neutral contracts in core, adapters own every process/filesystem/parse concern, the CLI renders. Providers and the CLI never spawn Godot; only the fixed runners in `@solaris/adapters` could, and at this stage they fail closed (ADR 0010).

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

## Deferred: persistence

Sessions are in-memory only. No SQLite, transcript storage, or session restoration exists. A persistence port will be added only when a real requirement demands it.

## Deferred: multi-agent functionality

The current workflow is one interactive primary agent. Multi-agent review, agents as a product concept, and `/evolve` are out of scope for the foundation stage and are not modelled in core.

## Deferred: process and write tools

The sandbox boundary, profiles, policy evaluator, environment filtering, and conformance suite exist; the workspace-mutation entry points (`workspace.create_file`/`workspace.edit_file`/`workspace.delete_file` and `/undo`) fail closed as `unavailable` before any write, approval, or checkpoint, and what remains is the core contracts, reusable tested primitives (path validation, diffing, hashing, the mutation lock, safe-replacement helpers), and the filesystem checkpoint store with startup reconciliation — Node offers no directory-relative (openat/renameat) primitive, so no approval for mutations is ever requested and no new checkpoint is ever created at this stage (historical checkpoint data from earlier sessions, if any, may still be listed; the former preview/approval/application logic was largely deleted and the identity-bound commit design is documented as future work). Read-only Git inspection (`git.status`, `git.diff` through a trusted allowlisted adapter) is unavailable at this stage: the adapter requires verified Solaris-owned private run directories for every sandboxed Git process, and run-directory creation and cleanup fail closed (no directory-relative or delete-by-handle primitive); Git can only ever execute inside an enforcing sandbox backend and is never spawned outside it. The sandboxed validation-command surface (`process.run`, `/commands`, `/cancel` — structured arguments only, read-only workspace, denied network, minimal environment, closed stdin, bounded streamed output, digest-bound one-time approval, timeouts, and process-tree cancellation) exists, but no command can execute at this stage: both the `node-script` and `npm-script` runners fail closed as unavailable — the pinned Node runtime cannot mechanically bind execution to the approved script bytes (the script can reach internal surfaces such as `process.binding` (e.g. `spawn_sync`) to spawn an unconstrained interpreter, and the staged private copy can be substituted by a same-user process in the verify-to-launch window) — and private run-directory creation is unavailable. No general shell, arbitrary executable runner, writable command execution, package installation, interactive stdin, background process, or normal Godot project execution exists; Solaris does not open, import, execute, or run any Godot project (the recovery-mode project-probe surface — ADR 0009 — is restored as contracts, bounded static preparation, a one-time approval protocol with expiring single-use prepared probes, diagnostics, CLI reporting, and structural architecture enforcement of the fixed recovery invocation, but execution fails closed as unavailable on every platform: the disposable mirror and the recovery runner are fail-closed no-ops that never create, delete, or launch anything, because Node offers no exec-by-handle, no directory-relative create, and no delete-by-handle primitive), and Godot engine probing is itself intentionally unavailable at this stage; those remain deferred and, when added, must execute under the same enforcement.

## Git inspection

Core owns the Git-neutral contracts (`GitInspector` port, status/diff models, `GitError` categories) and knows nothing about processes or Git syntax. The Git adapter (`packages/adapters/src/git/cli`) owns exact invocation: a fixed allowlist of subcommands (`version`, `rev-parse`, `status`, `diff`), fixed argument arrays with no shell, a sanitized environment with Git safety variables, independent output bounds, timeouts, cancellation, and launch-time re-verification of the resolved executable (canonical identity, regular non-link file). **Git can only ever execute inside the sandbox backend under `validation-offline`** (network denied, writes limited to the exact private run directory, host reads limited to the repository root and trusted Git runtime roots, confined process tree — repository-configured helper code such as clean filters could only ever run inside that confinement, never on the host); the command-line overrides are defense in depth (the enumerable mechanisms are disabled; clean/smudge/process filters are not disabled from the command line and their only containment is the sandbox). When the backend cannot enforce the boundary, the adapter reports Git unavailable and never spawns Git. **Git inspection is unavailable at this stage**: the adapter requires verified Solaris-owned private run directories for every invocation, and run-directory creation and cleanup fail closed (no directory-relative or delete-by-handle primitive in Node), so nothing Git-related executes until that primitive exists. The adapter itself never spawns processes (enforced structurally by the architecture check; only the sandbox backend executes the Git process). It parses `--porcelain=v2 -z` status records and unified diff patches into the core models; providers receive only these structured results through the `git.status` and `git.diff` tools, and the CLI renders them without parsing raw Git output. The repository root must equal the workspace root; anything else is a structured failure, and non-Git workspaces remain fully supported.

## Recovery checkpoints

Core owns the checkpoint model, lifecycle rules, the `CheckpointStore` port, undo planning, and undo conflict rules; it never touches the filesystem. The filesystem checkpoint store (`packages/adapters/src/checkpoints/filesystem`) owns storage at `~/.solaris/checkpoints/<workspace-fingerprint>/<checkpoint-id>/`: atomic metadata replacement, preimage persistence, hash validation, symlink rejection, fail-closed retention limits (no automatic pruning: reaching the count or byte limit — or failing to prove capacity because any checkpoint's metadata or preimage is unreadable, invalid, oversized, linked, or size-inconsistent, or because content beyond the exact `metadata.json`/`preimage.bin` layout (unknown files, nested directories, temporary files, case-variant duplicates, links, special files) is present — refuses new checkpoints with a typed storage-limit error before any write and deletes nothing; the byte limit measures actual regular-file bytes beneath the checkpoint directory including metadata and preimages, declared preimages are content-verified through a shared handle-bound bounded verifier (exact bytes read must match the metadata byte length and SHA-256 via an explicit-offset read loop — a short read is never treated as EOF — with the opened handle and pathname identity proven against the pre-open lstat snapshot and a pre-read/final stability snapshot (identity, size, and mtime/ctime nanoseconds) captured from the handle before reading and re-verified after it, so a same-size corrupted preimage, a same-content hard-link/rename/symlink/junction substitution, or a same-inode in-place rewrite during verification is a refusal; `O_NOFOLLOW` is applied on POSIX while Windows carries the binding through the identity/stability comparisons, and unusable identity or stability fields fail closed), configured preimage limits are capped at 64 MiB with larger values rejected at store creation, checkpoint operations are bound to their before/after existence states (create: absent→present; update: present→present; delete: present→absent) by one shared validator for prepared and stored records, with the proposed checkpoint's exact serialized metadata and preimage bytes counted before any write, and unexpected content is never repaired, renamed, truncated, or quarantined; no checkpoint entry is skipped during retention; existing checkpoints are preserved), and the `reconcileWorkspaceCheckpoints` startup pass. **No new checkpoint is ever created at this stage** — every mutation fails closed as `unavailable` before recording — and `/checkpoints` may still list historical checkpoint data from earlier sessions if any exists. The undo service fails closed as `unavailable` (restoring requires pathname-based displacement, and Node offers no directory-relative primitive); the former reverse-diff/approval/restore machinery was largely deleted and the identity-bound design is documented as future work, while the store itself is tested internal code. The CLI's `/git-status`, `/diff`, `/checkpoints`, and `/undo` commands are thin renderers over these core-owned ports; the CLI never parses Git output and never restores files directly.
