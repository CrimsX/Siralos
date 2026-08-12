# Engineering

These are the engineering standards for the Siralos repository. They apply from the first commit. Architecture and quality rules that can be checked mechanically are checked mechanically.

## Capability status conventions

This document distinguishes three states, consistently with `README.md`, `SECURITY.md`, and `ARCHITECTURE.md`:

- **Surface implemented**: contracts, tools, commands, and tests exist.
- **Available**: the capability executes end to end in the shipped product.
- **Intentionally unavailable**: the surface exists but every entry point fails closed and reports `unavailable` â€” nothing executes, no approval is requested, and availability is never claimed.

At this stage the read-only workspace tools are available; Git inspection is unavailable (the adapter requires Siralos-owned private run directories, whose creation and cleanup fail closed â€” Git can only ever execute inside an enforcing sandbox backend and is never spawned outside it); workspace mutations, `/undo`, and command execution are intentionally unavailable; Godot discovery and static project inspection are available, but Godot engine probing is intentionally unavailable (see below).

## Logic and UI separation

The terminal interface is an adapter. It parses input, renders output, and composes dependencies. It must not own conversation policy, provider behaviour, application state transitions, persistence, or future Godot behaviour.

The application layer is usable without the terminal UI: `createSiralosApplication({ provider })` runs headlessly and is exercised directly by tests.

```text
Terminal input
    â†“
Parsed user intent
    â†“
Application API
    â†“
Provider port
    â†“
Application events
    â†“
Terminal rendering
```

## Inward dependency direction

Core application logic must not depend on infrastructure or UI implementations:

```text
CLI â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â†’ Core
 â”‚
 â””â”€ Composition â”€â”€â”€â†’ Adapters â”€â”€â”€â†’ Core ports
```

Core must not import the CLI, adapters, test utilities, or Node infrastructure modules. `npm run check:architecture` fails the build on violations; it is a developer guardrail that uses structural TypeScript parsing plus regex/text checks, not an OS security boundary.

## Strict TypeScript

