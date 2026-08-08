# Solaris

Solaris is an independent, provider-neutral, interactive agent harness for programming and developing games with the Godot Engine.

This repository currently contains the **foundation vertical slice**: an executable interactive CLI, a provider-neutral application core, and a deterministic fake provider. It does not yet develop Godot games.

## Current status

Working today:

- Interactive terminal session (`npm run solaris`)
- Slash commands: `/help`, `/status`, `/clear`, `/tools`, `/sandbox`, `/permissions`, `/git-status`, `/diff`, `/checkpoints`, `/undo`, `/commands`, `/cancel`, `/exit`, `/godot`, `/godot-installations`, `/godot-project`, `/godot-doctor`
- Prompt submission with incrementally streamed responses
- A bounded provider/tool loop with approved workspace mutations: `workspace.create_file`, `workspace.edit_file` (exact text replacements), and `workspace.delete_file` — each gated by capability policy, a complete reviewable diff, and one-time user approval, with SHA-256 conflict detection, mutation serialization, and post-write verification. Every approved mutation first durably records a Solaris-owned recovery checkpoint (exact pre-change bytes), reconciled at startup after crashes.
- Read-only workspace tools: `workspace.list`, `workspace.read` (with complete-file SHA-256), `workspace.search` — all paths are canonicalized and contained within the launch directory
- Read-only Git inspection (`git.status`, `git.diff`) through a trusted, allowlisted Git adapter — fixed argument arrays, no shell, and every executable Git helper disabled mechanically (fsmonitor, aliases, pagers, external diff, textconv, credential helpers, prompts), repository-redirecting and config-injecting environment variables stripped at the process boundary, bounded byte-counted output with a streaming UTF-8 decoder, timeouts, cancellation; structured summaries come from NUL-delimited machine-readable data with exact paths; the repository root must equal the workspace root
- Safe user-invoked undo (`/undo`) that restores only Solaris-owned changes with a complete reverse diff, one-time approval, and exact post-state hash validation rechecked immediately before the destructive commit; user changes after a Solaris mutation cause a conflict, never an overwrite
- A sandbox and permission foundation: capability policy, built-in `inspect` and `develop-offline` profiles, a pure permission evaluator, an Anthropic Sandbox Runtime backend behind a core-owned port with an enforced host-read allowlist (deny-root with re-allow on Linux/macOS; reported unavailable and refused on Windows), allowlist-based child environments with the wrapper's runtime-required environment merged under strict rules, fixed conformance probes (`npm run test:sandbox`), `/sandbox` and `/permissions` diagnostics, and a `--sandbox-doctor` CLI command with trustworthy exit codes (0 passed, 1 probe failure, 3 probes unavailable)
- The sandboxed development-command surface (`process.run`): structured arguments only (never a provider-supplied shell string), read-only workspace, denied network, a minimal sanitized environment, closed stdin, bounded streamed output, bounded timeouts, process-tree cancellation, and digest-bound one-time approval under the internal `validation-offline` profile. No command can execute at this stage: both the `node-script` and `npm-script` runners fail closed as `unavailable` — the pinned Node runtime cannot mechanically bind execution to the approved script bytes, because the script can reach internal surfaces such as `process.binding` (e.g. `spawn_sync`) to spawn an unconstrained interpreter and the staged private copy can be substituted by a same-user process in the verify-to-launch window — so `isAvailable()` returns false for both and every request is refused before any approval.
- Godot executable discovery and engine profiling, before any project execution: trusted user-configured installations (absolute paths with optional edition hints) plus fixed-name PATH search — no broad filesystem scanning; exact executable fingerprints (canonical path, size, mtime, SHA-256) with the complete SHA-256 recomputed before every probe and execution bound to a verified private executable copy; project-independent probes (`--version`, `--help`, `--dump-extension-api`, fixed tuples built by a single private constructor) through the sandbox backend under the internal `godot-probe-offline` profile, which excludes the workspace from readable roots and requires the host-read boundary; adversarial version parsing with release channels preserved; conservative edition classification; deterministic selection ranking with recorded rationale (explicit selection never falls back silently); and an engine-profile cache at `~/.solaris/godot/engine-profiles` (bounded, atomic, symlink-rejected, fully field-validated, invalidated by full executable hash revalidation)
- Static Godot project detection and profiling: only the root `project.godot` is read (regular file, symlinks rejected, never parents/children), everything parsed conservatively and never evaluated, plus an executable-content inventory (tool scripts, editor plugins, GDExtension descriptors, autoloads, C# project files) that never loads or runs anything
- Godot provider tools and CLI surface: `godot.inspect_engine` and `godot.inspect_project` (allow in every built-in policy, no one-time approval), `/godot`, `/godot-installations`, `/godot-project`, `/godot-doctor`, the `--godot-path` / `--godot-installation` / `--godot-doctor` startup flags, and the `SOLARIS_GODOT` / `SOLARIS_GODOT_INSTALLATION` environment overrides
- In-process conversation history
- Cancellation support through `AbortSignal`
- Deterministic fake provider (`deterministic-fake`) that requires no credentials and no network, with synthetic scenarios for read tools, the approved write workflow (`create solaris-write-test`, `edit solaris-write-test`, `delete solaris-write-test`), Git inspection (`git status`, `show working diff`, `show staged diff`, `show head diff`), and development commands (`run npm check`, `run npm test`, `run node validation fixture`)

Not yet implemented:

- General shell access, arbitrary executables, writable command execution, package installation, or background processes — no command can execute at this stage: both the `node-script` and `npm-script` runners fail closed as `unavailable` (workspace-read-only and offline by design), so every `process.run` request is refused with an explanation
- Git writes of any kind: staging, commits, reset, restore, checkout, clean, stash, branches, worktrees, remotes
- Godot project execution: Solaris does not open, import, execute, or run any Godot project. GDScript programming and editor/runtime integration remain unimplemented
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

| Command                               | Purpose                                                                                                                                                          |
| ------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `npm run build`                       | Build all workspaces into their `dist/` directories                                                                                                              |
| `npm run clean`                       | Remove all build output                                                                                                                                          |
| `npm run format`                      | Format the repository with Prettier (may modify files)                                                                                                           |
| `npm run format:check`                | Verify formatting without modifying files                                                                                                                        |
| `npm run lint`                        | Lint with type-aware ESLint rules                                                                                                                                |
| `npm run typecheck`                   | Type-check with strict TypeScript                                                                                                                                |
| `npm test`                            | Run all tests once (Vitest)                                                                                                                                      |
| `npm run test:watch`                  | Run tests in watch mode                                                                                                                                          |
| `npm run check:architecture`          | Verify workspace dependency boundaries (structural parsing, not regex-only)                                                                                      |
| `npm run test:sandbox`                | Run live sandbox conformance probes (skips loudly when the backend is unavailable)                                                                               |
| `npm run test:godot`                  | Run live Godot probe conformance (opt-in; requires `SOLARIS_TEST_GODOT="<absolute-path>"`; skips loudly when unset or the backend cannot enforce the boundaries) |
| `npm run check`                       | Run all non-mutating validation                                                                                                                                  |
| `npm run solaris`                     | Build and launch the interactive CLI                                                                                                                             |
| `npm run solaris -- --sandbox-doctor` | Print sandbox diagnostics (add `--run-probes` to run fixed probes; exit 0 passed, 1 probe failure, 3 probes unavailable)                                         |
| `npm run solaris -- --godot-doctor`   | Print Godot discovery, selection, and cache diagnostics                                                                                                          |

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

Supported profiles: `inspect` (default; read-only, no processes, no network — write and process tools are not exposed to the provider) and `develop-offline` (workspace writes require one-time approval, processes require one-time approval per exact command plan, network denied). Backends: `auto` and `anthropic-runtime`. An untrusted repository can never broaden these settings. Commands would run under the internal `validation-offline` profile: the project workspace is readable but never writable (no command can currently execute — both runners fail closed as unavailable; see below). Godot engine probes execute under the internal `godot-probe-offline` profile: the workspace is excluded from readable roots. See `SECURITY.md` for the full security model.

## Command execution status

No command can execute at this stage. Both the `node-script` and `npm-script`
runners report `unavailable` for every request (including the fake provider's
`run npm check` / `run npm test` / `run node validation fixture` scenarios),
and `/commands` shows both as unavailable. The pinned Node runtime cannot
mechanically bind execution to the approved script bytes: the script can reach
internal surfaces such as `process.binding` (e.g. `spawn_sync`) to spawn an
unconstrained interpreter, and the staged private copy can be substituted by a
same-user process in the verify-to-launch window — so Solaris refuses instead
of claiming exact approval. The command surface itself remains in place:
`process.run` with structured arguments only (never a provider-supplied shell
string), read-only workspace, denied network, minimal environment, closed
stdin, bounded streamed output, bounded timeouts, process-tree cancellation,
digest-bound one-time approval under the internal `validation-offline`
profile, and the `/commands` and `/cancel` commands.

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
      approval/            interactive one-time approval reviewer
packages/
  core/                    application behaviour and external contracts
    src/
      application/         in-memory conversation, provider/tool loop
      commands/            command-runner contracts, registry, limits, digest,
                           prepared-command tool contract, command events
      domain/              conversation items, JSON types, cancellation
      ports/               provider contract
      security/            capability policy, sandbox profiles, approval port,
                           backend port
      tools/               tool contracts, registry, prepared mutations
  adapters/                implementations of core-owned ports
    src/
      config/              trusted user-level configuration
      environment/         child-environment allowlist builder
      process/             process.run tool, run-directory provider, trusted
                           Node/npm resolution, npm-script and node-script
                           runners
      providers/           deterministic fake provider (with tool scenarios)
      sandbox/             Anthropic Sandbox Runtime backend, conformance probes
      godot/               discovery, profiling, static project inspection
      tools/workspace/     read-only workspace tools + approved mutation tools
docs/
  adr/                     architecture decision records
schemas/                   user configuration JSON Schema
scripts/
  sandbox/                 live conformance runner
scripts/                   architecture checks, validation fixture
```

## Architecture summary

- `@solaris/core` owns application behaviour, conversation history, the provider port, the security model (capability policy, built-in sandbox profiles, the pure permission evaluator, the `SandboxBackend` port, classified errors), the Git-neutral inspection contracts (`GitInspector`, status/diff models, error categories), the checkpoint model (metadata, lifecycle, the `CheckpointStore` port, undo planning and conflict rules), and the provider-neutral development-command contracts (runner contracts, the immutable runner registry, command limits, the deterministic command digest, the opaque single-use prepared command, and the `PreparedCommandTool` contract). It imports no Node infrastructure, no adapters, no UI code, and no sandbox runtime.
- `@solaris/adapters` implements ports: the deterministic fake provider, the read-only workspace tools, the approved mutation tools, the allowlist child-environment builder, the trusted Git CLI adapter (fixed allowlisted subcommands, no shell, sanitized environment, bounded output), the durable filesystem checkpoint store, the safe undo service, and the command layer — trusted Node/npm CLI resolution, the `npm-script` and `node-script` runners (both fail closed as unavailable, so no command can execute at this stage), the sandbox-private run-directory provider, and the `process.run` tool that would execute approved plans through the `SandboxBackend` under the `validation-offline` profile. Only the sandbox adapter module may import the runtime package; only the Git adapter spawns processes directly.
- `@solaris/cli` is a terminal input/output adapter. It parses input, renders events, reviews approvals interactively, and composes dependencies in one composition root. `/commands` and `/cancel` are CLI capabilities; the CLI never spawns commands and never renders sandbox-private paths.
- Dependency direction is inward: `CLI -> Core` and `CLI -> composition -> Adapters -> Core ports`. `npm run check:architecture` enforces this mechanically, including process, Git, checkpoint, and sandbox boundaries and the absence of raw process execution (`shell: true`, `exec`, `execSync`, `spawnSync`) in runtime code.
- See `ARCHITECTURE.md`, `SECURITY.md`, and the ADRs in `docs/adr/` for details.

## Testing and validation

```bash
npm run check
```

runs formatting, linting, type checking, tests, and the architecture check without modifying files. Before a change is considered complete, it must pass.

## Next planned milestone

Godot executable discovery, exact-version profiling, static project detection, and read-only engine capability probes are complete. The next narrow task is to add version-matched Godot knowledge profiles and GDScript language intelligence — official documentation/API indexing and a read-only GDScript diagnostic path — before normal project execution. See `ROADMAP.md`.
