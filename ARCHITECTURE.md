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
- The security model: `Capability`/`CapabilityPolicy`, built-in `SandboxProfile`s (`inspect`, `develop-offline`, plus the internal `validation-offline` used only for commands), the pure `evaluatePermission` function, the `SandboxBackend` port, classified `SandboxError` codes, and the `SolarisSecurity` facade (`evaluateCapability`, `checkSandbox`)
- The provider-neutral development-command contracts: `CommandRunner` preparation/execution contracts, the immutable `CommandRunnerRegistry`, `COMMAND_LIMITS`, the canonical command digest, the opaque single-use `PreparedCommand`, the `PreparedCommandTool` contract for `process.run`, and the `CommandApplicationEvent`/audit model

Core imports no Node infrastructure modules, no adapters, no CLI code, no terminal libraries, and no OS sandbox runtime. It never inspects the parent environment and never spawns processes. Architecture checks enforce all of this.

## Application layer

`createSolarisApplication({ provider, tools, maxToolRounds? })` returns the application: `sendPrompt(text, signal?)` streams `ApplicationEvent`s and `getStatus()` reports provider, state, and item count. State is private; only immutable views are exposed. The application owns conversation history, which providers must not.

The security facade (`createSolarisSecurity({ backend, policy, profile })`) sits beside the application in the composition root: `evaluateCapability` applies the pure permission evaluator, and `checkSandbox()` streams `sandbox_check_started` / `sandbox_check_completed` events from the backend's `inspect()`. The facade is consumed today by CLI diagnostics; future process and write tools will be gated through it before any backend call.

## Security model

- **Capability policy** (`CapabilityPolicy`) maps `workspace.read`, `workspace.write`, `process.execute`, and `network.outbound` to `allow` / `ask` / `deny`. Missing rules fail closed; explicit denies win; a profile can never broaden a denied policy; no built-in profile enables network.
- **Profiles**: `inspect` (read-only, no processes, no network — the default), `develop-offline` (workspace writes and process execution both require one-time approval, network denied, protected metadata paths, minimal environment, timeouts, output limits), and `validation-offline` (internal: commands always run with a read-only workspace regardless of the user profile).
- **Backend port** (`SandboxBackend`): `inspect()` reports truthful per-platform status and capabilities; `execute(request)` runs a trusted `SandboxedProcessRequest` (executable + arguments, never a raw shell string; optional bounded output streaming, explicit timeout, and per-stream hard limits) and returns a bounded `SandboxedProcessResult` with violations; `close()` resets backend state and is idempotent. Errors normalize into `SandboxError` codes.
- **User configuration**: `~/.solaris/config.json` selects the profile and backend (defaults: `inspect`, `auto`). Unknown profiles/backends fail validation; project repositories cannot broaden these settings; `validation-offline` is not selectable.
- **Conformance**: `npm run test:sandbox` runs fixed internal probes (workspace read/write, outside-write denial, secret denial, network denial, descendant confinement, output limits, timeout, cancellation) plus validation-command probes (read-only workspace enforcement for root/child/grandchild/npm scripts, network and loopback denial, credential and `NODE_OPTIONS` absence, disabled npm pre/post hooks, closed stdin, output-limit termination, descendant termination on timeout and cancellation, no workspace artifacts, run-directory cleanup) against the real backend using temporary directories and fake secrets. Unavailable backends are reported loudly and never treated as secure.

## Command execution

`process.run` is a `PreparedCommandTool` owned by adapters, built on core contracts and ports:

```text
Provider requests a development command
   ↓
Runner validates structured input (node-script available; npm-script
fails closed as unavailable)
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
Creates a sandbox-private run directory (no-follow verified, exclusive
creation, canonical re-verification), acquires the shared mutation
lock, records Git status
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
Git status compared; run directory removed; lock released
```

