# CLI domain (`apps/cli`)

## Owner

- `apps/cli` is the composition root and the only interactive terminal
  surface. It wires host-owned services; it never owns context-selection,
  briefing, or planning semantics.

## Rules

- The terminal sanitizer is the single output boundary, the input queue
  is the single interactive-read owner, and the command catalog is the
  single command-vocabulary source.
- New slash commands register in the command catalog and delegate to
  host-owned services; the CLI never re-implements application policy.

## Relevant architecture

- `ARCHITECTURE.md` — CLI, Composition root
- `docs/architecture/README.md` — architecture index (domain map)

## Applicable ADRs

- ADR-0013 (quality gates), ADR-0020 (planning), ADR-0022–0024 (executor
  context and scoped documentation)
