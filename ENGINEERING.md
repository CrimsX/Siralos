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

Validate data that crosses genuinely untrusted boundaries. Internally produced data does not need runtime validation. There are no trusted runtime boundaries in the current slice beyond the provider port, which is core-owned and compiled against.

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