- Runners are prepared/executed through the immutable runner registry; the concrete plans are opaque single-use objects that only their creating runner can translate back into an executable request (revalidated and rehashed). The digest covers runner id, executable identity and version, script path and complete SHA-256, arguments, working directory, profile, environment policy, timeout, output limits, stdin and network policy.
- Command execution never spawns a process from core, the CLI, providers, or the runners; only the sandbox adapter (and the Git adapter, for inspection) uses process APIs, and the architecture check rejects `shell: true`/`exec`/`execSync`/`spawnSync` in runtime code.
- Each run lives under `~/.solaris/runs/<workspace-fingerprint>/<run-id>/` with private `home/`, `tmp/`, `npm-cache/`, and a script cache; every path component is verified with no-follow semantics before anything is created beneath it, components are created exclusively, the runs root must be outside the workspace, and the sandbox is granted exactly the current run directory. Cleanup re-verifies containment immediately before deletion, is link-safe, and preserves the directory when a safe state cannot be proven.
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

Write tools (`workspace.create_file`, `workspace.edit_file`, `workspace.delete_file`) are `PreparedMutationTool`s registered alongside the immediate read-only tools:

1. The provider requests a write tool; the application evaluates the capability policy (`workspace.write` is `ask` under `develop-offline`, `deny` under `inspect`).
2. The tool `prepare`s the change in memory: validates input, resolves the target with component-aware path safety, checks protected paths, reads and hashes the current file, applies exact text replacements (for edits), and builds a complete deterministic bounded unified diff.
3. Under `ask`, the application calls the core-owned `ApprovalReviewer` with the bounded preview. The CLI implements the interactive reviewer; denial is the default; EOF, reviewer failure, timeout, and cancellation all prevent mutation; decisions are one-time.
4. On approval, the tool `apply`s exactly once: re-acquires the serialized mutation lock, revalidates the path, file type, symlink state, and hash, stages content in an exclusive temp file (updates), enters a short non-cancellable commit section, replaces or deletes the target, verifies the final bytes and hash, and cleans up temp artifacts.
5. Conflicts (stale hash, disappeared or appeared target, symlink races, replaced parents) return `conflict` without touching the changed external state; a revised proposal requires a new approval.

Prepared mutations are opaque single-use objects; core never sees their contents and the tools never accept another tool's mutation. See `SECURITY.md` and ADR 0005 for the full model.

Prepared command tools follow the same pattern: `prepare` (validate + build the immutable plan), approval (the exact preview and digest), then `executePrepared` exactly once with the approved digest; the tool's payload map makes reuse impossible and the runner's revalidation makes changed plans conflict. See "Command execution" above.

## Ports

External capabilities the application needs are narrow interfaces owned by core: the `ModelProvider` port (`stream(request): AsyncIterable<ModelEvent>` with an optional `AbortSignal`), the `SandboxBackend` port, the `ApprovalReviewer` port, the `GitInspector` port, the `CheckpointStore` port, the `UndoService` port, and the `CommandDigestService` port (hashing is injected so core stays free of Node imports). The command tool consumes the sandbox, approval, git, lock, runner-registry, and run-directory ports; runners are core contracts implemented in adapters.

## Adapters (`packages/adapters`)

Adapters implement core-owned ports. Providers, concrete tools, configuration, environment building, and the sandbox backend live here:

