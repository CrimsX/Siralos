# Solaris

Solaris is an independent, provider-neutral, interactive agent harness for programming and developing games with the Godot Engine.

This repository contains the provider-neutral harness foundations through
**Stage 3 milestone 7 (Host-Controlled Planning)**: an executable interactive
CLI, deterministic fake provider, bounded tool and projection pipelines,
read-only workspace/Godot inspection, structured task state, references and
research contracts, capability diagnostics, and revision-bound planning. It
does not yet develop Godot games end to end: every operation that would require
an identity-bound filesystem or process primitive still fails closed as
`unavailable`.

## Current status

Capability status conventions used in this document:

- **Surface implemented**: the contracts, tools, commands, and test coverage exist in the codebase.
- **Available**: the capability executes end to end in the shipped product.
- **Intentionally unavailable**: the surface exists but every entry point fails closed and reports `unavailable` — nothing executes, no approval is requested, and availability is never claimed.

In short: read-only workspace inspection is available; Git inspection is unavailable at this stage (the adapter requires Solaris-owned private run directories, whose creation and cleanup fail closed); workspace mutations, `/undo`, and development-command execution are intentionally unavailable; the GDScript development workflow (the bounded inspect → propose → approve → checkpoint → apply → parse → fresh-LSP → diagnose → repair loop) is implemented as contracts, change-set machinery, orchestration, and truthful reporting, but its execution gate — the exact change-set applier — fails closed as unavailable on every platform, so no approval for a mutation is ever requested at this stage and no checkpoint is ever created; Godot discovery and static project inspection are available, but Godot engine probing is intentionally unavailable.

Working today:

