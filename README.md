# Solaris

Solaris is an independent, provider-neutral, interactive agent harness for programming and developing games with the Godot Engine.

This repository currently contains the **foundation vertical slice**: an executable interactive CLI, a provider-neutral application core, and a deterministic fake provider. It does not yet develop Godot games.

## Current status

Working today:

- Interactive terminal session (`npm run solaris`)
- Slash commands: `/help`, `/status`, `/clear`, `/exit`
- Prompt submission with incrementally streamed responses
- In-process conversation history
- Cancellation support through `AbortSignal`
- Deterministic fake provider (`deterministic-fake`) that requires no credentials and no network

Not yet implemented:

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

| Command                      | Purpose                                                |
| ---------------------------- | ------------------------------------------------------ |
| `npm run build`              | Build all workspaces into their `dist/` directories    |
| `npm run clean`              | Remove all build output                                |
| `npm run format`             | Format the repository with Prettier (may modify files) |
| `npm run format:check`       | Verify formatting without modifying files              |
| `npm run lint`               | Lint with type-aware ESLint rules                      |
| `npm run typecheck`          | Type-check with strict TypeScript                      |
| `npm test`                   | Run all tests once (Vitest)                            |
| `npm run test:watch`         | Run tests in watch mode                                |
| `npm run check:architecture` | Verify workspace dependency boundaries                 |
| `npm run check`              | Run all non-mutating validation                        |
| `npm run solaris`            | Build and launch the interactive CLI                   |

## How to launch the CLI

```bash
npm run solaris
```

Example session:

```text
Solaris
Interactive Godot development harness
Provider: deterministic-fake

> hello

Solaris received: hello

> /status

Provider: deterministic-fake
Session: active
Messages: 2

> /help

Available commands:
  /help    Show this help
  /status  Show provider and session status
  /clear   Clear the terminal (conversation is kept)
  /exit    Close Solaris

> /exit
```

## Repository structure

```text
apps/
  cli/                     interactive terminal (input/output adapter)
    src/
      bootstrap/           composition root
      input/               slash-command parsing
packages/
  core/                    application behaviour and external contracts
    src/
      application/         in-memory conversation and prompt use case
      domain/              conversation model, cancellation classification
      ports/               provider contract
  adapters/                implementations of core-owned ports
    src/
      providers/           deterministic fake provider
docs/
  adr/                     architecture decision records
scripts/                   architecture checks
```

## Architecture summary

- `@solaris/core` owns application behaviour, conversation history, and the provider port. It imports no Node infrastructure, no adapters, and no UI code.
- `@solaris/adapters` implements the provider port. The only implementation is the deterministic fake provider.
- `@solaris/cli` is a terminal input/output adapter. It parses input, renders events, and composes dependencies in one composition root (`apps/cli/src/bootstrap/create-application.ts`). It owns no application behaviour.
- Dependency direction is inward: `CLI -> Core` and `CLI -> composition -> Adapters -> Core ports`. `npm run check:architecture` enforces this mechanically.
- See `ARCHITECTURE.md` and `docs/adr/0001-modular-monolith.md` for details.

## Testing and validation

```bash
npm run check
```

runs formatting, linting, type checking, tests, and the architecture check without modifying files. Before a change is considered complete, it must pass.

## Next planned milestone

Stage 2 of `ROADMAP.md`: the Godot script-development MVP (Godot project detection and understanding, GDScript-first development workflows). No work on that stage has begun.