- `strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `noImplicitOverride`, `noImplicitReturns`, `noFallthroughCasesInSwitch`, and `useUnknownInCatchVariables` are enabled in `tsconfig.base.json`.
- Avoid `any`. Use `unknown` at untrusted boundaries.
- Do not use unchecked type assertions to bypass design problems.
- Exported APIs have explicit return types.
- Type-only imports use `import type` (`verbatimModuleSyntax` is enforced).
- The repository targets Node.js 24 and ESM; relative imports carry `.js` extensions.

## Runtime validation

Validate data that crosses genuinely untrusted boundaries. Provider-generated tool input is untrusted: every tool performs runtime validation of its `unknown` input before any work happens. Internally produced data does not need runtime validation. No provider SDK is trusted to validate on the harness's behalf.

## Security properties

These properties are established and tested; weakening any of them is an architecture change requiring explanation:

- **Sandbox and permission policy are separate**: a permission decision gates whether an operation proceeds; a sandbox profile defines the technical restrictions under which it runs. Approval never means unrestricted execution.
- **Fail closed**: when the requested policy cannot be enforced the process does not run; there is no silent host fallback, no weakened profile, no network enablement, and no unrestricted backend.
- **Workspace containment**: no tool can read outside the canonicalized launch directory. Requested paths are resolved relative to the workspace root, canonicalized, and checked against the canonical root; symlink escapes, parent traversal, absolute paths, null bytes, and prefix-confusion paths are rejected.
- **Read-only operation**: the workspace tools perform no intentional writes.
- **No shell**: tools never invoke a shell or external command; development commands are bound to Siralos-owned runners with structured arguments (never provider-supplied shell strings, executables, or environments) through the sandbox backend â€” both runners fail closed as unavailable today, so no command executes; the architecture check rejects `shell: true`, `exec`, `execSync`, and `spawnSync` in runtime code.
- **No network**: sandboxed child processes never get network access; all built-in profiles deny outbound traffic. Provider API networking stays in the host process.
- **Credential isolation**: child environments come from an explicit allowlist with deny patterns (`*_API_KEY`, `*_TOKEN`, `*_SECRET`, `*_PASSWORD`, `AWS_*`, `AZURE_*`, `GOOGLE_*`, `GITHUB_TOKEN`, `GH_TOKEN`, `SSH_AUTH_SOCK`, `NPM_TOKEN`, `NODE_AUTH_TOKEN`, provider keys, `SIRALOS_CONFIG`, `NODE_OPTIONS`, Git override variables, npm user-config and script-shell variables, proxy variables), applied case-insensitively; `process.env` is never forwarded. Sandbox home and temp values are controlled; commands get per-run private home, temp, and npm cache.
- **Explicit capability registration**: only tools constructed in the composition root are available; there is no dynamic tool loading; command runners come from an immutable explicit registry and providers can never register runners.
- **Untrusted input validation**: all provider-generated tool arguments are validated at runtime by the selected tool.
- **Untrusted output classification**: file contents and command output remain tool data (`tool_result` conversation items) and never become system or developer instructions; command output is never parsed as tool calls.
- **Bounded execution**: directory listings, file reads, searches, provider tool rounds, command arguments, npm scripts, package files, sandboxed process output (1 MiB hard limit per stream, 256 KiB provider-visible window with truncation markers), and execution time (default 120 s, maximum 10 minutes) have explicit limits with truncation metadata.
- **Cancellation**: long searches and sandboxed processes stop promptly when aborted; process trees terminate including descendants that ignore normal termination; a cancellation is never reported as completion.
- **Safe failures**: unknown tools, invalid arguments, path escapes, binary files, oversized files, duplicate call ids, filesystem errors, sandbox denials, and backend unavailability produce typed failures rather than crashes. A nonzero command exit is a completed command result, never an infrastructure failure.
- **Project configuration cannot broaden access**: sandbox configuration is user-level only (`~/.siralos/config.json`); an untrusted repository cannot enable network access, add writable roots, change backends, or disable environment filtering.
- **Approval is separate from sandbox enforcement**: an approval means "apply this exact prepared mutation or run this exact prepared command once" â€” never unrestricted execution, never a session grant, never a sandbox expansion. Write tools and `process.run` are hidden from the provider when the capability policy denies the capability; under `develop-offline`, `workspace.write` and `process.execute` are `ask` in policy. At this stage neither mutations nor commands ever reach approval: every mutation entry point and both command runners fail closed as `unavailable` before any write, approval, or checkpoint, so no approval for mutations or commands is ever requested.
- **Mutation conflict safety**: `workspace.read` returns complete-file SHA-256 hashes; edits and deletions require the exact expected hash; targets are revalidated immediately before mutation; stale or racing changes return `conflict` and never overwrite newer content. Replacement commits and rollbacks use an exclusive absence-preserving primitive (hard link, failing on `EEXIST`; a rename is never used to commit or restore), so a target appearing after the quarantine displacement is never overwritten and rollback conflicts return an explicit uncertain result preserving the quarantine. Creation verifies every parent component's identity immediately before the exclusive open and proves the created object's identity before writing any byte. **This design is not offered at this stage**: every mutation entry point fails closed as `unavailable` before any write, approval, or checkpoint, because Node offers no directory-relative (openat/renameat) primitive and a same-user process can swap a parent or target at any instruction boundary; the machinery above is tested internal code. Commands record the script SHA-256 before approval and revalidate it (plus the trusted executable identity and the full plan digest) before execution; any change is a `conflict`. No command executes at this stage: both the `node-script` and `npm-script` runners fail closed as unavailable under the pinned runtime â€” the pinned Node runtime cannot bind execution to the approved script bytes (the script can reach internal surfaces such as `process.binding` (e.g. `spawn_sync`) to spawn an unconstrained interpreter, and the staged private copy can be substituted by a same-user process in the verify-to-launch window), so every command request is refused with `unavailable` before any approval.
- **Complete previews before mutation**: every mutation produces a deterministic bounded unified diff shown before approval; truncated or oversized previews cannot be approved. The command-approval preview shows the complete npm script body (bounded at preparation, never truncated before approval), every argument boundary, and every execution boundary â€” no command reaches approval at this stage because both runners fail closed as unavailable, and no mutation reaches approval because every mutation entry point fails closed as `unavailable` before preparing anything.
- **Workspace immutability verification for commands**: Git structured status is compared before and after execution; a detected workspace change marks a sandbox violation and disables further commands for the session (the OS sandbox remains the security boundary).
- **Command serialization**: commands and approved file mutations share one in-process lock; a second command waits, and no mutation can begin while a command runs.

Tool output limits live in one discoverable module: `WORKSPACE_LIMITS` in `packages/adapters/src/tools/workspace/limits.ts` (directory entries 200, readable file size 512 KiB, returned read content 64 000 chars, search file size 512 KiB, files scanned 500, search matches 100, returned line length 400 chars, text file size 1 MiB, created content 512 KiB, replacements 32, replacement text 64 KiB, complete diff 256 KiB, diff lines 10 000). Command limits live in `COMMAND_LIMITS` in `@siralos/core` (arguments 64 / 8 KiB each / 64 KiB total, package.json 1 MiB, npm script 32 KiB, Node script 4 MiB, timeout 120 s default / 10 min max, stdout and stderr hard limits 1 MiB, provider return window 256 KiB, output event 16 KiB). Development-workflow limits live in `DEVELOPMENT_LIMITS` in `@siralos/core` (1 concurrent workflow, 16 files per change set, 512 KiB complete diff, 4 MiB resulting bytes, 32 replacements per file, 3 repair proposals, 4 iterations, 30 s parser timeout, 30 s LSP startup, 2 min validation budget per iteration, 15 min total budget). Quality limits live in `QUALITY_LIMITS` in `@siralos/core` (50 review findings, 4096 chars per evidence/impact/recommendation field, 256-char titles, 1 MiB review-context diff chunked by complete file, 3 total review rounds, 2 review-repair rounds, 120 s review timeout, 30-line warning tolerance, 160-char long-line advisory, 100 convention findings, 200 warning-delta entries). The provider tool-round limit is `DEFAULT_MAX_TOOL_ROUNDS` (8) in `@siralos/core`. The approval timeout is `DEFAULT_MAX_PENDING_APPROVAL_MS` (10 minutes) in `@siralos/core`. Sandbox process limits come from the active profile (`timeoutMs`, `maxOutputBytes`); the backend is pinned exactly (`@anthropic-ai/sandbox-runtime@0.0.70`) and isolated behind the `SandboxBackend` port. See `SECURITY.md` for the threat model and platform support details.

## Godot engine profiling pipeline

> **Status note (fail-closed).** Engine probing is **intentionally unavailable at this stage**: the probe runner reports `unavailable` for every probe and never spawns the executable, because the backend re-opens the staged copy's pathname at spawn time and a same-user process can substitute bytes between final verification and launch (no exec-by-handle primitive). Discovery and validation (stages 1â€“2 below) work; the probes stage (3) never executes, so no profile can be produced, classification and selection cannot run against a real probe result, and `godot.inspect_engine` cannot return a profile. The engine-profile cache is an **explicitly unavailable no-op component** at this stage: it is never initialized, created, read, or written (`load()` is always a miss, `store()` returns a typed unavailable result, `count()` is 0, and the doctor reports it disabled) â€” the earlier storage implementation was removed rather than retained as an unsafe surface. Static project inspection (below) works and rescans the complete bounded project on every inspection â€” no profile cache is used. The pipeline below documents the design for a future mechanically identity-bound launch primitive.

Godot discovery happens before any project execution and follows a fixed pipeline: **discovery â†’ validation â†’ probes â†’ classification â†’ selection â†’ cache**. Each stage is bounded and cancellable; a cancelled probe kills the Godot process tree, removes run directories and partial dumps, caches nothing, and returns to the prompt.

1. **Discovery**: configured user installations (absolute paths only, with optional edition hints `standard`/`dotnet`/`unknown`) plus a fixed-name PATH search (`godot.exe`, `godot4.exe`, `godot-mono.exe`, `godot4-mono.exe` on Windows; `godot`, `godot4`, `godot-mono`, `godot4-mono` elsewhere). No broad filesystem scanning, no registry/Spotlight/package-database searches, no `where`/`which`, no shell. Windows PATHEXT is honored safely (only `.exe` is ever appended; `.bat`/`.cmd` never considered). macOS `.app` bundles resolve to their exact `Contents/MacOS` executable (CFBundleExecutable from the XML Info.plist with a "Godot" fallback) and are never launched via `open` or Apple Events. At most 16 candidates are considered.
2. **Validation**: exact executable fingerprints â€” canonical path, size, mtime, SHA-256 (512 MiB size bound). Before every probe the complete bounded SHA-256 is recomputed and the file type and canonical identity re-verified â€” size+mtime alone are never treated as identity. Execution binds to the verified bytes: the probe stages a verified private executable copy inside its run directory and executes only that copy, never the mutable configured path. Executables inside the project workspace are rejected by default.
3. **Probes** (project-independent, through the sandbox backend under the internal `godot-probe-offline` profile): exactly `--version` (10 s, 64 KiB output), `--help` (15 s, 2 MiB), and `--dump-extension-api` (120 s), each constructed by the single private `fixedProbeArguments` constructor in the probe adapter. Never `--path`/`--upwards`/`--import`/`--scene`/`--script`/`--editor`; the workspace path appears nowhere in the request (executable, arguments, working directory, environment, or sandbox configuration) and the profile excludes the workspace from readable roots; the request carries an explicit empty read-roots list so only the run directory and required system runtime paths are granted. stdin closed, network denied, per-run home/temp, no provider or project credentials, process tree confined, fail-closed unless the backend reports available with host-read, write, network, and process-tree restriction. The probe working directory is a Siralos-private run directory under `~/.siralos/runs`; the API dump lands there and is deleted after parsing (exactly `extension_api.json` is expected; unexpected files ignored; symlinked output rejected; verified success requires exit code exactly zero and a raw-byte fingerprint). Only bounded metadata â€” header version, API hash, class/builtin-class/global-enum/utility-function counts, configuration format version, size, SHA-256 â€” is kept; the complete dump never enters provider context and is not persisted.
4. **Classification**: adversarial version parsing (release channels stable/rc/beta/alpha/dev/custom/unknown preserved; prereleases never normalized to stable; control characters sanitized; empty/non-Godot/non-numeric major-minor output fails). Edition classification is conservative: filename `mono` is never proof of .NET; explicit hint + filename + `--build-solutions` + API features + probe success combine, conflicting evidence lowers confidence and is reported, uncertain â†’ unknown, and a help probe without any editor signal is a runtime-only heuristic that is never selected. Siralos support classification is never taken from the internet: exact 4.7.1 stable standard = verified; other stable 4.7 standard = compatible-untested; 4.7/4.8 prereleases and dev builds = prerelease-untested; custom builds = custom-build-untested; Godot 3 = unsupported-major; .NET = compatible-untested; runtime-only = runtime-only.
5. **Selection**: precedence is `--godot-path`, `--godot-installation`, `SIRALOS_GODOT`, `SIRALOS_GODOT_INSTALLATION`, `godot.activeInstallation`, preferred compatible PATH candidate, no selection. CLI/env path and id are mutually exclusive at the same level; an explicit selection that fails never falls back silently. Deterministic ranking within compatible candidates: verified baseline (4.7.1 stable standard) > compatible stable standard > compatible stable .NET > prerelease editor; stable over prerelease, standard over .NET, higher patch within a tested minor line, deterministic path tie-breaker. Godot 3.x and runtime-only binaries are never auto-selected. The rationale is recorded and shown by `/godot-installations`.
6. **Cache (design)**: the intended engine-profile cache at `~/.siralos/godot/engine-profiles` would be keyed by executable SHA-256, bounded to 32 entries, with atomic metadata writes, symlinked cache paths rejected, user-level only, storing only bounded normalized data â€” never credentials, complete dumps, project files, or absolute project paths â€” with an executable hash change invalidating the entry, and no provider tool able to delete or modify it. At this stage the cache is an explicitly unavailable no-op (see the status note): nothing is stored, read, created, or removed. Static project inspection uses no cache at all â€” every inspection rescans the complete bounded project.

### Milestone limits

| Limit                            | Value   |
| -------------------------------- | ------- |
| Discovery candidates             | 16      |
| Executable size                  | 512 MiB |
| Version probe output             | 64 KiB  |
| Help probe output                | 2 MiB   |
| API dump                         | 128 MiB |
| Version probe timeout            | 10 s    |
| Help probe timeout               | 15 s    |
| API dump timeout                 | 120 s   |
| Project file (`project.godot`)   | 4 MiB   |
| Plugin descriptor                | 256 KiB |
| GDExtension descriptor           | 1 MiB   |
| Files scanned (language profile) | 50,000  |
| Inspected source bytes           | 64 MiB  |
| Static scan time                 | 30 s    |
| Cache entries                    | 32      |

Provider input cannot raise these limits and user config cannot disable them; truncation is explicit and reported. Capability probing is token-matched exactly against a fixed set (complete option tokens, never substrings); advertised capabilities and operationally verified capabilities are distinct (`verifiedCapabilities`/`degradedCapabilities`), and `--dump-extension-api-with-docs` and `--doctool` are never invoked.

### Compatibility assessment

Major mismatch or an engine minor older than declared = error; same minor = compatible (never guaranteed); newer minor = likely-compatible warning. .NET project + standard engine = edition-mismatch; GDScript + .NET engine = likely-compatible warning. Custom/prerelease engines = engine-unverified; missing declared version = project-version-unknown. Every assessment explains itself.

### Opt-in live conformance

`npm run test:godot` runs live Godot probe conformance against a real engine on the host, but only when opted in: without `SIRALOS_TEST_GODOT="<absolute-path>"` the suite refuses to run or skips loudly â€” it never pretends a skipped or unavailable probe passed. **At this stage probing fails closed, so the suite reports UNAVAILABLE loudly and never passes, on any platform, with or without `SIRALOS_TEST_GODOT` set**; a skipped or unavailable result is never treated as a live security pass. The live suite never modifies the user-supplied engine: all timeout, cancellation, descendant-termination, stdin, and identity-invalidation probes run against disposable fixed fixtures. The live Git helper-confinement test proves clean-filter execution, repository-write denial, and network denial **only when it actually runs against a real enforcing sandbox and observes the denial** through the sandbox-private result channel: the filter connects to a controlled loopback endpoint proven reachable from an unsandboxed preflight client and kept listening through the run, and the denial is attributed to sandbox enforcement by that controlled comparison (bounded failure, never a connection or timeout, no server-side connection) â€” never by a socket error alone. A skipped or unavailable sandbox is reported as skipped, never as passed. `npm run test:sandbox` on this machine (Windows) skips loudly because the backend requires the one-time `npx sandbox-runtime windows-install` setup and cannot enforce the host-read allowlist; a skip is never treated as a pass. Rerun commands: `npm run test:sandbox` after the Windows setup completes; `SIRALOS_TEST_GODOT="<absolute-path>" npm run test:godot` (reports UNAVAILABLE until probing becomes available); `SIRALOS_TEST_GODOT="<absolute-path>" npm run test:godot-recovery` (verifies the fail-closed recovery behavior — capability unavailable with a precise reason, preparation refuses before approval, nothing created, nothing executed — and reports the live engine-isolation probe as skipped, never passed, while execution is unavailable).

`SIRALOS_TEST_GODOT="<absolute-path>" npm run test:godot-diagnostics` verifies the fail-closed GDScript diagnostic behavior the same way: capability unavailable with a precise reason, preparation refuses before approval, nothing created, nothing executed, and the live engine-isolation probe reported skipped, never passed.

## Godot API knowledge and GDScript diagnostics pipeline

> **Status note (fail-closed).** Exact-engine API knowledge generation and GDScript check-only diagnostics are **implemented as contracts, adapters, static preparation, and truthful reporting — with execution unavailable on every platform at this stage**. The API documentation runner (`--dump-extension-api-with-docs`) and the check-only runner (`--headless --path <disposable-mirror> --script <mirror-script> --check-only`) **never spawn the executable** and return typed `unavailable` outcomes; the knowledge cache is an explicitly unavailable no-op (never initialized, created, read, or written); no approval is requested while execution is unavailable; nothing is created or deleted (ADR 0010). The designed pipeline below becomes operational only after identity-bound execution primitives exist.

**Knowledge pipeline (design):** `--dump-extension-api-with-docs` runs in a Siralos-private probe directory (no project, no `--path`, workspace excluded from readable roots, network denied, credentials absent, stdin closed, bounded output) → exactly `extension_api.json` (symlinks rejected, size-bounded) is parsed only after successful generation → the bounded deterministic index is built (classes, methods, properties, signals, constants, enums, utility functions, built-in classes, operators; deterministic symbol identities with `#N` overload ordinals; descriptions truncated to the immutable bound) → the profile (executable SHA-256, dump SHA-256, schema version) is stored only after a complete successful generation → search/lookup serve bounded structured results. A knowledge profile must never silently survive an executable fingerprint change.

