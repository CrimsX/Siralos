# Godot domain (`packages/adapters/src/godot`)

## Owner

- `packages/adapters/src/godot` owns Godot executable discovery, probes,
  API knowledge, diagnostics, the LSP client, the development-loop
  runners, and the scene/resource intelligence tooling. The static
  inspection semantics live in `packages/core/src/godot` (see
  `packages/core/AGENTS.md`).

## Rules

- Static inspection launches no Godot process and mutates no
  scene/resource; every derived semantic model binds to the exact source
  revision it was read from, and stale derived state is never current.
- Node parent relationships are never conflated with node ownership, and
  inheritance is never conflated with instancing.

## Relevant architecture

- `ARCHITECTURE.md` — Godot engine discovery and profiling, Godot API
  knowledge, GDScript language session, read-only scene/resource intelligence
- `docs/architecture/README.md` — architecture index (domain map)

## Applicable ADRs

- ADR-0008 (discovery/profiling), ADR-0009 (recovery probing), ADR-0010
  (knowledge/diagnostics), ADR-0011 (LSP), ADR-0012 (development loop),
  ADR-0013 (quality gates), ADR-0021 (read-only scene/resource
  intelligence)
