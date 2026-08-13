---
id: ADR-0035
status: accepted
domains: [runtime, architecture, domain-host]
paths: [packages/core/src/runtime/**, experiments/domain-abi/**, docs/development/RUST_MIGRATION.md]
supersedes: []
---

# ADR 0035 — Domain-Neutral Controlled Runtime Boundary

## Context

ADR 0031 established the host-owned readiness contracts for future runtime
execution and described their first consumer as “Controlled Godot Execution.”
ADR 0034 then made Godot an optional domain behind a host-mediated boundary.
Using the Godot-specific name for the generic execution capability would blur
that ownership line and encourage engine concepts to enter the host core.

## Decision

The Stage 4 host capability is named **Controlled Runtime Execution**. It is a
domain-neutral, host-authorized operation that supervises an explicitly
approved process and produces bounded structured Runtime Evidence.

**Godot Runtime Adapter** is the separate, optional specialization that maps
Godot runtime intent and observations onto that host capability. “Controlled
Godot Execution” may describe the composed Godot use case, but never the
generic core boundary or its authority model.

This terminology clarification does not supersede ADR 0031's readiness,
identity, budget, supervision, or evidence decisions. It narrows the ownership
of those contracts and aligns them with ADR 0034.

## Consequences

- `siralos-core` runtime contracts remain domain-neutral.
- Optional domains request host-mediated runtime operations; they never gain
  process, filesystem, network, or approval authority by being installed.
- Stage 4 implements and verifies the generic boundary before the Godot
  adapter, visual evidence, controlled interaction, QA, or profiling layers.
- R3–R12 migration work remains sequenced ahead of Stage 4; this ADR adds no
  runtime implementation to the current milestone.
