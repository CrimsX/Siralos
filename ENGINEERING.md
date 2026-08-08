# Engineering

These are the engineering standards for the Solaris repository. They apply from the first commit. Architecture and quality rules that can be checked mechanically are checked mechanically.

## Capability status conventions

This document distinguishes three states, consistently with `README.md`, `SECURITY.md`, and `ARCHITECTURE.md`:

- **Surface implemented**: contracts, tools, commands, and tests exist.
- **Available**: the capability executes end to end in the shipped product.
- **Intentionally unavailable**: the surface exists but every entry point fails closed and reports `unavailable` — nothing executes, no approval is requested, and availability is never claimed.

At this stage the read-only workspace tools are available; Git inspection is unavailable (the adapter requires Solaris-owned private run directories, whose creation and cleanup fail closed — Git can only ever execute inside an enforcing sandbox backend and is never spawned outside it); workspace mutations, `/undo`, and command execution are intentionally unavailable; Godot discovery and static project inspection are available, but Godot engine probing is intentionally unavailable (see below).

## Logic and UI separation

The terminal interface is an adapter. It parses input, renders output, and composes dependencies. It must not own conversation policy, provider behaviour, application state transitions, persistence, or future Godot behaviour.

The application layer is usable without the terminal UI: `createSolarisApplication({ provider })` runs headlessly and is exercised directly by tests.

```text
Terminal input
    ↓
Parsed user intent
    ↓
Application API
    ↓
Provider port
    ↓
Application events
    ↓
Terminal rendering
```

## Inward dependency direction

Core application logic must not depend on infrastructure or UI implementations:

```text
CLI ───────────────→ Core
 │
 └─ Composition ───→ Adapters ───→ Core ports
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
- **No shell**: tools never invoke a shell or external command; development commands are bound to Solaris-owned runners with structured arguments (never provider-supplied shell strings, executables, or environments) through the sandbox backend — both runners fail closed as unavailable today, so no command executes; the architecture check rejects `shell: true`, `exec`, `execSync`, and `spawnSync` in runtime code.
- **No network**: sandboxed child processes never get network access; all built-in profiles deny outbound traffic. Provider API networking stays in the host process.
- **Credential isolation**: child environments come from an explicit allowlist with deny patterns (`*_API_KEY`, `*_TOKEN`, `*_SECRET`, `*_PASSWORD`, `AWS_*`, `AZURE_*`, `GOOGLE_*`, `GITHUB_TOKEN`, `GH_TOKEN`, `SSH_AUTH_SOCK`, `NPM_TOKEN`, `NODE_AUTH_TOKEN`, provider keys, `SOLARIS_CONFIG`, `NODE_OPTIONS`, Git override variables, npm user-config and script-shell variables, proxy variables), applied case-insensitively; `process.env` is never forwarded. Sandbox home and temp values are controlled; commands get per-run private home, temp, and npm cache.
- **Explicit capability registration**: only tools constructed in the composition root are available; there is no dynamic tool loading; command runners come from an immutable explicit registry and providers can never register runners.
- **Untrusted input validation**: all provider-generated tool arguments are validated at runtime by the selected tool.
- **Untrusted output classification**: file contents and command output remain tool data (`tool_result` conversation items) and never become system or developer instructions; command output is never parsed as tool calls.
- **Bounded execution**: directory listings, file reads, searches, provider tool rounds, command arguments, npm scripts, package files, sandboxed process output (1 MiB hard limit per stream, 256 KiB provider-visible window with truncation markers), and execution time (default 120 s, maximum 10 minutes) have explicit limits with truncation metadata.
- **Cancellation**: long searches and sandboxed processes stop promptly when aborted; process trees terminate including descendants that ignore normal termination; a cancellation is never reported as completion.
- **Safe failures**: unknown tools, invalid arguments, path escapes, binary files, oversized files, duplicate call ids, filesystem errors, sandbox denials, and backend unavailability produce typed failures rather than crashes. A nonzero command exit is a completed command result, never an infrastructure failure.
- **Project configuration cannot broaden access**: sandbox configuration is user-level only (`~/.solaris/config.json`); an untrusted repository cannot enable network access, add writable roots, change backends, or disable environment filtering.
- **Approval is separate from sandbox enforcement**: an approval means "apply this exact prepared mutation or run this exact prepared command once" — never unrestricted execution, never a session grant, never a sandbox expansion. Write tools and `process.run` are hidden from the provider when the capability policy denies the capability; under `develop-offline`, `workspace.write` and `process.execute` are `ask` in policy. At this stage neither mutations nor commands ever reach approval: every mutation entry point and both command runners fail closed as `unavailable` before any write, approval, or checkpoint, so no approval for mutations or commands is ever requested.
- **Mutation conflict safety**: `workspace.read` returns complete-file SHA-256 hashes; edits and deletions require the exact expected hash; targets are revalidated immediately before mutation; stale or racing changes return `conflict` and never overwrite newer content. Replacement commits and rollbacks use an exclusive absence-preserving primitive (hard link, failing on `EEXIST`; a rename is never used to commit or restore), so a target appearing after the quarantine displacement is never overwritten and rollback conflicts return an explicit uncertain result preserving the quarantine. Creation verifies every parent component's identity immediately before the exclusive open and proves the created object's identity before writing any byte. **This design is not offered at this stage**: every mutation entry point fails closed as `unavailable` before any write, approval, or checkpoint, because Node offers no directory-relative (openat/renameat) primitive and a same-user process can swap a parent or target at any instruction boundary; the machinery above is tested internal code. Commands record the script SHA-256 before approval and revalidate it (plus the trusted executable identity and the full plan digest) before execution; any change is a `conflict`. No command executes at this stage: both the `node-script` and `npm-script` runners fail closed as unavailable under the pinned runtime — the pinned Node runtime cannot bind execution to the approved script bytes (the script can reach internal surfaces such as `process.binding` (e.g. `spawn_sync`) to spawn an unconstrained interpreter, and the staged private copy can be substituted by a same-user process in the verify-to-launch window), so every command request is refused with `unavailable` before any approval.
- **Complete previews before mutation**: every mutation produces a deterministic bounded unified diff shown before approval; truncated or oversized previews cannot be approved. The command-approval preview shows the complete npm script body (bounded at preparation, never truncated before approval), every argument boundary, and every execution boundary — no command reaches approval at this stage because both runners fail closed as unavailable, and no mutation reaches approval because every mutation entry point fails closed as `unavailable` before preparing anything.
- **Workspace immutability verification for commands**: Git structured status is compared before and after execution; a detected workspace change marks a sandbox violation and disables further commands for the session (the OS sandbox remains the security boundary).
- **Command serialization**: commands and approved file mutations share one in-process lock; a second command waits, and no mutation can begin while a command runs.

Tool output limits live in one discoverable module: `WORKSPACE_LIMITS` in `packages/adapters/src/tools/workspace/limits.ts` (directory entries 200, readable file size 512 KiB, returned read content 64 000 chars, search file size 512 KiB, files scanned 500, search matches 100, returned line length 400 chars, text file size 1 MiB, created content 512 KiB, replacements 32, replacement text 64 KiB, complete diff 256 KiB, diff lines 10 000). Command limits live in `COMMAND_LIMITS` in `@solaris/core` (arguments 64 / 8 KiB each / 64 KiB total, package.json 1 MiB, npm script 32 KiB, Node script 4 MiB, timeout 120 s default / 10 min max, stdout and stderr hard limits 1 MiB, provider return window 256 KiB, output event 16 KiB). The provider tool-round limit is `DEFAULT_MAX_TOOL_ROUNDS` (8) in `@solaris/core`. The approval timeout is `DEFAULT_MAX_PENDING_APPROVAL_MS` (10 minutes) in `@solaris/core`. Sandbox process limits come from the active profile (`timeoutMs`, `maxOutputBytes`); the backend is pinned exactly (`@anthropic-ai/sandbox-runtime@0.0.70`) and isolated behind the `SandboxBackend` port. See `SECURITY.md` for the threat model and platform support details.

## Godot engine profiling pipeline

> **Status note (fail-closed).** Engine probing is **intentionally unavailable at this stage**: the probe runner reports `unavailable` for every probe and never spawns the executable, because the backend re-opens the staged copy's pathname at spawn time and a same-user process can substitute bytes between final verification and launch (no exec-by-handle primitive). Discovery and validation (stages 1–2 below) work; the probes stage (3) never executes, so no profile can be produced, classification and selection cannot run against a real probe result, and `godot.inspect_engine` cannot return a profile. The engine-profile cache is an **explicitly unavailable no-op component** at this stage: it is never initialized, created, read, or written (`load()` is always a miss, `store()` returns a typed unavailable result, `count()` is 0, and the doctor reports it disabled) — the earlier storage implementation was removed rather than retained as an unsafe surface. Static project inspection (below) works and rescans the complete bounded project on every inspection — no profile cache is used. The pipeline below documents the design for a future mechanically identity-bound launch primitive.

Godot discovery happens before any project execution and follows a fixed pipeline: **discovery → validation → probes → classification → selection → cache**. Each stage is bounded and cancellable; a cancelled probe kills the Godot process tree, removes run directories and partial dumps, caches nothing, and returns to the prompt.

1. **Discovery**: configured user installations (absolute paths only, with optional edition hints `standard`/`dotnet`/`unknown`) plus a fixed-name PATH search (`godot.exe`, `godot4.exe`, `godot-mono.exe`, `godot4-mono.exe` on Windows; `godot`, `godot4`, `godot-mono`, `godot4-mono` elsewhere). No broad filesystem scanning, no registry/Spotlight/package-database searches, no `where`/`which`, no shell. Windows PATHEXT is honored safely (only `.exe` is ever appended; `.bat`/`.cmd` never considered). macOS `.app` bundles resolve to their exact `Contents/MacOS` executable (CFBundleExecutable from the XML Info.plist with a "Godot" fallback) and are never launched via `open` or Apple Events. At most 16 candidates are considered.
2. **Validation**: exact executable fingerprints — canonical path, size, mtime, SHA-256 (512 MiB size bound). Before every probe the complete bounded SHA-256 is recomputed and the file type and canonical identity re-verified — size+mtime alone are never treated as identity. Execution binds to the verified bytes: the probe stages a verified private executable copy inside its run directory and executes only that copy, never the mutable configured path. Executables inside the project workspace are rejected by default.
3. **Probes** (project-independent, through the sandbox backend under the internal `godot-probe-offline` profile): exactly `--version` (10 s, 64 KiB output), `--help` (15 s, 2 MiB), and `--dump-extension-api` (120 s), each constructed by the single private `fixedProbeArguments` constructor in the probe adapter. Never `--path`/`--upwards`/`--import`/`--scene`/`--script`/`--editor`; the workspace path appears nowhere in the request (executable, arguments, working directory, environment, or sandbox configuration) and the profile excludes the workspace from readable roots; the request carries an explicit empty read-roots list so only the run directory and required system runtime paths are granted. stdin closed, network denied, per-run home/temp, no provider or project credentials, process tree confined, fail-closed unless the backend reports available with host-read, write, network, and process-tree restriction. The probe working directory is a Solaris-private run directory under `~/.solaris/runs`; the API dump lands there and is deleted after parsing (exactly `extension_api.json` is expected; unexpected files ignored; symlinked output rejected; verified success requires exit code exactly zero and a raw-byte fingerprint). Only bounded metadata — header version, API hash, class/builtin-class/global-enum/utility-function counts, configuration format version, size, SHA-256 — is kept; the complete dump never enters provider context and is not persisted.
4. **Classification**: adversarial version parsing (release channels stable/rc/beta/alpha/dev/custom/unknown preserved; prereleases never normalized to stable; control characters sanitized; empty/non-Godot/non-numeric major-minor output fails). Edition classification is conservative: filename `mono` is never proof of .NET; explicit hint + filename + `--build-solutions` + API features + probe success combine, conflicting evidence lowers confidence and is reported, uncertain → unknown, and a help probe without any editor signal is a runtime-only heuristic that is never selected. Solaris support classification is never taken from the internet: exact 4.7.1 stable standard = verified; other stable 4.7 standard = compatible-untested; 4.7/4.8 prereleases and dev builds = prerelease-untested; custom builds = custom-build-untested; Godot 3 = unsupported-major; .NET = compatible-untested; runtime-only = runtime-only.
5. **Selection**: precedence is `--godot-path`, `--godot-installation`, `SOLARIS_GODOT`, `SOLARIS_GODOT_INSTALLATION`, `godot.activeInstallation`, preferred compatible PATH candidate, no selection. CLI/env path and id are mutually exclusive at the same level; an explicit selection that fails never falls back silently. Deterministic ranking within compatible candidates: verified baseline (4.7.1 stable standard) > compatible stable standard > compatible stable .NET > prerelease editor; stable over prerelease, standard over .NET, higher patch within a tested minor line, deterministic path tie-breaker. Godot 3.x and runtime-only binaries are never auto-selected. The rationale is recorded and shown by `/godot-installations`.
6. **Cache (design)**: the intended engine-profile cache at `~/.solaris/godot/engine-profiles` would be keyed by executable SHA-256, bounded to 32 entries, with atomic metadata writes, symlinked cache paths rejected, user-level only, storing only bounded normalized data — never credentials, complete dumps, project files, or absolute project paths — with an executable hash change invalidating the entry, and no provider tool able to delete or modify it. At this stage the cache is an explicitly unavailable no-op (see the status note): nothing is stored, read, created, or removed. Static project inspection uses no cache at all — every inspection rescans the complete bounded project.

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

`npm run test:godot` runs live Godot probe conformance against a real engine on the host, but only when opted in: without `SOLARIS_TEST_GODOT="<absolute-path>"` the suite refuses to run or skips loudly — it never pretends a skipped or unavailable probe passed. **At this stage probing fails closed, so the suite reports UNAVAILABLE loudly and never passes, on any platform, with or without `SOLARIS_TEST_GODOT` set**; a skipped or unavailable result is never treated as a live security pass. The live suite never modifies the user-supplied engine: all timeout, cancellation, descendant-termination, stdin, and identity-invalidation probes run against disposable fixed fixtures. `npm run test:sandbox` on this machine (Windows) skips loudly because the backend requires the one-time `npx sandbox-runtime windows-install` setup and cannot enforce the host-read allowlist; a skip is never treated as a pass. Rerun commands: `npm run test:sandbox` after the Windows setup completes; `SOLARIS_TEST_GODOT="<absolute-path>" npm run test:godot` (reports UNAVAILABLE until probing becomes available).

## Explicit dependency composition

Concrete dependencies are created in exactly one composition root (`apps/cli/src/bootstrap/create-application.ts`) by direct manual composition:

```ts
const application = createSolarisApplication({ provider });
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
