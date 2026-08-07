# Engineering

These are the engineering standards for the Solaris repository. They apply from the first commit. Architecture and quality rules that can be checked mechanically are checked mechanically.

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

Core must not import the CLI, adapters, test utilities, or Node infrastructure modules. `npm run check:architecture` fails the build on violations.

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
- **No shell**: tools never invoke a shell or external command; sandboxed commands are internally fixed and trusted.
- **No network**: sandboxed child processes never get network access; all built-in profiles deny outbound traffic. Provider API networking stays in the host process.
- **Credential isolation**: child environments come from an explicit allowlist with deny patterns (`*_API_KEY`, `*_TOKEN`, `*_SECRET`, `*_PASSWORD`, `AWS_*`, `AZURE_*`, `GOOGLE_*`, `GITHUB_TOKEN`, `GH_TOKEN`, `SSH_AUTH_SOCK`, `NPM_TOKEN`, `NODE_AUTH_TOKEN`, provider keys, `SOLARIS_CONFIG`); `process.env` is never forwarded. Sandbox home and temp values are controlled.
- **Explicit capability registration**: only tools constructed in the composition root are available; there is no dynamic tool loading.
- **Untrusted input validation**: all provider-generated tool arguments are validated at runtime by the selected tool.
- **Untrusted output classification**: file contents remain tool data (`tool_result` conversation items) and never become system or developer instructions.
- **Bounded execution**: directory listings, file reads, searches, provider tool rounds, sandboxed process output, and execution time have explicit limits with truncation metadata.
- **Cancellation**: long searches and sandboxed processes stop promptly when aborted; a cancellation is never reported as completion.
- **Safe failures**: unknown tools, invalid arguments, path escapes, binary files, oversized files, duplicate call ids, filesystem errors, sandbox denials, and backend unavailability produce typed failures rather than crashes.
- **Project configuration cannot broaden access**: sandbox configuration is user-level only (`~/.solaris/config.json`); an untrusted repository cannot enable network access, add writable roots, change backends, or disable environment filtering.
- **Approval is separate from sandbox enforcement**: an approval means "apply this exact prepared mutation once" — never unrestricted execution, never a session grant, never a sandbox expansion. Write tools are hidden from the provider when the capability policy denies writes; under `develop-offline`, `workspace.write` is `ask`.
- **Mutation conflict safety**: `workspace.read` returns complete-file SHA-256 hashes; edits and deletions require the exact expected hash; targets are revalidated immediately before mutation; stale or racing changes return `conflict` and never overwrite newer content.
- **Complete previews before mutation**: every mutation produces a deterministic bounded unified diff shown before approval; truncated or oversized previews cannot be approved.

Tool output limits live in one discoverable module: `WORKSPACE_LIMITS` in `packages/adapters/src/tools/workspace/limits.ts` (directory entries 200, readable file size 512 KiB, returned read content 64 000 chars, search file size 512 KiB, files scanned 500, search matches 100, returned line length 400 chars, text file size 1 MiB, created content 512 KiB, replacements 32, replacement text 64 KiB, complete diff 256 KiB, diff lines 10 000). The provider tool-round limit is `DEFAULT_MAX_TOOL_ROUNDS` (8) in `@solaris/core`. The approval timeout is `DEFAULT_MAX_PENDING_APPROVAL_MS` (10 minutes) in `@solaris/core`. Sandbox process limits come from the active profile (`timeoutMs`, `maxOutputBytes`); the backend is pinned exactly (`@anthropic-ai/sandbox-runtime@0.0.70`) and isolated behind the `SandboxBackend` port. See `SECURITY.md` for the threat model and platform support details.

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

Tests exercise public behaviour through public APIs: the application API, the provider port, the input parser, and the interactive session. Tests are deterministic, fast, independent of network access and credentials, and free of arbitrary sleeps.

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