**Diagnostics pipeline (design):** static preparation validates workspace-relative `.gd` paths (lexical + canonical containment, symlinks and non-regular files rejected, size-bounded) or enumerates the project deterministically (exclusions, symlink skipping, file-count and total-byte limits) and hashes every target → the risk manifest is refreshed → the prepared-check digest binds script hashes, manifest digest, fixed command digest, sandbox profile, and limits → one-time approval → one disposable mirror per run → strictly sequential `--check-only` invocations → conservative output normalization (engine-version fixtures; unmatched error-like lines preserved; line/column never fabricated; control characters sanitized; mirror paths never leak) → deterministic aggregation with explicit truncation → workspace-integrity verification → mirror cleanup. A script parse failure is a valid `checked` result, never an infrastructure failure.

## Godot GDScript LSP client

> **Status note (fail-closed).** The GDScript language-session surface is **implemented as contracts, transport, normalization, and truthful reporting — with session startup unavailable on every platform at this stage**. The LSP runner never spawns the editor, no mirror is created, no port is opened, and no approval is requested while execution is unavailable (ADR 0011). The designed pipeline below becomes operational only after identity-bound execution primitives exist.

**Session pipeline (design):** refresh risk manifest → one-time approval bound to the immutable plan (risk manifest, executable identity/version, mirror-copy policy, LSP capability set, sandbox profile, LSP policy version, limits) → prepare and verify the disposable mirror → allocate a loopback-only dynamic port (127.0.0.1, OS-assigned, race-safe) → start `<godot> --headless --editor --recovery-mode --path <mirror> --lsp-port <port>` → connect → `initialize` (rootUri = mirror, minimal client capabilities) → `initialized` → ready. Every stage is bounded and cancellable; startup failure terminates Godot, closes the socket, and cleans the mirror. The transport is standard LSP framing with an incremental parser (bounded headers/bodies, malformed lengths rejected deterministically) over a bounded JSON-RPC client (pending bound, timeouts, cancellation, safe late/duplicate handling, MethodNotFound for unsupported server requests, `workspace/applyEdit`/`workspace/executeCommand` never implemented). Diagnostics/hover/completion/definition are normalized conservatively into the provider-neutral models (mirror URIs to workspace-relative paths, 1-based positions, bounded fields, markup as data, insertText never applied). Sessions are single, bounded (30 s startup / 15 s requests / 10 min idle / 30 min lifetime / 5 s shutdown), staleness-revalidated before every query, and shut down gracefully with source-workspace verification and mirror cleanup.

