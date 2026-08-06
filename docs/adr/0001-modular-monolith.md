# ADR 0001: Modular monolith

Status: accepted

## Context

Solaris must grow from an interactive foundation into a Godot development harness with UI, application logic, provider integrations, Godot integration, persistence, and optionally multi-agent workflows. Early choices for process topology and module boundaries shape every later stage.

Options considered:

- A single package with loose module boundaries: cheapest, but allows UI/application/infrastructure coupling to creep in without mechanical resistance.
- A modular monolith: one repository, one npm workspace, multiple clearly separated packages with enforced dependency direction, deployed as one process.
- Distributed orchestration: separate processes, message buses, daemons, or microservices. Maximum flexibility, maximum operational complexity, premature for the current product.

## Decision

Build Solaris as a modular monolith:

- One npm workspace repository with `apps/*` and `packages/*`.
- `@solaris/core` owns application behaviour and external contracts (ports), imports no Node infrastructure, adapters, or UI code.
- `@solaris/adapters` implements core-owned ports.
- `@solaris/cli` is a terminal input/output adapter.
- Exactly one composition root creates concrete dependencies by direct manual composition.
- Package boundaries and dependency direction are enforced by a small custom script (`scripts/check-architecture.mjs`), not by a large framework.
- TypeScript project references give the workspace real build boundaries and build order.

## Consequences

Positive:

- UI, application logic, and infrastructure stay separable without operational complexity.
- The provider port in core keeps provider neutrality mechanical rather than aspirational.
- Tests run against public boundaries (application API, provider port, input parser, session loop) in one fast, offline process.
- Later stages (Godot integration, persistence, agents) can be added as new core-owned ports with adapters without restructuring.

Negative:

- One process means everything shares the same failure domain; a crash takes the whole harness down. That is acceptable for an interactive developer tool.
- Boundaries are convention plus a custom check rather than OS-level isolation; the check must be maintained as the workspace grows.
- Cross-package refactoring requires build coordination, handled by project references.

## Alternatives deferred

- Distributed orchestration, additional processes, a daemon, or an internal RPC protocol are deferred until a real requirement (for example a long-running Godot runtime process with separate lifecycle needs) demonstrates the need.
- A DI container or service locator is rejected: explicit composition in one obvious root is simpler to read and debug.