- Interactive terminal session (`npm run solaris`)
- Slash commands: `/help`, `/status`, `/clear`, `/tools`, `/sandbox`, `/permissions`, `/git-status`, `/diff`, `/checkpoints`, `/undo`, `/commands`, `/cancel`, `/develop`, `/plan`, `/development-status`, `/quality`, `/review-change`, `/exit`, `/godot`, `/godot-installations`, `/godot-project`, `/godot-doctor`, `/godot-probe`, `/godot-probe-status`, `/godot-knowledge`, `/godot-knowledge-refresh`, `/godot-api`, `/gdscript-check`, `/gdscript-diagnostics`, `/doctor`, `/solaris` (all are surfaces; see the capability notes below — `/undo` and `/commands` fail closed as unavailable, `/godot-probe` reports the recovery capability truthfully, `/godot-knowledge-refresh`, `/gdscript-check`, and `/gdscript-diagnostics` refuse before any approval while their execution is unavailable, `/develop` refuses before any approval for a mutation while the change-set applier is unavailable, and `/plan` is plan-only: read-only planning with zero workspace changes and zero mutation checkpoints)
- Prompt submission with incrementally streamed responses
- A bounded provider/tool loop with the workspace-mutation tools `workspace.create_file`, `workspace.edit_file` (exact text replacements), and `workspace.delete_file` — **intentionally unavailable**: every entry point fails closed as `unavailable` before any write, approval, or checkpoint, because Node offers no directory-relative (openat/renameat) primitive and a same-user process can swap a parent or target at any instruction boundary. What remains is the core contracts (prepared-command/digest/approval ports) plus reusable tested primitives (path validation, diffing, hashing, the mutation lock, safe-replacement helpers) and the filesystem checkpoint store with its startup reconciliation (automatic retention pruning is disabled: storage pressure fails closed with a typed storage-limit refusal and deletes nothing — the byte limit measures actual regular-file bytes beneath the checkpoint directory including metadata and preimages (allocation overhead is outside the logical byte limit), the proposed checkpoint's exact serialized metadata and preimage bytes are counted before any write, declared preimages are content-verified against the metadata SHA-256 through a handle-bound bounded read loop with a pre-read/final stability snapshot (identity plus size and mtime/ctime nanoseconds, so a same-size corrupted preimage, a same-content hard-link/rename/symlink/junction substitution, or a same-inode in-place rewrite during verification makes capacity unverifiable), preimage limits are capped at 64 MiB with larger configurations rejected at store creation, operations are bound to their before/after existence states (create: absent→present; update: present→present; delete: present→absent), and any content beyond the exact `cp_<valid-id>/metadata.json` + `cp_<valid-id>/preimage.bin` layout — unknown files, nested directories, temporary files, links, special files, or entries that cannot be inspected — makes capacity unverifiable and blocks new checkpoints, with unexpected content never repaired or deleted — and existing checkpoints are preserved for manual inspection); the former preview/approval/application logic was largely deleted, the identity-bound commit design is documented as future work but not offered, no approval for mutations is ever requested, and no new checkpoint is ever created at this stage (historical checkpoint data from earlier sessions, if any, may still be listed).
- Read-only workspace tools: `workspace.list`, `workspace.read` (with complete-file SHA-256), `workspace.search` — all paths are canonicalized and contained within the launch directory
- Read-only Git inspection (`git.status`, `git.diff`) through a trusted, allowlisted Git adapter — **intentionally unavailable at this stage**: the adapter requires verified Solaris-owned private run directories for every sandboxed Git process, and private run-directory creation and cleanup fail closed because Node offers no directory-relative (openat/mkdirat-style) or delete-by-handle primitive. Nothing Git-related executes. The adapter's design is mechanically sound for when that primitive exists: Git can only ever execute inside an enforcing sandbox backend (network denied, writes limited to the exact private run directory, host reads limited to the repository root and Git runtime roots, confined process tree), fixed argument arrays with no shell, command-line overrides as defense in depth (the enumerable mechanisms — fsmonitor, aliases, pagers, external diff, textconv, credential helpers, prompts — are disabled; repository-selected helpers such as clean/smudge/process filters are NOT disabled from the command line and their only containment is the sandbox), repository-redirecting and config-injecting environment variables stripped at the process boundary, bounded byte-counted output with a streaming UTF-8 decoder, timeouts, cancellation, and the resolved executable re-verified immediately before every launch request; structured summaries come from NUL-delimited machine-readable data with exact paths; the repository root must equal the workspace root. When the backend cannot enforce, the adapter reports Git unavailable and never spawns Git; the adapter itself never spawns processes (architecture-enforced).
- Safe user-invoked undo (`/undo`) — **intentionally unavailable**: restoring a checkpoint requires pathname-based displacement and replacement, and Node offers no directory-relative (openat/renameat) primitive, so `/undo` fails closed as `unavailable` before any write, approval, or restore. The undo service is an unavailable stub; the former reverse-diff/approval/restore machinery was largely deleted and is documented as future work rather than presented as shipped capability.
- A sandbox and permission foundation: capability policy, built-in `inspect` and `develop-offline` profiles, a pure permission evaluator, an Anthropic Sandbox Runtime backend behind a core-owned port with an enforced host-read allowlist (deny-root with re-allow on Linux/macOS; reported unavailable and refused on Windows), allowlist-based child environments with the wrapper's runtime-required environment merged under strict rules, fixed conformance probes (`npm run test:sandbox`), `/sandbox` and `/permissions` diagnostics, and a `--sandbox-doctor` CLI command with trustworthy exit codes (0 passed, 1 probe failure, 3 probes unavailable)
- The sandboxed development-command surface (`process.run`): structured arguments only (never a provider-supplied shell string), read-only workspace, denied network, a minimal sanitized environment, closed stdin, bounded streamed output, bounded timeouts, process-tree cancellation, and digest-bound one-time approval under the internal `validation-offline` profile. No command can execute at this stage: both the `node-script` and `npm-script` runners fail closed as `unavailable` — the pinned Node runtime cannot mechanically bind execution to the approved script bytes, because the script can reach internal surfaces such as `process.binding` (e.g. `spawn_sync`) to spawn an unconstrained interpreter and the staged private copy can be substituted by a same-user process in the verify-to-launch window — so `isAvailable()` returns false for both and every request is refused before any approval.
- Godot executable discovery and validation, before any project execution: trusted user-configured installations (absolute paths with optional edition hints) plus fixed-name PATH search — no broad filesystem scanning; exact executable fingerprints (canonical path, size, mtime, SHA-256) with full-hash revalidation. **Engine probing is intentionally unavailable**: the probe runner reports `unavailable` and never spawns the executable, because the backend re-opens the staged copy's pathname at spawn time and a same-user process can substitute bytes between final verification and launch (no exec-by-handle primitive) — so no engine profile can be produced and `godot.inspect_engine` cannot return a profile. The engine-profile cache is an **explicitly unavailable no-op component**: it is never initialized, created, read, or written (`load()` is always a miss, `store()` returns a typed unavailable result, `count()` is 0, and `--godot-doctor` reports it disabled) — the earlier storage implementation was removed rather than retained as an unsafe surface. The designed probe path (project-independent fixed probes `--version`/`--help`/`--dump-extension-api` through the sandbox backend under the internal `godot-probe-offline` profile, which excludes the workspace from readable roots, adversarial version parsing, conservative edition classification, deterministic selection ranking with recorded rationale) is documented but not offered at this stage.
- Static Godot project detection and profiling: only the root `project.godot` is read (regular file, symlinks rejected, never parents/children), everything parsed conservatively and never evaluated, plus an executable-content inventory (tool scripts, editor plugins, GDExtension descriptors, autoloads, C# project files) that never loads or runs anything. Every inspection rescans the complete bounded project — no profile cache is used
- Godot provider tools and CLI surface: `godot.inspect_engine` and `godot.inspect_project` (allow in every built-in policy, no one-time approval; `godot.inspect_engine` cannot return a profile while probing is unavailable), `/godot`, `/godot-installations` (lists unprofiled candidates with the reason), `/godot-project`, `/godot-doctor` (exits 0–7; code 7 = explicitly requested recovery capability unavailable via `--godot-doctor --recovery-probe`), the `--godot-path` / `--godot-installation` / `--godot-doctor` startup flags, and the `SOLARIS_GODOT` / `SOLARIS_GODOT_INSTALLATION` environment overrides
- The recovery-mode project-probe surface (`godot.probe_project`, `/godot-probe`, `/godot-probe-status`): bounded static preparation (authored-file manifest, risk digest, workspace-integrity baseline, diagnostic classification), a one-time approval protocol with expiring single-use prepared probes, and truthful capability reporting — **execution fails closed as unavailable on every platform** (the disposable mirror and the recovery runner are fail-closed no-ops that never create or launch anything, because Node offers no exec-by-handle, no directory-relative create, and no delete-by-handle primitive), so the CLI refuses before requesting approval and the doctor reports the capability truthfully (see ADR 0009)
- The version-matched Godot API knowledge surface (ADR 0010): the knowledge-profile model bound to the exact executable SHA-256 and API dump, deterministic API symbol identities, the bounded index builder over `--dump-extension-api-with-docs` dumps (classes, methods, properties, signals, constants, enums, utility functions, built-in classes, operators), literal/token search and exact lookup with deterministic ranking, and the manual-documentation-channel classification. Provider tools `godot.api_search` and `godot.api_lookup` (allow in the user-facing profiles; no approval), CLI commands `/godot-knowledge`, `/godot-api <query>`, `/godot-knowledge-refresh`, and truthful `/status` knowledge reporting. **Generation fails closed as unavailable on every platform** (the fixed `--dump-extension-api-with-docs` runner never spawns the executable — no exec-by-handle primitive — so no profile is ever produced) and the knowledge cache is an explicitly unavailable no-op component (never initialized, created, read, or written: `load()` is always a miss, `store()` returns a typed unavailable outcome, `count()` is 0)
- The read-only GDScript diagnostic surface (`godot.check_script`, `godot.check_project_scripts`, `/gdscript-check <relative-path>`, `/gdscript-diagnostics`): workspace-relative `.gd` validation (symlinks and non-regular files rejected, size-bounded), deterministic bounded project-wide enumeration, script content hashing, conservative engine-output normalization (parser errors, indentation errors, unknown identifiers, invalid types, missing base classes, warnings, control-character sanitization, mirror-path normalization, generic preservation of unmatched error-like lines), deterministic aggregation, and a one-time approval protocol binding the exact script hashes, risk-manifest digest, fixed `--headless --path <disposable-mirror> --script <mirror-script> --check-only` command, sandbox profile, and limits. **Execution fails closed as unavailable on every platform** (the check-only runner and the disposable mirror are fail-closed no-ops that never create or launch anything), so the CLI refuses before requesting approval and a script parse failure would be a valid diagnostic result, never an infrastructure failure (see ADR 0010)
- The bounded GDScript language-session surface (ADR 0011): the provider-neutral session port (`GDScriptLanguageService`), the immutable prepared-session plan bound to the risk manifest, executable SHA-256, engine version, mirror-copy policy, LSP capability set, sandbox profile, and LSP policy version; the incremental LSP frame parser and bounded JSON-RPC client (correlation, pending bound, timeouts, cancellation, safe late/duplicate handling, server-request rejection incl. `workspace/applyEdit`/`workspace/executeCommand`); conservative normalization of diagnostics/hover/completion/definition into the existing models (mirror URIs to workspace-relative paths, 1-based positions, bounded fields, markup as data, insertText never applied); the loopback-only dynamic port allocator; the fixed `--headless --editor --recovery-mode --path <disposable-mirror> --lsp-port <allocated>` runner; staleness revalidation; provider tools `godot.lsp_session`, `godot.hover`, `godot.complete`, `godot.definition`, `godot.lsp_diagnostics`; CLI commands `/gdscript-lsp`, `/gdscript-lsp-stop`, `/gdscript-hover`, `/gdscript-complete`, `/gdscript-definition`; and `/status` reporting. **Session startup fails closed as unavailable on every platform**: the LSP runner never spawns the editor, no mirror is created, no port is opened, no approval is requested, and the live isolation probe is reported skipped, never passed (no exec-by-handle / directory-relative-create / delete-by-handle primitives)
- The bounded GDScript development workflow (ADR 0012): the first complete GDScript programming loop — `/develop <request>` starts one workflow through a one-time approval that binds the request, the project and engine fingerprints, and the immutable limits, and covers only the read-only validation context (LSP recreation after approved edits, `--check-only` parsing, API lookup, workspace and Git inspection); the provider investigates with the read-only tools, proposes an exact text change set (`workspace.apply_text_changeset`: bounded create/edit/delete with exact SHA-256 preconditions, complete deterministic diff, immutable digest), and each change set — including every repair — still requires its own exact one-time approval; on apply the language session is suspended before the edit (a failed suspension never applies), every file is checkpointed before application, files are applied sequentially with post-state hash verification under the mutation lock, changed scripts run `--check-only`, a fresh disposable mirror and language session are recreated (engine fingerprint unchanged, project delta exactly the approved change sets), LSP diagnostics settle deterministically, and bounded validation evidence (parser, LSP, Git, workspace integrity with unexpected-change detection) is collected; repairs are bounded (3 proposals, 4 iterations) and denial or cancellation preserves approved changes. The workflow state machine, change-set machinery, checkpoint/apply/recovery protocol, and evidence model are fully implemented and tested through injected in-memory primitives and fakes, **but the change-set applier fails closed as unavailable on every platform** (no directory-relative commit primitive in Node), so the workflow refuses before any approval for a mutation, no checkpoint is ever created, and `/develop` reports the capability truthfully; the opt-in `npm run test:godot-development` conformance verifies this fail-closed behavior and always reports the live development-loop isolation probe as skipped, never passed
- In-process conversation history
- Cancellation support through `AbortSignal`
- Deterministic fake provider (`deterministic-fake`) that requires no credentials and no network, with synthetic scenarios for read tools, the write-workflow scenarios (`create solaris-write-test`, `edit solaris-write-test`, `delete solaris-write-test`) that now exercise the fail-closed `unavailable` path, Git inspection (`git status`, `show working diff`, `show staged diff`, `show head diff`), development commands (`run npm check`, `run npm test`, `run node validation fixture`), which are refused as `unavailable`, and the development-loop scenarios (`develop fixture`, `develop fixture with repair`) that read a fixture script, propose an exact change set, and summarize the validated result or the truthful failure

Not yet implemented:

- General shell access, arbitrary executables, writable command execution, package installation, or background processes — no command can execute at this stage: both the `node-script` and `npm-script` runners fail closed as `unavailable` (workspace-read-only and offline by design), so every `process.run` request is refused with an explanation
- Workspace mutations of any kind: `workspace.create_file`, `workspace.edit_file`, `workspace.delete_file`, `workspace.apply_text_changeset`, and `/undo` all fail closed as `unavailable` before any write, approval, or checkpoint (Node offers no directory-relative primitive; see "Working today" above). No approval for mutations is ever requested. The development workflow's change-set applier has the same gate: the full checkpoint/apply/recovery protocol is tested internal code, but no change set is ever applied at this stage
- Git inspection of any kind: the adapter is unavailable at this stage because private run-directory creation and cleanup fail closed (no directory-relative or delete-by-handle primitive in Node); Git can only ever execute inside an enforcing sandbox backend and is never spawned outside it
- Git writes of any kind: staging, commits, reset, restore, checkout, clean, stash, branches, worktrees, remotes
- Godot project execution: Solaris does not open, import, execute, or run any Godot project. The recovery-mode project-probe surface is restored (contracts, static preparation, one-time approval machinery, diagnostics, CLI reporting, ADR 0009) but **execution is unavailable on every platform at this stage**: the disposable mirror and the recovery runner fail closed with typed `unavailable` outcomes, never create or delete anything, and never launch the engine, until Node provides identity-bound launch and mirror-lifecycle primitives. The GDScript development workflow (ADR 0012) implements the complete edit/validate/repair loop — change-set preparation, one-time approvals, checkpoint-before-apply, parser and fresh-LSP gates, validation evidence, bounded repairs — but its change-set applier also fails closed as unavailable on every platform (no directory-relative commit primitive), so no mutation, approval for a mutation, or checkpoint ever occurs at this stage. GDScript programming, editor/runtime integration, and the live development loop remain unimplemented. Godot engine probing (engine profiles, `godot.inspect_engine` results), exact-engine API knowledge generation, and GDScript check-only diagnostics are intentionally unavailable at this stage (the fixed runners never spawn the executable; see ADR 0010)
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

| Command                               | Purpose                                                                                                                                                                                                                                                                                                                                                                                                                              |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `npm run build`                       | Build all workspaces into their `dist/` directories                                                                                                                                                                                                                                                                                                                                                                                  |
| `npm run clean`                       | Remove all build output                                                                                                                                                                                                                                                                                                                                                                                                              |
| `npm run format`                      | Format the repository with Prettier (may modify files)                                                                                                                                                                                                                                                                                                                                                                               |
| `npm run format:check`                | Verify formatting without modifying files                                                                                                                                                                                                                                                                                                                                                                                            |
| `npm run lint`                        | Lint with type-aware ESLint rules                                                                                                                                                                                                                                                                                                                                                                                                    |
| `npm run typecheck`                   | Type-check with strict TypeScript                                                                                                                                                                                                                                                                                                                                                                                                    |
| `npm test`                            | Run all tests once (Vitest)                                                                                                                                                                                                                                                                                                                                                                                                          |
| `npm run test:watch`                  | Run tests in watch mode                                                                                                                                                                                                                                                                                                                                                                                                              |
| `npm run check:architecture`          | Verify workspace dependency boundaries (developer guardrail: structural TypeScript parsing plus regex/text checks, not an OS security boundary)                                                                                                                                                                                                                                                                                      |
| `npm run test:sandbox`                | Run live sandbox conformance probes (skips loudly when the backend is unavailable)                                                                                                                                                                                                                                                                                                                                                   |
| `npm run test:godot`                  | Run live Godot probe conformance (opt-in; requires `SOLARIS_TEST_GODOT="<absolute-path>"`). At this stage it reports UNAVAILABLE loudly (probing fails closed) and never passes; it never modifies the user-supplied engine                                                                                                                                                                                                          |
| `npm run test:godot-recovery`         | Run live recovery-mode conformance (opt-in; requires `SOLARIS_TEST_GODOT="<absolute-path>"`). Verifies the fail-closed behavior truthfully (capability unavailable with a precise reason, nothing created, nothing executed); the live engine-isolation probe is always reported skipped while execution is unavailable and never passed                                                                                             |
| `npm run test:godot-development`      | Run live development-workflow conformance (opt-in; requires `SOLARIS_TEST_GODOT="<absolute-path>"`). Verifies the fail-closed behavior truthfully (capability unavailable with a precise reason, workflow preparation refuses before any approval, nothing created or executed, source untouched); the live development-loop isolation probe is always reported skipped while the change-set applier is fail-closed and never passed |
| `npm run test:godot-quality`          | Run live quality-stage conformance (opt-in; requires `SOLARIS_TEST_GODOT="<absolute-path>"`). Verifies the fail-closed behavior truthfully (workflow unavailable, so no quality stage, review, or validation command runs; nothing created or executed); the live quality-stage isolation probe is always reported skipped while the change-set applier is fail-closed and never passed                                              |
| `npm run check`                       | Run all non-mutating validation                                                                                                                                                                                                                                                                                                                                                                                                      |
| `npm run solaris`                     | Build and launch the interactive CLI                                                                                                                                                                                                                                                                                                                                                                                                 |
| `npm run solaris -- --sandbox-doctor` | Print sandbox diagnostics (add `--run-probes` to run fixed probes; exit 0 passed, 1 probe failure, 3 probes unavailable)                                                                                                                                                                                                                                                                                                             |
| `npm run solaris -- --godot-doctor`   | Print Godot discovery, selection, and cache diagnostics (exit 0–7; 0 success, 1 no valid engine, 2 selection failure, 3 sandbox unavailable, 4 probe failure, 5 identity mismatch, 6 degraded, 7 recovery capability unavailable when `--recovery-probe` was requested)                                                                                                                                                              |

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
      godot/development/   GDScript development workflow, change-set preparation/executor (fail-closed)
      godot/quality/       quality stage, validation plan discovery/executor,
                           provider + fake independent reviewers (read-only)
      tools/workspace/     read-only workspace tools + mutation tools (fail closed as unavailable)
      providers/           deterministic fake provider (with tool scenarios)
docs/
  adr/                     architecture decision records
schemas/                   user configuration JSON Schema
scripts/
  sandbox/                 live conformance runner
scripts/                   architecture checks, validation fixture
```

## Architecture summary

- `@solaris/core` owns application behaviour, conversation history, the provider port, the security model (capability policy, built-in sandbox profiles, the pure permission evaluator, the `SandboxBackend` port, classified errors), the Git-neutral inspection contracts (`GitInspector`, status/diff models, error categories), the checkpoint model (metadata, lifecycle, the `CheckpointStore` port, undo planning and conflict rules), the provider-neutral development-command contracts (runner contracts, the immutable runner registry, command limits, the deterministic command digest, the opaque single-use prepared command, and the `PreparedCommandTool` contract), and the GDScript development-workflow contracts (phases, statuses, the bounded session and evidence models, the development result, immutable workflow limits, the exact text change-set contract with canonical digest, the checkpoint-then-apply protocol with partial-failure recovery, and the `GDScriptDevelopmentService` port). It imports no Node infrastructure, no adapters, no UI code, and no sandbox runtime.
- `@solaris/adapters` implements ports: the deterministic fake provider, the read-only workspace tools, the workspace-mutation surface (the `workspace.create_file`/`workspace.edit_file`/`workspace.delete_file` tools and `/undo` are unavailable entry points that fail closed as `unavailable` before any write, approval, or checkpoint; the reusable primitives — path validation, diffing, hashing, the mutation lock, safe-replacement helpers — and the checkpoint store remain as tested internal code with fail-closed retention (no automatic pruning; the byte limit counts actual stored regular-file bytes beneath the checkpoint directory including metadata and preimages, declared preimage bytes are hash-verified through a handle-bound bounded read loop with a pre-read/final stability snapshot during capacity checks (same-content substitutions and same-inode in-place rewrites are refused), preimage limits are capped with larger configurations rejected at store creation, operations are bound to their before/after existence states, unexpected checkpoint content makes capacity unverifiable and is never repaired or deleted, and storage pressure refuses new checkpoints and deletes nothing), and no new checkpoint is ever created at this stage), the allowlist child-environment builder, the trusted Git CLI adapter (fixed allowlisted subcommands, no shell, sanitized environment, bounded output, launch-time executable re-verification — Git can only ever execute inside the sandbox backend and is unavailable at this stage because private run-directory creation and cleanup fail closed), the durable filesystem checkpoint store, and the command layer — trusted Node/npm CLI resolution, the `npm-script` and `node-script` runners (both fail closed as unavailable, so no command can execute at this stage), the private run-directory provider (creation and cleanup fail closed as unavailable), and the `process.run` tool that would execute approved plans through the `SandboxBackend` under the `validation-offline` profile. Only the sandbox adapter module may import the runtime package and spawn processes; the Git adapter never spawns directly.
- `@solaris/cli` is a terminal input/output adapter. It parses input, renders events, reviews approvals interactively, and composes dependencies in one composition root. `/commands` and `/cancel` are CLI capabilities; the CLI never spawns commands and never renders sandbox-private paths.
- Dependency direction is inward: `CLI -> Core` and `CLI -> composition -> Adapters -> Core ports`. `npm run check:architecture` enforces this mechanically (a developer guardrail using structural TypeScript parsing plus regex/text checks), including process, Git, checkpoint, and sandbox boundaries and the absence of raw process execution (`shell: true`, `exec`, `execSync`, `spawnSync`) in runtime code.
- See `ARCHITECTURE.md`, `SECURITY.md`, and the ADRs in `docs/adr/` for details.

## Testing and validation

```bash
npm run check
```

runs formatting, linting, type checking, tests, and the architecture check without modifying files. Before a change is considered complete, it must pass.

## Self-reference and capability doctor

Solaris explains its own installed behavior from host-owned metadata
instead of model memory:

- **`@solaris` self-reference** — built-in read-only documentation of the
  exact installed runtime: version/build identity, commands, config
  surface, capabilities, sandbox profiles, registered tools, Godot
  capability status, references/research configuration, and Task Runtime
  concepts, with a stable runtime revision. Retrieve it on demand with
  the `self.read` / `self.search` tools, `/solaris`, or `solaris --self`.
  It contains no secrets and has no mutation surface.
- **`solaris --doctor [area] [--json] [--report-safe]`** (interactive:
  `/doctor [area]`) — deterministic read-only diagnostics over ten areas
  (runtime, configuration, providers, sandbox, workspace, godot, project,
  references, research, capabilities), with per-check timeouts, honest
  fail-closed sandbox reporting, and documented exit codes (0 = no
  failures, 1 = one or more failures, 2 = invocation error; warnings
  never fail). Default operation is offline, non-paid, and never mutates
  anything. `--json` emits a deterministic schema-versioned report;
  `--report-safe` emits a sanitized report for bug reports (paths,
  credentials, and source content excluded — NOT anonymous).
- The command catalog is the single source for the interactive command
  vocabulary and help; `/doctor` and `/solaris` are catalogued commands.

## Next planned milestone

The next narrow milestone is **Stage 3 milestone 8: Read-Only Godot Scene and
Resource Intelligence**. It adds deterministic, bounded parsing and structured
inspection for `.tscn`, `.tres`, UID/resource relationships, scene inheritance,
node ownership, script attachments, signals, project settings, and autoloads.
It remains strictly read-only: no scene/resource mutation, project import, or
Godot launch is part of that milestone. The existing engine, mutation, command,
and development execution surfaces remain intentionally unavailable until an
identity-bound host primitive can enforce them. See `ROADMAP.md`.
