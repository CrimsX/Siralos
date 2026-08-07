# Solaris

Solaris is an independent, provider-neutral, interactive agent harness for programming and developing games with the Godot Engine.

This repository currently contains the **foundation vertical slice**: an executable interactive CLI, a provider-neutral application core, and a deterministic fake provider. It does not yet develop Godot games.

## Current status

Working today:

- Interactive terminal session (`npm run solaris`)
- Slash commands: `/help`, `/status`, `/clear`, `/tools`, `/sandbox`, `/permissions`, `/exit`
- Prompt submission with incrementally streamed responses
- A bounded provider/tool loop: the provider can request read-only workspace tools, results are added to the conversation, and the provider produces a final response
- Read-only workspace tools: `workspace.list`, `workspace.read`, `workspace.search` — all paths are canonicalized and contained within the launch directory; symlink escapes, binary files, oversized files, and traversal limits are enforced
- A sandbox and permission foundation: capability policy, built-in `inspect` and `develop-offline` profiles, a pure permission evaluator, an Anthropic Sandbox Runtime backend behind a core-owned port, allowlist-based child environments, fixed conformance probes (`npm run test:sandbox`), `/sandbox` and `/permissions` diagnostics, and a `--sandbox-doctor` CLI command
- In-process conversation history
- Cancellation support through `AbortSignal`
- Deterministic fake provider (`deterministic-fake`) that requires no credentials and no network, with synthetic tool-call scenarios (`list files`, `read README.md`, `search <text>`)

Not yet implemented:

- Any file modification, patching, or deletion through tools
- Shell or Git command execution — no provider-accessible process or write tool exists yet
- Godot project understanding, GDScript programming, or editor/runtime integration
- Real model providers (e.g. Anthropic, OpenAI)
- Persistent sessions or transcript storage
- Multi-agent functionality, skills, or agent profiles
- `/evolve` self-improvement workflows

Solaris is at the foundation stage. Do not expect it to develop games yet.

## Prerequisites

- Node.js 24 LTS (see `.nvmrc`; `nvm install && nvm use`)
- npm (the lockfile is generated with npm 11.13.0; the exact version is recorded in `packageManager`)

## Installation

```bash
npm install
```

This installs tooling at the root and links the workspace packages (`@solaris/core`, `@solaris/adapters`, `@solaris/cli`).

## Development setup

```bash
npm run build        # compile all workspaces with tsc -b
npm run solaris      # launch the interactive CLI
```

## npm commands

| Command                               | Purpose                                                                            |
| ------------------------------------- | ---------------------------------------------------------------------------------- |
| `npm run build`                       | Build all workspaces into their `dist/` directories                                |
| `npm run clean`                       | Remove all build output                                                            |
| `npm run format`                      | Format the repository with Prettier (may modify files)                             |
| `npm run format:check`                | Verify formatting without modifying files                                          |
| `npm run lint`                        | Lint with type-aware ESLint rules                                                  |
| `npm run typecheck`                   | Type-check with strict TypeScript                                                  |
| `npm test`                            | Run all tests once (Vitest)                                                        |
| `npm run test:watch`                  | Run tests in watch mode                                                            |
| `npm run check:architecture`          | Verify workspace dependency boundaries                                             |
| `npm run test:sandbox`                | Run live sandbox conformance probes (skips loudly when the backend is unavailable) |
| `npm run check`                       | Run all non-mutating validation                                                    |
| `npm run solaris`                     | Build and launch the interactive CLI                                               |
| `npm run solaris -- --sandbox-doctor` | Print sandbox diagnostics (add `--run-probes` to run fixed probes)                 |

## Sandbox configuration

User-level configuration lives at `~/.solaris/config.json`:

```json
{
  "sandbox": {
    "profile": "inspect",
    "backend": "auto"
  }
}
```

Supported profiles: `inspect` (default; read-only, no processes, no network) and `develop-offline` (workspace writes and processes allowed, network denied). Backends: `auto` and `anthropic-runtime`. An untrusted repository can never broaden these settings. See `SECURITY.md` for the full security model.

## How to launch the CLI

```bash
npm run solaris
```

Example session:

```text
Solaris
Interactive Godot development harness
Provider: deterministic-fake

> list files

● workspace.list {"path":"."}
  19 entries

Solaris inspected 19 workspace entries.

> /tools

Available tools:
  workspace.list - List one directory within the approved workspace. (read-only)
  workspace.read - Read a bounded range from one text file inside the workspace. (read-only)
  workspace.search - Search text files recursively within a bounded workspace directory. (read-only)

> /status

Provider: deterministic-fake
Session: active
Messages: 4
Workspace: C:\Users\...\Solaris
Tools: 3

> /exit
```

## Repository structure

```text
apps/
  cli/                     interactive terminal (input/output adapter)
    src/
      bootstrap/           composition root, sandbox doctor
      input/               slash-command parsing
packages/
  core/                    application behaviour and external contracts
    src/
      application/         in-memory conversation, provider/tool loop
      domain/              conversation items, JSON types, cancellation
      ports/               provider contract
      security/            capability policy, sandbox profiles, backend port
      tools/               tool contracts and the tool registry
  adapters/                implementations of core-owned ports
    src/
      config/              trusted user-level configuration
      environment/         child-environment allowlist builder
      providers/           deterministic fake provider (with tool scenarios)
      sandbox/             Anthropic Sandbox Runtime backend, conformance probes
      tools/workspace/     read-only workspace tools (list, read, search)
docs/
  adr/                     architecture decision records
schemas/                   user configuration JSON Schema
scripts/
  sandbox/                 live conformance runner
scripts/                   architecture checks
```

## Architecture summary

- `@solaris/core` owns application behaviour, conversation history, the provider port, and — since the sandbox milestone — the security model: capability policy, built-in sandbox profiles, the pure permission evaluator, the `SandboxBackend` port, and classified sandbox errors. It imports no Node infrastructure, no adapters, no UI code, and no sandbox runtime.
- `@solaris/adapters` implements ports: the deterministic fake provider, the read-only workspace tools, the allowlist child-environment builder, and the Anthropic Sandbox Runtime backend (pinned exactly at `0.0.70`). Only the sandbox adapter module may import the runtime package.
- `@solaris/cli` is a terminal input/output adapter. It parses input, renders events, and composes dependencies in one composition root. `/sandbox` and `/permissions` are diagnostics; the sandbox doctor is a startup-mode report.
- Dependency direction is inward: `CLI -> Core` and `CLI -> composition -> Adapters -> Core ports`. `npm run check:architecture` enforces this mechanically, including process and sandbox boundaries.
- See `ARCHITECTURE.md`, `SECURITY.md`, and the ADRs in `docs/adr/` for details.

## Testing and validation

```bash
npm run check
```

runs formatting, linting, type checking, tests, and the architecture check without modifying files. Before a change is considered complete, it must pass.

## Next planned milestone

The next narrow task is to add workspace-write tools and an explicit approval flow using the established sandbox and capability policy. See `ROADMAP.md`.
