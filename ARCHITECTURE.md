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
- The security model: `Capability`/`CapabilityPolicy`, built-in `SandboxProfile`s (`inspect`, `develop-offline`), the pure `evaluatePermission` function, the `SandboxBackend` port, classified `SandboxError` codes, and the `SolarisSecurity` facade (`evaluateCapability`, `checkSandbox`)

Core imports no Node infrastructure modules, no adapters, no CLI code, no terminal libraries, and no OS sandbox runtime. It never inspects the parent environment and never spawns processes. Architecture checks enforce all of this.

## Application layer

`createSolarisApplication({ provider, tools, maxToolRounds? })` returns the application: `sendPrompt(text, signal?)` streams `ApplicationEvent`s and `getStatus()` reports provider, state, and item count. State is private; only immutable views are exposed. The application owns conversation history, which providers must not.

The security facade (`createSolarisSecurity({ backend, policy, profile })`) sits beside the application in the composition root: `evaluateCapability` applies the pure permission evaluator, and `checkSandbox()` streams `sandbox_check_started` / `sandbox_check_completed` events from the backend's `inspect()`. The facade is consumed today by CLI diagnostics; future process and write tools will be gated through it before any backend call.

## Security model

- **Capability policy** (`CapabilityPolicy`) maps `workspace.read`, `workspace.write`, `process.execute`, and `network.outbound` to `allow` / `ask` / `deny`. Missing rules fail closed; explicit denies win; a profile can never broaden a denied policy; no built-in profile enables network.
- **Profiles**: `inspect` (read-only, no processes, no network — the default) and `develop-offline` (workspace writes and processes, network denied, protected metadata paths, minimal environment, timeouts, output limits).
- **Backend port** (`SandboxBackend`): `inspect()` reports truthful per-platform status and capabilities; `execute(request)` runs a trusted `SandboxedProcessRequest` (executable + arguments, never a raw shell string) and returns a bounded `SandboxedProcessResult` with violations; `close()` resets backend state and is idempotent. Errors normalize into `SandboxError` codes.
- **User configuration**: `~/.solaris/config.json` selects the profile and backend (defaults: `inspect`, `auto`). Unknown profiles/backends fail validation; project repositories cannot broaden these settings.
- **Conformance**: `npm run test:sandbox` runs fixed internal probes (workspace read/write, outside-write denial, secret denial, network denial, descendant confinement, output limits, timeout, cancellation) against the real backend using temporary directories and fake secrets. Unavailable backends are reported loudly and never treated as secure.

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

- The provider receives the conversation and the available tool definitions in every request.
- Assistant text, tool calls (`assistant_tool_call`), and tool results (`tool_result`) are stored as distinct `ConversationItem`s in chronological order. File contents stay classified as tool data.
- Tool activity surfaces as `tool_started`, `tool_completed`, `tool_failed`, and `tool_cancelled` application events with bounded display summaries.
- Unknown tools and duplicate call ids produce failed tool results without executing anything; the provider gets a subsequent turn to respond.
- A configurable maximum tool-round count (default 8, `DEFAULT_MAX_TOOL_ROUNDS`) stops the loop with a clear failure instead of an infinite loop.
- Cancellation flows from the application signal into the provider stream and each tool execution; later tool calls never start after cancellation, and no false completion is stored.

## Ports

External capabilities the application needs are narrow interfaces owned by core. The only port in this slice is `ModelProvider`, implemented as `stream(request): AsyncIterable<ModelEvent>` with an optional `AbortSignal`. No other ports exist yet; none are speculative.

## Adapters (`packages/adapters`)

Adapters implement core-owned ports. Providers, concrete tools, configuration, environment building, and the sandbox backend live here:

- `DeterministicFakeProvider` (id `deterministic-fake`): streams text responses in chunks, supports cancellation, and has synthetic tool scenarios (`list files`, `read README.md`, `search <text>`) that request registered tools and respond truthfully to their results. It never touches the filesystem or executes tools.
- Read-only workspace tools: `workspace.list`, `workspace.read`, `workspace.search`. All three share one canonical containment implementation (`resolveWorkspacePath`), the explicit exclusion list (`node_modules`, `.git`, `dist`, `coverage`), and the `WORKSPACE_LIMITS` output limits.
- User configuration (`loadUserConfig`): reads `~/.solaris/config.json`, defaults to `inspect`/`auto`, and rejects unknown profiles or backends.
- Child environments (`buildChildEnvironment`): allowlist-based construction with denied credential patterns; the only sanctioned way to build a child environment.
- `AnthropicSandboxRuntimeBackend`: the first concrete `SandboxBackend`, wrapping `@anthropic-ai/sandbox-runtime@0.0.70` (pinned exactly). It translates Solaris profiles into the runtime's configuration (empty network allowlists, workspace-rooted writes, protected paths, no weaker-isolation flags), reports truthful per-platform status (including Windows `setup-required`), enforces timeouts and output limits, collects violations, and normalizes errors. Only this module may import the runtime package.
- Conformance runner (`runSandboxConformance`): writes fixed fixture programs into a temporary workspace and executes them through the backend, reporting pass/fail per probe.

The workspace root is the canonicalized directory Solaris was launched from; it is stored privately by the tools and displayed in `/status`. Provider adapters never import sandbox, environment, or tool modules — the architecture check enforces that boundary.

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
- Adapters import only core contracts; adapter providers never import adapter tools, sandbox, or environment modules; sandbox adapters never import providers.
- The CLI imports core anywhere, and concrete adapters only in the composition root (tests may import adapters directly).
- `process.env` is never inspected in package source; child environments are built from an explicit allowlist.
- Direct `node:child_process` usage is limited to sandbox modules and test files.
- `npm run check:architecture` (see `scripts/check-architecture.mjs`) enforces these rules mechanically: prohibited imports, prohibited package dependencies, provider/sandbox isolation, process and environment boundaries, and workspace dependency cycles all fail the check.

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

The sandbox boundary, profiles, policy evaluator, environment filtering, and conformance suite exist, but no provider-accessible process or write tool does. The next milestone adds workspace-write tools and an explicit approval flow gated through the established policy and sandbox profiles. Shell execution and Godot execution remain deferred beyond that.
