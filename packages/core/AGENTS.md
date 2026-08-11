# Core domain (`@solaris/core`)

## Owner

- `packages/core` owns application policy and domain contracts: the task
  runtime, projections, workspace revision models, instructions and
  knowledge, references and research contracts, planning, self-reference
  and doctor, and the executor-context surface. It must not import
  adapters.

## Rules

- Domain models are validated and detached at every runtime boundary;
  caller-owned mutable data is never retained.
- Executor-briefing modules (`src/executor`) are derived-context only:
  no capability, security, approval, network, or filesystem imports
  (architecture-checked). `WorkspaceScope`, documentation selection, and
  the context pack can never grant capability.
- Planning modules are host-routed and structurally read-only; plans are
  descriptive and never authorize edits or commands.

## Relevant architecture

- `ARCHITECTURE.md` — Core, Application layer, Task runtime, Context/tool/evidence projection
- `docs/architecture/README.md` — architecture index (domain map)

## Applicable ADRs

- ADR-0001 (modular monolith), ADR-0014 (task runtime), ADR-0015
  (projection), ADR-0016 (workspace revisions), ADR-0017
  (instructions/knowledge), ADR-0019 (self-reference/doctor), ADR-0020
  (planning), ADR-0021 (scene/resource intelligence), ADR-0022–0024
  (executor context)
