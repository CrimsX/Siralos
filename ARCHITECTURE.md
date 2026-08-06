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

- Conversation model (`ConversationMessage`, `ConversationRole`)
- Provider request/event contracts and the `ModelProvider` port
- Application events (`response_started`, `text_delta`, `response_completed`, `response_cancelled`, `response_failed`)
- In-memory conversation history, kept private behind the `SolarisApplication` API
- Prompt submission: stores the user message, sends a snapshot of the full history to the provider, accumulates streamed text, and stores the assistant message only after successful completion
- Cancellation and failure normalization (a cancellation is never reported as completion)

Core imports no Node infrastructure modules, no adapters, no CLI code, and no terminal libraries. It is platform-neutral TypeScript.

## Application layer

`createSolarisApplication({ provider, tools, maxToolRounds? })` returns the application: `sendPrompt(text, signal?)` streams `ApplicationEvent`s and `getStatus()` reports provider, state, and item count. State is private; only immutable views are exposed. The application owns conversation history, which providers must not.

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

Adapters implement core-owned ports. Providers and concrete tools live here:

- `DeterministicFakeProvider` (id `deterministic-fake`): streams text responses in chunks, supports cancellation, and has synthetic tool scenarios (`list files`, `read README.md`, `search <text>`) that request registered tools and respond truthfully to their results. It never touches the filesystem or executes tools.
- Read-only workspace tools: `workspace.list`, `workspace.read`, `workspace.search`. All three share one canonical containment implementation (`resolveWorkspacePath`), the explicit exclusion list (`node_modules`, `.git`, `dist`, `coverage`), and the `WORKSPACE_LIMITS` output limits. They resolve every path against the workspace root, reject escapes, skip binary/oversized content, and return workspace-relative paths only.

The workspace root is the canonicalized directory Solaris was launched from (`resolveWorkspaceRoot`); it is stored privately by the tools and displayed in `/status`. The fake provider never imports concrete tools — the architecture check enforces that boundary.

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
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const workspaceTools = [
    createWorkspaceListTool(workspaceRoot),
    createWorkspaceReadTool(workspaceRoot),
    createWorkspaceSearchTool(workspaceRoot),
  ];
  const provider = createDeterministicFakeProvider();
  const application = createSolarisApplication({
    provider,
    tools: createToolRegistry(workspaceTools),
  });
  // ...
}
```

No dependency-injection container, service locator, or reflection. The registry rejects duplicate tool names at startup and never mutates afterwards.

## Current dependency direction

```text
CLI ───────────────→ Core
 │
 └── composition ─→ Adapters ─→ Core ports
```

- Core imports nothing from the workspace.
- Adapters import only core contracts; adapter providers never import adapter tools.
- The CLI imports core anywhere, and concrete adapters only in the composition root.
- `npm run check:architecture` (see `scripts/check-architecture.mjs`) enforces these rules mechanically: prohibited imports, prohibited package dependencies, provider/tool isolation, and workspace dependency cycles all fail the check.

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
