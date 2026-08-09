# Solaris

Solaris is an independent, provider-neutral, interactive agent harness for programming and developing games with the Godot Engine.

This repository currently contains the **foundation vertical slice**: an executable interactive CLI, a provider-neutral application core, and a deterministic fake provider. It does not yet develop Godot games.

## Current status

Capability status conventions used in this document:

- **Surface implemented**: the contracts, tools, commands, and test coverage exist in the codebase.
- **Available**: the capability executes end to end in the shipped product.
- **Intentionally unavailable**: the surface exists but every entry point fails closed and reports `unavailable` — nothing executes, no approval is requested, and availability is never claimed.

In short: read-only workspace inspection is available; Git inspection is unavailable at this stage (the adapter requires Solaris-owned private run directories, whose creation and cleanup fail closed); workspace mutations, `/undo`, and development-command execution are intentionally unavailable; Godot discovery and static project inspection are available, but Godot engine probing is intentionally unavailable.

Working today:

- Interactive terminal session (`npm run solaris`)
- Slash commands: `/help`, `/status`, `/clear`, `/tools`, `/sandbox`, `/permissions`, `/git-status`, `/diff`, `/checkpoints`, `/undo`, `/commands`, `/cancel`, `/exit`, `/godot`, `/godot-installations`, `/godot-project`, `/godot-doctor` (all are surfaces; see the capability notes below — `/undo` and `/commands` fail closed as unavailable)
- Prompt submission with incrementally streamed responses
- A bounded provider/tool loop with the workspace-mutation tools `workspace.create_file`, `workspace.edit_file` (exact text replacements), and `workspace.delete_file` — **intentionally unavailable**: every entry point fails closed as `unavailable` before any write, approval, or checkpoint, because Node offers no directory-relative (openat/renameat) primitive and a same-user process can swap a parent or target at any instruction boundary. What remains is the core contracts (prepared-command/digest/approval ports) plus reusable tested primitives (path validation, diffing, hashing, the mutation lock, safe-replacement helpers) and the filesystem checkpoint store with its startup reconciliation (automatic retention pruning is disabled: storage pressure fails closed with a typed storage-limit refusal and deletes nothing — the byte limit measures actual regular-file bytes beneath the checkpoint directory including metadata and preimages (allocation overhead is outside the logical byte limit), the proposed checkpoint's exact serialized metadata and preimage bytes are counted before any write, declared preimages are content-verified against the metadata SHA-256 through a handle-bound bounded read loop (a same-size corrupted preimage makes capacity unverifiable, and a same-content hard-link, rename, symlink, or junction substitution is refused because identity, never content equality, is verified), preimage limits are capped at 64 MiB with larger configurations rejected at store creation, operations are bound to their before/after existence states (create: absent→present; update: present→present; delete: present→absent), and any content beyond the exact `cp_<valid-id>/metadata.json` + `cp_<valid-id>/preimage.bin` layout — unknown files, nested directories, temporary files, links, special files, or entries that cannot be inspected — makes capacity unverifiable and blocks new checkpoints, with unexpected content never repaired or deleted — and existing checkpoints are preserved for manual inspection); the former preview/approval/application logic was largely deleted, the identity-bound commit design is documented as future work but not offered, no approval for mutations is ever requested, and no new checkpoint is ever created at this stage (historical checkpoint data from earlier sessions, if any, may still be listed).
- Read-only workspace tools: `workspace.list`, `workspace.read` (with complete-file SHA-256), `workspace.search` — all paths are canonicalized and contained within the launch directory
- Read-only Git inspection (`git.status`, `git.diff`) through a trusted, allowlisted Git adapter — **intentionally unavailable at this stage**: the adapter requires verified Solaris-owned private run directories for every sandboxed Git process, and private run-directory creation and cleanup fail closed because Node offers no directory-relative (openat/mkdirat-style) or delete-by-handle primitive. Nothing Git-related executes. The adapter's design is mechanically sound for when that primitive exists: Git can only ever execute inside an enforcing sandbox backend (network denied, writes limited to the exact private run directory, host reads limited to the repository root and Git runtime roots, confined process tree), fixed argument arrays with no shell, command-line overrides as defense in depth (the enumerable mechanisms — fsmonitor, aliases, pagers, external diff, textconv, credential helpers, prompts — are disabled; repository-selected helpers such as clean/smudge/process filters are NOT disabled from the command line and their only containment is the sandbox), repository-redirecting and config-injecting environment variables stripped at the process boundary, bounded byte-counted output with a streaming UTF-8 decoder, timeouts, cancellation, and the resolved executable re-verified immediately before every launch request; structured summaries come from NUL-delimited machine-readable data with exact paths; the repository root must equal the workspace root. When the backend cannot enforce, the adapter reports Git unavailable and never spawns Git; the adapter itself never spawns processes (architecture-enforced).
- Safe user-invoked undo (`/undo`) — **intentionally unavailable**: restoring a checkpoint requires pathname-based displacement and replacement, and Node offers no directory-relative (openat/renameat) primitive, so `/undo` fails closed as `unavailable` before any write, approval, or restore. The undo service is an unavailable stub; the former reverse-diff/approval/restore machinery was largely deleted and is documented as future work rather than presented as shipped capability.
- A sandbox and permission foundation: capability policy, built-in `inspect` and `develop-offline` profiles, a pure permission evaluator, an Anthropic Sandbox Runtime backend behind a core-owned port with an enforced host-read allowlist (deny-root with re-allow on Linux/macOS; reported unavailable and refused on Windows), allowlist-based child environments with the wrapper's runtime-required environment merged under strict rules, fixed conformance probes (`npm run test:sandbox`), `/sandbox` and `/permissions` diagnostics, and a `--sandbox-doctor` CLI command with trustworthy exit codes (0 passed, 1 probe failure, 3 probes unavailable)
- The sandboxed development-command surface (`process.run`): structured arguments only (never a provider-supplied shell string), read-only workspace, denied network, a minimal sanitized environment, closed stdin, bounded streamed output, bounded timeouts, process-tree cancellation, and digest-bound one-time approval under the internal `validation-offline` profile. No command can execute at this stage: both the `node-script` and `npm-script` runners fail closed as `unavailable` — the pinned Node runtime cannot mechanically bind execution to the approved script bytes, because the script can reach internal surfaces such as `process.binding` (e.g. `spawn_sync`) to spawn an unconstrained interpreter and the staged private copy can be substituted by a same-user process in the verify-to-launch window — so `isAvailable()` returns false for both and every request is refused before any approval.
- Godot executable discovery and validation, before any project execution: trusted user-configured installations (absolute paths with optional edition hints) plus fixed-name PATH search — no broad filesystem scanning; exact executable fingerprints (canonical path, size, mtime, SHA-256) with full-hash revalidation. **Engine probing is intentionally unavailable**: the probe runner reports `unavailable` and never spawns the executable, because the backend re-opens the staged copy's pathname at spawn time and a same-user process can substitute bytes between final verification and launch (no exec-by-handle primitive) — so no engine profile can be produced and `godot.inspect_engine` cannot return a profile. The engine-profile cache is an **explicitly unavailable no-op component**: it is never initialized, created, read, or written (`load()` is always a miss, `store()` returns a typed unavailable result, `count()` is 0, and `--godot-doctor` reports it disabled) — the earlier storage implementation was removed rather than retained as an unsafe surface. The designed probe path (project-independent fixed probes `--version`/`--help`/`--dump-extension-api` through the sandbox backend under the internal `godot-probe-offline` profile, which excludes the workspace from readable roots, adversarial version parsing, conservative edition classification, deterministic selection ranking with recorded rationale) is documented but not offered at this stage.
- Static Godot project detection and profiling: only the root `project.godot` is read (regular file, symlinks rejected, never parents/children), everything parsed conservatively and never evaluated, plus an executable-content inventory (tool scripts, editor plugins, GDExtension descriptors, autoloads, C# project files) that never loads or runs anything. Every inspection rescans the complete bounded project — no profile cache is used
- Godot provider tools and CLI surface: `godot.inspect_engine` and `godot.inspect_project` (allow in every built-in policy, no one-time approval; `godot.inspect_engine` cannot return a profile while probing is unavailable), `/godot`, `/godot-installations` (lists unprofiled candidates with the reason), `/godot-project`, `/godot-doctor` (exits 0–6; code 6 = degraded), the `--godot-path` / `--godot-installation` / `--godot-doctor` startup flags, and the `SOLARIS_GODOT` / `SOLARIS_GODOT_INSTALLATION` environment overrides
- In-process conversation history
- Cancellation support through `AbortSignal`
- Deterministic fake provider (`deterministic-fake`) that requires no credentials and no network, with synthetic scenarios for read tools, the write-workflow scenarios (`create solaris-write-test`, `edit solaris-write-test`, `delete solaris-write-test`) that now exercise the fail-closed `unavailable` path, Git inspection (`git status`, `show working diff`, `show staged diff`, `show head diff`), and development commands (`run npm check`, `run npm test`, `run node validation fixture`), which are refused as `unavailable`

Not yet implemented:

- General shell access, arbitrary executables, writable command execution, package installation, or background processes — no command can execute at this stage: both the `node-script` and `npm-script` runners fail closed as `unavailable` (workspace-read-only and offline by design), so every `process.run` request is refused with an explanation
- Workspace mutations of any kind: `workspace.create_file`, `workspace.edit_file`, `workspace.delete_file`, and `/undo` all fail closed as `unavailable` before any write, approval, or checkpoint (Node offers no directory-relative primitive; see "Working today" above). No approval for mutations is ever requested
- Git inspection of any kind: the adapter is unavailable at this stage because private run-directory creation and cleanup fail closed (no directory-relative or delete-by-handle primitive in Node); Git can only ever execute inside an enforcing sandbox backend and is never spawned outside it
- Git writes of any kind: staging, commits, reset, restore, checkout, clean, stash, branches, worktrees, remotes
- Godot project execution: Solaris does not open, import, execute, or run any Godot project; recovery-mode project probing is not implemented. GDScript programming and editor/runtime integration remain unimplemented. Godot engine probing (engine profiles, `godot.inspect_engine` results) is intentionally unavailable at this stage
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

| Command                               | Purpose                                                                                                                                                                                                                     |
| ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `npm run build`                       | Build all workspaces into their `dist/` directories                                                                                                                                                                         |
| `npm run clean`                       | Remove all build output                                                                                                                                                                                                     |
| `npm run format`                      | Format the repository with Prettier (may modify files)                                                                                                                                                                      |
| `npm run format:check`                | Verify formatting without modifying files                                                                                                                                                                                   |
| `npm run lint`                        | Lint with type-aware ESLint rules                                                                                                                                                                                           |
| `npm run typecheck`                   | Type-check with strict TypeScript                                                                                                                                                                                           |
| `npm test`                            | Run all tests once (Vitest)                                                                                                                                                                                                 |
| `npm run test:watch`                  | Run tests in watch mode                                                                                                                                                                                                     |
| `npm run check:architecture`          | Verify workspace dependency boundaries (developer guardrail: structural TypeScript parsing plus regex/text checks, not an OS security boundary)                                                                             |
| `npm run test:sandbox`                | Run live sandbox conformance probes (skips loudly when the backend is unavailable)                                                                                                                                          |
| `npm run test:godot`                  | Run live Godot probe conformance (opt-in; requires `SOLARIS_TEST_GODOT="<absolute-path>"`). At this stage it reports UNAVAILABLE loudly (probing fails closed) and never passes; it never modifies the user-supplied engine |
| `npm run check`                       | Run all non-mutating validation                                                                                                                                                                                             |
| `npm run solaris`                     | Build and launch the interactive CLI                                                                                                                                                                                        |
| `npm run solaris -- --sandbox-doctor` | Print sandbox diagnostics (add `--run-probes` to run fixed probes; exit 0 passed, 1 probe failure, 3 probes unavailable)                                                                                                    |
| `npm run solaris -- --godot-doctor`   | Print Godot discovery, selection, and cache diagnostics (exit 0–6; 0 success, 1 no valid engine, 2 selection failure, 3 sandbox unavailable, 4 probe failure, 5 identity mismatch, 6 degraded)                              |

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

Supported profiles: `inspect` (default; read-only, no processes, no network — write and process tools are not exposed to the provider) and `develop-offline` (workspace writes require one-time approval, processes require one-time approval per exact command plan, network denied). Backends: `auto` and `anthropic-runtime`. An untrusted repository can never broaden these settings. No command can currently execute: both runners fail closed as unavailable (see below), and every workspace mutation fails closed as `unavailable` before any approval — approval is never requested for mutations or commands at this stage. Commands would run under the internal `validation-offline` profile (the project workspace is readable but never writable), and Godot probes would run under the internal `godot-probe-offline` profile, which excludes the workspace from readable roots (the workspace is never writable and never readable during probing); engine probing is intentionally unavailable at this stage. See `SECURITY.md` for the full security model.

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
  workspace.list - List one directory within the approved workspace. (read-only, allowed)
  workspace.read - Read a bounded range from one text file inside the workspace. (read-only, allowed)
  workspace.search - Search text files recursively within a bounded workspace directory. (read-only, allowed)
  git.status - Show structured Git repository status. (read-only, allowed)
  git.diff - Show a bounded Git diff for the working tree, index, or HEAD. (read-only, allowed)
  godot.inspect_engine - Inspect the selected Godot installation: exact version, edition, release channel, Solaris support classification, advertised and operationally verified capabilities, and the extension API dump fingerprint. Read-only; no project is opened or imported. (read-only, allowed)
  godot.inspect_project - Statically inspect the Godot project at the workspace root: detection, name, config version, declared engine version, main scene, language profile, rendering methods, autoloads, enabled plugins, tool scripts, editor plugins, GDExtension descriptors, and compatibility with the selected engine. No project code is executed or imported. (read-only, allowed)

> /status

Provider: deterministic-fake
Session: active
Messages: 4
Workspace: C:\Users\...\Solaris
Tools: 7

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
      godot/               discovery, fail-closed probe runner, profiling, static project inspection
      tools/workspace/     read-only workspace tools + mutation tools (fail closed as unavailable)
docs/
  adr/                     architecture decision records
schemas/                   user configuration JSON Schema
scripts/
  sandbox/                 live conformance runner
scripts/                   architecture checks, validation fixture
```

## Architecture summary

- `@solaris/core` owns application behaviour, conversation history, the provider port, the security model (capability policy, built-in sandbox profiles, the pure permission evaluator, the `SandboxBackend` port, classified errors), the Git-neutral inspection contracts (`GitInspector`, status/diff models, error categories), the checkpoint model (metadata, lifecycle, the `CheckpointStore` port, undo planning and conflict rules), and the provider-neutral development-command contracts (runner contracts, the immutable runner registry, command limits, the deterministic command digest, the opaque single-use prepared command, and the `PreparedCommandTool` contract). It imports no Node infrastructure, no adapters, no UI code, and no sandbox runtime.
- `@solaris/adapters` implements ports: the deterministic fake provider, the read-only workspace tools, the workspace-mutation surface (the `workspace.create_file`/`workspace.edit_file`/`workspace.delete_file` tools and `/undo` are unavailable entry points that fail closed as `unavailable` before any write, approval, or checkpoint; the reusable primitives — path validation, diffing, hashing, the mutation lock, safe-replacement helpers — and the checkpoint store remain as tested internal code with fail-closed retention (no automatic pruning; the byte limit counts actual stored regular-file bytes beneath the checkpoint directory including metadata and preimages, declared preimage bytes are hash-verified through a handle-bound bounded read loop during capacity checks (same-content substitutions are refused), preimage limits are capped with larger configurations rejected at store creation, operations are bound to their before/after existence states, unexpected checkpoint content makes capacity unverifiable and is never repaired or deleted, and storage pressure refuses new checkpoints and deletes nothing), and no new checkpoint is ever created at this stage), the allowlist child-environment builder, the trusted Git CLI adapter (fixed allowlisted subcommands, no shell, sanitized environment, bounded output, launch-time executable re-verification — Git can only ever execute inside the sandbox backend and is unavailable at this stage because private run-directory creation and cleanup fail closed), the durable filesystem checkpoint store, and the command layer — trusted Node/npm CLI resolution, the `npm-script` and `node-script` runners (both fail closed as unavailable, so no command can execute at this stage), the private run-directory provider (creation and cleanup fail closed as unavailable), and the `process.run` tool that would execute approved plans through the `SandboxBackend` under the `validation-offline` profile. Only the sandbox adapter module may import the runtime package and spawn processes; the Git adapter never spawns directly.
- `@solaris/cli` is a terminal input/output adapter. It parses input, renders events, reviews approvals interactively, and composes dependencies in one composition root. `/commands` and `/cancel` are CLI capabilities; the CLI never spawns commands and never renders sandbox-private paths.
- Dependency direction is inward: `CLI -> Core` and `CLI -> composition -> Adapters -> Core ports`. `npm run check:architecture` enforces this mechanically (a developer guardrail using structural TypeScript parsing plus regex/text checks), including process, Git, checkpoint, and sandbox boundaries and the absence of raw process execution (`shell: true`, `exec`, `execSync`, `spawnSync`) in runtime code.
- See `ARCHITECTURE.md`, `SECURITY.md`, and the ADRs in `docs/adr/` for details.

## Testing and validation

```bash
npm run check
```

runs formatting, linting, type checking, tests, and the architecture check without modifying files. Before a change is considered complete, it must pass.

## Next planned milestone

Godot executable discovery, validation, and static project detection are complete; read-only engine capability probing is intentionally unavailable at this stage (the probe runner reports `unavailable` and never spawns the executable), so no engine profile can be produced until an identity-bound launch primitive exists. The next narrow task after that is to add version-matched Godot knowledge profiles and GDScript language intelligence — official documentation/API indexing and a read-only GDScript diagnostic path — before normal project execution. Solaris does not open, import, execute, or run any Godot project at this stage. See `ROADMAP.md`.