## GDScript development workflow

> **Status note (fail-closed).** The GDScript development workflow is **implemented as contracts, change-set machinery, orchestration, and truthful reporting — with the change-set applier unavailable on every platform at this stage**. The applier's platform gate fails closed before any write, lock, approval, or checkpoint, because Node offers no directory-relative (openat/renameat) primitive; the workflow refuses before any approval for a mutation and no checkpoint is ever created (ADR 0012). The orchestration below is tested internal code exercised through injected in-memory file primitives, scripted language/parser services, and the real filesystem checkpoint store; it becomes operational only after identity-bound commit primitives exist.

**Workflow pipeline:** `/develop <request>` prepares and one-time-approves a workflow start bound to the request text, the project authored-file fingerprint, the engine fingerprint, and the immutable limits (1 concurrent workflow, 16 files per change set, 512 KiB complete diff, 4 MiB resulting bytes, 3 repair proposals, 4 iterations, 30 s parser timeout, 30 s LSP startup, 2 min validation budget per iteration, 15 min total budget). The authorization covers only the read-only validation context — LSP recreation after approved edits, `--check-only` parsing, API lookup, workspace and Git inspection. The provider investigates with the read-only tools, then proposes an exact text change set (`workspace.apply_text_changeset`): bounded create/edit/delete on UTF-8 text files with exact current SHA-256 preconditions, complete deterministic diffs, and an immutable digest; each change set (including every repair) requires its own exact one-time approval. On apply: the language session is suspended (`closing_for_edit`; a failed suspension never applies), every precondition is revalidated, every affected file is checkpointed (with its exact pre-change bytes; an absence state for creates) before anything is written, files are applied sequentially with post-state hash verification under the mutation lock, changed `.gd` scripts run the fixed `--check-only` invocation sequentially, a fresh disposable mirror and language session are recreated (engine fingerprint unchanged, project delta exactly the approved change sets), LSP diagnostics settle deterministically (initial receipt, bounded quiet period, hard timeout), and bounded validation evidence is recorded (parser, LSP, Git status when available, workspace integrity with unexpected-change detection — unexpected external changes are reported truthfully, never reverted). A partial application failure triggers hash-gated recovery from the just-created checkpoints (`apply_failed_recovered` / `apply_failed_partial_recovery` / `apply_failed_uncertain`; never success after partial application). Validation errors enter the bounded repair loop (each repair approved separately; denial and cancellation preserve accepted changes). Validation infrastructure failures keep the approved source changes and end the workflow `validation_failed` — the source is never blamed for an infrastructure failure, and a skipped gate is never presented as success.