- `DeterministicFakeProvider` (id `deterministic-fake`): streams text responses in chunks, supports cancellation, and has synthetic tool scenarios (`list files`, `read README.md`, `search <text>`) that request registered tools and respond truthfully to their results. It never touches the filesystem or executes tools.
- Read-only workspace tools: `workspace.list`, `workspace.read`, `workspace.search`. All three share one canonical containment implementation (`resolveWorkspacePath`), the explicit exclusion list (`node_modules`, `.git`, `dist`, `coverage`), and the `WORKSPACE_LIMITS` output limits. `workspace.read` returns the complete-file SHA-256.
- Approved mutation tools: `workspace.create_file`, `workspace.edit_file`, `workspace.delete_file`. They share the write-path safety and protected-path enforcement (`mutation-paths.ts`), the serialized mutation lock, exclusive temp-file staging, hash-based conflict detection, deterministic bounded diffs (`diff.ts` on the `diff` package), and post-write verification. Every replacement and deletion commit is identity-bound through `safe-replacement.ts`: the target is displaced to a same-directory quarantine, the displaced object is hash-verified against the approved state, and only then is the commit rename or unlink performed — on every platform. Only these modules may call direct write APIs.
- User configuration (`loadUserConfig`): reads `~/.solaris/config.json`, defaults to `inspect`/`auto`, and rejects unknown profiles or backends.
- Child environments (`buildChildEnvironment`, `buildCommandEnvironment`): allowlist-based construction with denied credential patterns and fixed safe command values; the only sanctioned way to build a child environment.
- Command layer (`src/process`): trusted Node CLI resolution (`resolveTrustedNode` — npm CLI resolution remains for the conformance suite, but the `npm-script` runner fails closed as unavailable because npm's execution cannot be bound to the approved package bytes under the pinned runtime), the `node-script` runner (structured validation, file hashing, digest computation, full revalidation, and staging of the exact approved script bytes into the run's private script cache with hash verification — the child executes the immutable private copy, never the mutable workspace path), the sandbox-private run-directory provider (every path component verified with no-follow semantics before creation, exclusive creation, canonical re-verification before use, runs root outside the workspace, cleanup that re-verifies immediately before deletion and never traverses links), and the `process.run` tool that executes approved plans through the `SandboxBackend` under `validation-offline`, granting the sandbox exactly the current run directory. No module in this layer spawns processes.
- `AnthropicSandboxRuntimeBackend`: the first concrete `SandboxBackend`, wrapping `@anthropic-ai/sandbox-runtime@0.0.70` (pinned exactly). Only this module may import the runtime package. It enforces a deny-by-default host-read allowlist (deny `/` and re-allow the workspace, the current run directory, the trusted runner executables, and the minimum system runtime paths on Linux/macOS; refused as unavailable on Windows), gives every request its own per-execution filesystem/network configuration so no request can inherit a broader earlier profile, resets and reinitializes the shared manager when the effective configuration changes, streams bounded decoded output accounted on raw bytes with the hard limit enforced inside the crossing chunk, terminates the process tree on timeout/cancellation/output-limit, runs cleanup in `finally` on every path, and isolates failing output callbacks.
- Conformance runner (`runSandboxConformance`): writes fixed fixture programs into a temporary workspace and executes them through the backend, reporting pass/fail per probe. Host-read probes use existing regular files in representative unapproved locations selected independently of the deny surface, plus cross-run isolation and bidirectional profile-isolation probes.

The workspace root is the canonicalized directory Solaris was launched from; it is stored privately by the tools and displayed in `/status`. Provider adapters never import sandbox, environment, tool, checkpoint, git, or process modules — the architecture check enforces that boundary.

## CLI (`apps/cli`)

The CLI is an input/output adapter:

- Reads terminal input and renders terminal output through a small `SessionIO` interface
- Parses slash commands (`/help`, `/status`, `/clear`, `/exit`) in a pure module separate from rendering
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
- Direct `node:child_process` usage is limited to sandbox and git modules and test files (the `node:` prefix and the bare `child_process` spelling normalize to one rule); command runners never spawn processes.
- Raw process execution (`shell: true`, `exec(`, `execSync(`, `spawnSync(`) is prohibited in runtime code, with documented exemptions only for test fixtures and the embedded conformance probe sources; the checks are structural (TypeScript parsing of imports, re-exports, static dynamic imports, aliases, and call sites) with textual fallbacks for constructs parsing cannot represent.
- Destructive filesystem APIs (`writeFile`, `unlink`, `rename`, `appendFile`, `createWriteStream`, and friends, including aliased and namespace forms) are limited to the workspace mutation modules, the conformance runner, the process adapter (run directories), and tests.
- Git mutation commands are rejected in runtime code both as string tokens and structurally in spawn argument lists.
- `npm run check:architecture` (see `scripts/check-architecture.mjs`) enforces these rules mechanically: prohibited imports, prohibited package dependencies, provider/sandbox isolation, process, environment, and write boundaries, and workspace dependency cycles all fail the check. It is a developer guardrail, not an OS security boundary; runtime-constructed module specifiers and string contents are documented limitations.

