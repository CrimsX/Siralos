# Adapters domain (`@siralos/adapters`)

## Owner

- `packages/adapters` implements the provider, workspace, sandbox, Git,
  Godot, reference, research, and planning ports. It implements port
  contracts; it never imports `@siralos/core` internals beyond them.

## Rules

- Unavailable capabilities report `unavailable` truthfully — a skipped
  live probe is never a pass — and no surface claims availability its
  enforcement cannot back.
- Adapters never spawn processes outside an enforcing sandbox backend,
  and provider adapters never recreate briefing, planning, or
  documentation-selection semantics (architecture-checked).

## Relevant architecture

- `ARCHITECTURE.md` — Ports, Adapters, Security model, Tool loop
- `docs/architecture/README.md` — architecture index (domain map)

## Applicable ADRs

- ADR-0002 (provider-neutral tool loop), ADR-0004 (sandbox boundary),
  ADR-0006 (Git/checkpoints), ADR-0007 (validation runners), ADR-0008–0013
  (Godot discovery/knowledge/diagnostics/LSP/development/quality),
  ADR-0018 (references/research)