## Development quality gates and independent review

> **Status note (fail-closed).** The quality stage (ADR 0013) lives inside the development workflow, whose change-set applier fails closed as unavailable on every platform; the stage, the reviewer plumbing, the validation executor, and the report machinery are tested internal code exercised through injected fakes, and in the shipped product no quality stage, review, or validation command runs. The opt-in `npm run test:godot-quality` conformance verifies this truthfully and always reports the live quality-stage isolation probe as skipped, never passed.

- **Deterministic gates are authoritative; the reviewer is a separate reasoning signal.** Parser checks, LSP diagnostics, hash verification, source-integrity checks, sandbox enforcement, and test exit codes are application-computed and can never be replaced or weakened by a reviewer. Gates are hard (block clean completion), soft (advisories), or informational; the classification is fixed, not provider-configurable. A required gate that could not run is `validation_incomplete`, never `passed`.
- **Warning policy is conservative.** Warnings surfaced as errors by Godot or project policy are hard errors; normal new warnings are advisory; pre-existing warnings are never attributed to the change unless evidence shows the change caused them; attribution that cannot be proven is labelled uncertain. `@warning_ignore` is never inserted and project warning configuration is never modified.
- **Conventions respect the project.** The read-only analyzer checks only new/modified lines with high-confidence rules; local file conventions take precedence over Godot fallback guidance; static typing is never forced globally; findings are advisory unless explicitly repository-mandatory.
- **Validation commands still require one-time process approval.** Discovered `check`/`test`/`lint`/`typecheck` scripts are untrusted project content: exact command and full script body are shown, one-time approval binds the exact plan, the workspace stays read-only, network stays denied, stdin stays closed; denial or unavailability is `validation_incomplete`; a missing test runner is `not_applicable`, never an infrastructure failure.
- **The reviewer is read-only and isolated.** Fresh provider context per review, a composition-root-owned read-only tool registry, no approval/execution/mutation surface, runtime-validated bounded structured output, and structural architecture enforcement (the quality/reviewer adapter cannot import mutation, process, checkpoint, sandbox, or environment adapters; deterministic gates cannot import reviewer implementations; reviewer construction is composition-root owned).
- **Repairs are separately approved and bounded.** Confirmed blockers are repaired through exact-change-set approval, checkpoints, parser/LSP revalidation, applicable test checks, and a fresh holistic review; at most 2 review-repair rounds; remaining blockers end as `completed_with_blocking_findings`.
- This is not the general multi-agent framework: exactly one reviewer, no voting/consensus/debate, no provider-to-provider conversations, no autonomous approval, no persistent review history.

## Explicit dependency composition

Concrete dependencies are created in exactly one composition root (`apps/cli/src/bootstrap/create-application.ts`) by direct manual composition:

```ts
const application = createSiralosApplication({ provider });
```

No dependency-injection container, decorator injection, service locator, runtime reflection, or abstract factory.

## Small cohesive modules

One concrete responsibility per module. Names state the responsibility (`parse-input`, `interactive-session`, `deterministic-fake-provider`). Avoid vague names such as `Manager`, `Helper`, `Utils`, or `Common`.

## No circular dependencies

Module-level circular imports are prohibited. Workspace dependency cycles are detected by the architecture check and by TypeScript project references.

## No unnecessary abstractions

Build the smallest coherent implementation that satisfies the current requirements. A future requirement is not a current implementation requirement. Do not add speculative ports, generic frameworks, or compatibility layers.

## Tests at meaningful boundaries

Tests exercise public behaviour through public APIs: the application API, the provider port, the input parser, and the interactive session. Git tests use temporary local repositories (no global identity, no remotes); checkpoint and undo tests use temporary stores outside the workspace. Tests are deterministic, fast, independent of network access and credentials, and free of arbitrary sleeps.

## Formatting and linting

- Prettier for formatting (`npm run format`, `npm run format:check`).
- ESLint with type-aware `typescript-eslint` rules for linting (`npm run lint`).
- Stylistic ESLint rules that duplicate or conflict with Prettier are not added.
- `npm run check` runs all non-mutating validation and never rewrites files.

## Rust engineering (Stage 3R, ADR 0032)

All Rust code follows the authoritative Siralos Rust Style & Engineering
Guide (`docs/development/RUST_STYLE.md`) — this section is only a
pointer, not a duplicate:

- Formatting: rustfmt with the repository-owned `rustfmt.toml`
  (max_width 79); `cargo fmt --all --check` is a required gate.
- Linting: Clippy with warnings denied
  (`cargo clippy --workspace --all-targets --all-features -- -D warnings`).
- `unsafe` is forbidden (`unsafe_code = "forbid"`), edition 2024 and a
  pinned toolchain are required, and `npm run check:rust` enforces crate
  shape, dependency direction, binary identity, and core domain
  neutrality.
- The TypeScript implementation is the behavioral reference; Rust
  migrations preserve behavior while deliberately improving structure.

## Simplicity over speculative flexibility

Choose the simple design now; generalize when a real second consumer appears. The fake provider is an adapter because provider neutrality is an actual requirement, not because an adapter layer was fashionable.

## Clear naming

Names identify one concrete responsibility. Discriminated unions carry the event vocabulary; plain functions and data carry the behaviour. Classes are used only where they provide a clear benefit.

## Removal of dead or replaced code

Dead code, replaced modules, and unused exports are removed. Build output (`dist/`) is generated, ignored by Git, and never committed.

## Architecture changes require explanation

Architecture changes are recorded in `docs/adr/` and reflected in `ARCHITECTURE.md`. A change that weakens a mechanical check must come with a test that proves the new boundary.

## `/evolve` must never silently weaken gates

A future `/evolve` workflow may not weaken engineering, architecture, validation, or security gates without explicit human review. This rule stands even if a later stage implements self-improvement tooling.

## Task runtime rules

1. **Every authoritative mutable domain has one owner.** TaskState is owned
   exclusively by the core `TaskRuntime`; CLI, providers, adapters, and UI
   receive immutable snapshots, projections, or events. Provider adapters
   cannot import the task runtime surface (architecture-enforced).
2. **Model assertions are not authoritative evidence.** "Done" in provider
   text is never a transition; steps complete only on host-validated
   evidence references, and a model-issued `complete` disposition still
   passes the host completion gate.
3. **Task completion is host verified.** `completeTask` refuses unless the
   gate passes: steps completed, acceptance criteria satisfied, validation
   and review clean, no unresolved critical/high findings. Stage 2 quality
   gates remain authoritative; infrastructure failures stay
   `validation_incomplete`, never criterion failure and never success.
4. **Behavior-changing agent-runtime modifications require behavior
   evidence.** Changes to the task runtime, completion gate, progress
   semantics, or the `/develop`-task integration must extend the
   `tests/behavior/` fixtures (behaviors 1–15) rather than relying on unit
   tests alone.
5. **Final-boundary effects are tested.** Behavior/security-sensitive
   changes are verified at the final observable boundary (task phase,
   activity log, workspace contents, checkpoint list), not only through an
   intermediate helper's return value.
6. **Task configuration is snapshotted at task start.** The immutable
   `TaskRuntimeSnapshot` captures the reproducibility-relevant identities
   (runtime version, provider profile, sandbox profile, capability-policy
   fingerprint, workspace identity, engine fingerprint, workflow
   identity/digest); ordinary global config changes affect future tasks
   only.
7. **Activity logs are append-only records, not competing state.** The typed
   `TaskActivityEvent` log is for auditability, debugging, persistence, UI
   projection, and tests — never a second authoritative TaskState, and never
   a generic event bus.
8. **Security policy cannot be broadened by task state.** TaskContract,
   TaskState, and dispositions are descriptive/control-flow state; they
   grant no capabilities. Approvals, sandboxing, capability policy, path
   containment, and checkpoint integrity stay authoritative elsewhere.

## Planning rules (Stage 3 milestone 7, ADR 0020)

1. **Planning depth is selected by deterministic host policy.** The
   `PlanningPolicy` routes `none | light | full` from host-visible task
   facts; the model never classifies complexity and the provider never
   chooses depth (architecture-enforced).
2. **Simple tasks must not incur planner cost without reason.** `none`
   routing never invokes a planner provider — the existing Task Runtime
   path runs directly.
3. **Planner capabilities are structurally read-only.** The planner
   registry has no mutation/process/approval/checkpoint/undo tools, the
   executor refuses every non-read-only tool at the runtime boundary, and
   the ToolProjector `planning` mode hides anything else from the
   provider-visible schema.
4. **Plans are immutable and revisioned.** A material change produces rev
   N+1; rev N is never mutated and stays inspectable. Plan identity is
   host-assigned.
5. **Plans bind to a TaskContract revision.** A plan created against rev
   N is stale the moment the contract moves to N+1; stale plans are never
   silently executed.
6. **Verified touchpoints require evidence; guesses remain candidates.**
   A verified touchpoint records the exact inspected workspace revision
   handle; candidate touchpoints are explicitly unverified.
7. **Full-plan execution requires explicit acceptance criteria.** At
   least two criteria with one host-verifiable criterion, or mutation
   execution is blocked with a precise reason.
8. **Plan approval binds to the exact plan revision.** Advancing the plan
   revision or the TaskContract revision invalidates the prior approval;
   the runtime refuses stale approvals.
9. **Plan approval never authorizes source edits or commands.** Mutations
   and commands keep their own exact one-time approval paths; plan
   requirements are descriptive and grant nothing.
10. **Security/capability policy outranks plan content.** The plan model
    has no capability surface, and policy-shaped claims in plan text are
    rejected at validation.
11. **TaskState, not TaskPlan, owns execution progress.** Plan steps are
    proposed structure; they never become competing mutable progress.
12. **Planner private reasoning is not authoritative state.** Planner and
    executor use separate provider contexts; only the validated structured
    plan (current revision) enters TaskState, projection, or activity.
13. **Planning behavior requires final-boundary tests.** Behavior
    fixtures 1–35 and effect tests 47–51 cover routing, read-only
    enforcement, immutability, approval binding, staleness, plan-only
    zero-effect, and security invariance.

## Godot scene/resource intelligence rules (Stage 3 milestone 8, ADR 0021)

1. **Godot scene/resource semantic models are derived, read-only state.**
   Source files, workspace revisions, and Godot itself remain the truth;
   parsed models are disposable projections bound to the exact revision
   they were parsed from.