## Why a modular monolith

One process with explicit module boundaries is the smallest structure that keeps UI, application logic, and infrastructure separable without introducing distributed orchestration, message buses, or deployment complexity. Solaris can grow its later stages inside this boundary and can extract packages later if a real need appears.

## Why the fake provider is an adapter

Provider neutrality is a stated product requirement, so the provider contract belongs in core and concrete implementations live behind it. The fake provider exercises the port end to end without credentials or network access, which keeps development and tests self-contained.

## Why UI code does not own application behaviour

The application must remain usable without a terminal (headless tests, future Godot-facing surfaces). Conversation policy and state transitions live in core; the terminal only translates between user intent and application events.

## Deferred: Godot integration

No Godot integration exists in this slice: no Godot executable or project detection, no GDScript handling, no editor or runtime bridges. When it arrives it will live behind new core-owned ports as adapters.

## Deferred: persistence

Sessions are in-memory only. No SQLite, transcript storage, or session restoration exists. A persistence port will be added only when a real requirement demands it.

## Deferred: multi-agent functionality

The current workflow is one interactive primary agent. Multi-agent review, agents as a product concept, and `/evolve` are out of scope for the foundation stage and are not modelled in core.

## Deferred: process and write tools

The sandbox boundary, profiles, policy evaluator, environment filtering, and conformance suite exist; approved single-file workspace mutations execute through them (identity-bound quarantine commits); read-only Git inspection (`git.status`, `git.diff` through a trusted allowlisted adapter) and Solaris-owned recovery checkpoints with safe undo are complete; and sandboxed validation-command execution (`process.run` with the `node-script` runner — the `npm-script` runner fails closed as unavailable because npm's execution cannot be bound to the approved package bytes under the pinned runtime) is complete: structured arguments only, immutable private script execution, read-only workspace, denied network, minimal environment, closed stdin, bounded streamed output, digest-bound one-time approval, timeouts, and process-tree cancellation. No general shell, arbitrary executable runner, writable command execution, package installation, interactive stdin, background process, or Godot execution exists; those remain deferred and, when added, must execute under the same enforcement.

## Git inspection

Core owns the Git-neutral contracts (`GitInspector` port, status/diff models, `GitError` categories) and knows nothing about processes or Git syntax. The Git adapter (`packages/adapters/src/git/cli`) owns exact invocation: a fixed allowlist of subcommands (`version`, `rev-parse`, `status`, `diff`, `check-ignore`), fixed argument arrays with no shell, a sanitized environment with Git safety variables, independent output bounds, timeouts, and cancellation. It parses `--porcelain=v2 -z` status records and unified diff patches into the core models; providers receive only these structured results through the `git.status` and `git.diff` tools, and the CLI renders them without parsing raw Git output. The repository root must equal the workspace root; anything else is a structured failure, and non-Git workspaces remain fully supported.

## Recovery checkpoints

Core owns the checkpoint model, lifecycle rules, the `CheckpointStore` port, undo planning, and undo conflict rules; it never touches the filesystem. The filesystem checkpoint store (`packages/adapters/src/checkpoints/filesystem`) owns storage at `~/.solaris/checkpoints/<workspace-fingerprint>/<checkpoint-id>/`: atomic metadata replacement, preimage persistence, hash validation, symlink rejection, bounded retention with fail-closed pruning, and the `reconcileWorkspaceCheckpoints` startup pass. The undo service reuses the mutation lock, path safety, protected-path checks, diff generation, and the approval reviewer to restore only Solaris-owned changes after an exact post-state hash match. The CLI's `/git-status`, `/diff`, `/checkpoints`, and `/undo` commands are thin renderers over these core-owned ports; the CLI never parses Git output and never restores files directly.