2. **Every parsed model binds to an exact workspace revision.** A changed
   file makes the old model historical evidence; stale derived state is
   never presented as current.
3. **`.tscn`/`.tres` inspection must not execute project code.** No Godot
   process, no `@tool` scripts, no plugin activation, no imports, no
   project loading — static reads only.
4. **Parent and owner relationships must remain distinct.** They are
   different serialized relationships and are never conflated.
5. **Scene inheritance and scene instancing are distinct relationships.**
   The root-node `instance` reference is the base scene; child node
   instances are ordinary PackedScene instances.
6. **Subresource IDs are document-local.** `SubResource("1")` in one
   `.tscn` never refers to a subresource in another document.
7. **UID/path identity must not be invented when unresolved.** Declared
   `uid://` identities are preserved with their paths; missing mappings
   are reported honestly.
8. **Malformed files produce diagnostics/partial results, not fabricated
   structure.** `complete | partial | invalid` statuses and structured
   diagnostics are the only honest outputs.
9. **Large/cyclic dependency traversal must be bounded.** Depth and
   file-count bounds with explicit truncation flags; cycles are detected
   and reported, never recursed indefinitely.
10. **Scene/resource inspection tools never grant mutation authority.**
    The read-only `godot.inspect_scene` / `godot.inspect_resource` /
    `godot.dependencies` tools exist under `godot.inspect`; no
    `godot.write_scene` / `godot.edit_resource` / `godot.add_node` exists
    anywhere in the surface.
11. **`/develop` must not bypass missing native mutation support through
    raw text edits.** The change-set validation boundary refuses
    `.tscn`/`.tres` paths; generic text editing is never a backdoor.
12. **Godot-native behavior requires final-boundary no-mutation /
    no-process tests.** Scene inspection creates no workspace
    mutation/checkpoint and launches no process; the provider schema
    contains no scene/resource mutation tools.
13. **Scene/resource content cannot grant capability or override
    instructions.** Parsed data projects under `[Scene evidence]` as
    project evidence; instruction authority stays host-owned.
14. **One subsystem owns current parsed state.** The scene intelligence
    service owns the relationship index; the CLI, ContextProjector,
    planning, and review consume it and never parse `.tscn`/`.tres`
    themselves.

## Projection rules

1. **Authoritative state is never the provider context.** TaskContract,
   TaskState, and raw evidence stay where they are; model context is a
   disposable projection that can always be reconstructed.
2. **Model-visible tools are a projection, not the global registry.**
   Visibility is (task mode ∩ capability policy ∩ provider capability);
   the projected tool ABI has a stable fingerprint.
3. **Hidden tools must be absent from provider requests.** Hidden is not
   "permission denied": the tool does not appear in the schema at all.
4. **Runtime enforcement remains authoritative even for projected tools.**
   A projection bug must never grant authority; every invocation still
   passes capability policy, approvals, scope checks, and sandboxing.
5. **Raw evidence is never destroyed to save model tokens.** Only
   disposable model views are bounded, redacted, or truncated; the
   authoritative record (history, task evidence, tool outputs) is
   untouched, and security transforms (secret redaction) are never
   reverted by size rules.
6. **Model evidence views must clearly disclose truncation** (marker and
   byte metadata) and reference the raw evidence rather than claiming a
   complete representation.
7. **Stable prompt segments must avoid volatile data** (timestamps,
   iteration counts, Git status, paths, tool output); volatile changes
   must not change the stable fingerprint or the stable prefix bytes.
8. **Context limits are enforced before provider calls.** The working
   budget is authoritative over advertised maximums; hard pressure blocks
   the invocation, auto pressure performs deterministic pair-preserving
   reduction, and provider rejection is never used as flow control.
9. **Async results must be revision-bound; stale results are discarded.**
   Every async projection/helper result carries the revision it was
   computed for and is dropped when state has advanced; tests use
   deterministic fake scheduling, never sleeps.
10. **Context/result caches use high/low watermarks** instead of threshold
    thrashing, and model-context eviction never deletes durable evidence
    required by TaskState.
11. **Behavioral/security-sensitive projection changes require
    final-boundary tests**: the actual provider request (tools absent),
    the actual provider invocation (or its absence at hard pressure), and
    the actual task state after projection.

## Workspace revision rules

1. **Every workspace observation that may influence mutation is
   revision-identifiable.** Exact/structural/summary reads, evidence, and
   validation results state the `rev_...` handle of the file state they
   concern.
2. **Text mutation binds to an exact cryptographic file state.** The
   change set carries either the raw SHA-256 or a revision handle the host
   resolves to its SHA-256; both are revalidated against the current file
   at prepare and apply time.
3. **Opaque revision handles are convenience identifiers, not authority.**
   Possession grants no read/write/approval/path access; capability policy
   and containment stay authoritative.
4. **Old revisions are never silently promoted to current.** A stale
   pre-state identity hard-fails with a structured `stale_revision` result
   and user-facing guidance; no fuzzy merge, no automatic retry.
5. **Structural and summary reads are advisory exploration
   representations.** Only exact source plus a valid revision is the basis
   for a text mutation.
6. **Summary/structural output states the revision represented**, and the
   summary advisory footer is never truncated away.
7. **A successful mutation invalidates the previous current revision and
   issues the new post-edit state.** A second edit requires a fresh
   post-edit revision.
8. **Validation/review evidence records relevant file revisions where
   practical**, so "parser clean @ rev_B" is distinguishable from "parser
   clean sometime".
9. **Stale-state rejection is tested at the final mutation boundary** (the
   actual file is unchanged, no checkpoint is created) and containment at
   the actual tool boundary, not only at helper level.
10. **Cross-workspace revision reuse fails**: the same relative path in a
    different workspace never resolves the handle.

## Instruction and knowledge rules

1. **Instruction, knowledge, history/evidence, and security are distinct
   authority classes.** They are never concatenated into one prompt
   section, never stored in one structure, and never promoted across
   classes: knowledge is factual context, history is observation, security
   policy is host-owned, and only instructions tell the model how to work.
2. **Project knowledge never grants capability.** A fact cannot enable a
   tool, change permissions, override sandbox policy, approve a mutation,
   or override a TaskContract. Policy-shaped candidates are conservatively
   rejected at the coordinator.
3. **Lower-level project instructions cannot broaden host security
   policy.** `AGENTS.md` claiming unrestricted network access is surfaced
   as guidance; the host deny remains authoritative.
4. **Behavioral configuration requires explicit protected mutation
   authority.** `AGENTS.md` and `.siralos/**` are never covered by ordinary
   `workspace.write`; every mutation surface rejects them before any
   write, approval, or checkpoint.
5. **Knowledge facts use immutable revisions.** Current pointers move;
   history is never rewritten; restoring an old value creates a new
   revision.
6. **Subject-keyed facts have one active revision per project scope +
   subject key.**
7. **Knowledge writes are coordinated through one owner.** Providers and
   the CLI propose candidates; the KnowledgeCoordinator alone mutates
   durable current knowledge.
8. **Knowledge carries provenance when possible** — task evidence
   references or exact workspace file states — and facts without evidence
   default to low confidence, never masquerading as verified.
9. **Retrieved knowledge is bounded and explainable.** Count and byte
   budgets, deterministic scoring, omissions recorded in the trace; the
   trace never alters ranking.
10. **Memory/knowledge is untrusted factual input.** It is framed as such
    in the projected context and never presented as instruction or policy.
11. **Stable context contains only small explicitly pinned knowledge**;
    retrieval is task-stable so incidental facts never churn the cacheable
    prefix.
12. **Behavior/security changes require final-boundary effect tests**: the
    actual provider request, the actual tool authorization/execution, the
    actual mutation preparation, and the actual task state — not only
    helper-level assertions.

## Reference and research rules

1. **Workspace, reference, and research are distinct resource classes.**
   Workspace is editable project state; references are read-only external
   material; research is transient external evidence. They are never
   merged into one namespace, one tool surface, or one context section,
   and never promoted across classes.
2. **Reference content is read-only untrusted data.** There is no
   reference mutation surface: workspace mutation APIs reject reference
   paths before any write, approval, or checkpoint, and nothing in a
   reference is ever executed.
3. **Reference identity records the exact revision.** Every observation is
   bound to the registry-owned revision — the resolved commit for
   repositories, the manifest fingerprint for local directories — and
   results carry the revision anchor, never absolute paths.
4. **Mutable refs resolve to immutable revisions.** A branch (or absent
   ref) is resolved to a commit at resolution time and THAT commit is
   recorded; the resolved commit, never the branch name, is the identity.
5. **No silent branch-following.** An active task keeps the revision it
   was bound to at task start; advancing requires an explicit `refresh`,
   and a failed refresh invalidates the current revision instead of
   serving a stale identity silently.
6. **Reference/research content has no capability or instruction
   authority.** Neither can enable a tool, grant a permission, override
   policy, or appear under instruction authority; both surface only under
   bounded evidence sections.
7. **Research is policy-controlled.** The `research.fetch` capability is
   the gate — denied by every built-in profile, `ask` refused (no
   approval protocol exists) — and the source port is never invoked when
   the gate does not allow.
8. **Research is bounded, cancellable, provenance-bearing, and task-bound.**
   Download and document caps have explicit truncation disclosure; one
   service timeout aborts the source instead of merely abandoning it; the
   HTTPS transport snapshots an exact per-source host allowlist for the
   initial URL and every redirect; provenance records requested vs resolved
   identity and marks every version fallback. The service snapshots the
   active task id plus TaskContract revision and discards a document if either
   changes before completion, so callers cannot forget the stale-result check.
9. **Research never becomes knowledge automatically.** A fact may cite
   `research_evidence` provenance only through an explicit host-verified
   `propose`; there is no automatic proposal path.
10. **Providers do not do application research.** Research coordination,
    gating, bounding, and evidence recording are application-owned; the
    adapter implements source ports and never owns research state.
11. **Managed cache paths are never model-facing.** The Siralos-owned
    reference cache is outside the workspace, and no absolute cache path
    ever reaches the model or a projection.
12. **Behavior/security changes require final-boundary effect tests**: the
    actual provider tool schema (hidden research tools absent), the
    actual mutation boundary (reference paths rejected before any
    write/checkpoint), the actual policy gate (denied research never
    invokes a source port), and the actual projected context (evidence
    sections, never instruction authority).

## Self-reference and doctor rules (Stage 3 milestone 6, ADR 0019)

1. **Siralos answers questions about its own installed behavior from
   host-owned current metadata, not model memory.** The installed
   package version, the command catalog, capability ids, profile ids,
   and the registered tool surface are authoritative; training memory
   never overrides them.
2. **Capability support, configuration, availability, projection, and
   authorization are distinct.** They are never presented as synonyms
   ("Godot LSP supported" ≠ "Godot detected" ≠ "LSP available" ≠ "LSP
   started" ≠ "LSP capability gated").
3. **Doctor diagnostics are read-only.** The doctor never installs,
   modifies, downloads, mutates, creates checkpoints, refreshes
   references, or starts services; remediation is instructions only.
4. **The default doctor is offline and non-paid.** No network requests,
   no live probes, no provider calls, no engine launches.
5. **Live external probes require explicit user action** and are not
   implemented at this milestone.
6. **Diagnostic reports never include secret values.** Checks report
   credential names and presence only; the safe report additionally
   strips absolute paths, source content, and credential-shaped tokens.
7. **ToolProjector remains authoritative for model-visible tools.** The
   doctor queries it through the sources port; doctor/self modules never
   import projection internals or capability-resolution machinery
   (architecture-enforced).
8. **SandboxBackend remains authoritative for enforcement capability.**
   Required enforcement failures report `fail`, and no unrestricted
   fallback is ever reported or used.
9. **The doctor does not silently repair or broaden permissions.** A
   snapshot is observation; it grants nothing.
10. **Current task runtime snapshots are diagnostic facts**, never
    mutable through the doctor.
11. **Expensive diagnostics are opt-in.** Anything that scans, hashes,
    launches, or fetches is skipped or reported available-on-demand.
12. **The self-reference and doctor surfaces require final-boundary
    tests** for no-mutation, no-secret, and no-network behavior.
