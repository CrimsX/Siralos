---
id: ADR-0032
status: accepted
domains: [architecture, process]
paths: [crates/**, docs/development/RUST_STYLE.md]
supersedes: []
---

# ADR 0032 — Siralos Rename and Rust Migration

- Status: accepted (Stage 3R — R1)
- Date: current milestone
- Related: ADR 0001 (modular monolith), ADR 0028 (content identity),
  ADR 0029 (determinism and reproducibility), ADR 0030 (interpretable
  context architecture)

## Problem

The project identity is "Solaris", but the product direction is a
deterministic, security-first, context-efficient software-development
and QA harness with a domain-neutral core and explicitly installed
optional domain intelligence. Two problems follow:

1. The "Solaris" identity must be replaced by the canonical identity
   "Siralos" throughout the project so that no project-owned occurrence
   of the old identity remains.
2. The TypeScript implementation cannot satisfy the product's long-term
   requirements for type safety, ownership, error semantics, memory
   behavior, and deterministic concurrency. Siralos needs an idiomatic
   Rust implementation, migrated deliberately rather than translated
   line for line.

## Decision

### Rename

- The canonical project identity is **Siralos**; the canonical CLI is
  `siralos`; the canonical environment prefix is `SIRALOS_`; the
  canonical user state directory is `~/.siralos`; the canonical npm
  scope and self-reference identity is `@siralos`.
- Project-owned `Solaris`, `SOLARIS`, and `solaris` identities (and
  derived forms such as `solaris-*`, `SOLARIS_*`, `.solaris`, and
  `@solaris`) are renamed. Unrelated external terminology is excluded
  (none currently exists in this repository).
- No indefinite aliases for the old identity are introduced; this is a
  clean rename.
- The rename is protected by a deterministic repository-level check
  (`npm run check:identity`), with narrow documented exclusions only for
  the verification mechanism itself and its tests.

### Migration model

- The TypeScript implementation is the **Siralos TypeScript behavioral
  reference implementation**: an executable behavioral specification and
  the migration oracle. It is kept, renamed to Siralos, and its
  observable behavior is preserved. The Rust implementation is the
  **Siralos Rust candidate implementation**. There is one product:
  Siralos.
- Behavioral parity is explicitly distinguished from structural parity:
  migrations preserve observable behavior, invariants, authority
  boundaries, security guarantees, protocols, and acceptance criteria
  while deliberately improving type safety, ownership, API design,
  module boundaries, error semantics, memory behavior, concurrency
  structure, maintainability, testability, and evidence-justified
  performance.
- Every migrated subsystem undergoes a refactoring review: abstractions
  that exist only because TypeScript/Node encouraged them (unnecessary
  service objects, single-implementation interfaces, pass-through
  wrappers, stringly typed maps, unnecessary async, Node-inherited
  concurrency) are removed or redesigned idiomatically.
- Optimization is evidence-driven and speculative optimization is
  prohibited; performance work may never weaken security, determinism,
  or correctness.
- The TypeScript implementation is retired only by later Stage 3R
  milestones after differential verification.

### Rust workspace

- A Rust workspace exists with exactly three crates:

  ```text
  crates/siralos-core      domain-neutral host semantics and types
  crates/siralos-adapters  infrastructure/adapters ownership
  crates/siralos-cli       the `siralos` binary (composition boundary)
  ```

- Dependency direction: `siralos-cli` → `siralos-adapters` →
  `siralos-core`. Core depends on nothing; adapters may depend on core;
  the CLI may depend on core and adapters. Core must never depend on
  infrastructure or on a domain implementation.
- **Domain neutrality is enforceable**: `siralos-core` contains no
  Godot-specific implementation or semantic types and compiles with the
  Godot domain completely absent. A repository-level architecture check
  (`npm run check:rust`) enforces crate shape, dependency direction,
  binary identity, edition policy, formatting policy, toolchain policy,
  domain-neutral core symbols, and the unsafe-Rust prohibition.
- Rust edition 2024, an explicit pinned toolchain
  (`rust-toolchain.toml`), a repository-owned `rustfmt.toml`
  (max_width 79, small heuristics max), and workspace lints
  (`unsafe_code = "forbid"`, `missing_docs = "deny"`,
  `broken_intra_doc_links = "deny"`) are standard. Warnings are errors.
- **Unsafe Rust is forbidden** in the new foundation; any future
  exception requires an evidenced requirement and the isolation,
  documentation, testing, and benchmarking protocol in the Rust
  Engineering Guide.
- A canonical **Siralos Rust Style & Engineering Guide**
  (`docs/development/RUST_STYLE.md`) is authoritative for all Rust code.
- No placeholder domains, marketplace, plugin ecosystem, automatic
  Godot acquisition, or hypothetical domain crates (Rust/Web/Python/
  Unity/Unreal) exist.

### Optional domain product policy

- The permanent policy: Godot is **not installed by default, not
  enabled by default, not auto-detected for installation, not
  auto-recommended, and not auto-downloaded**. The user must explicitly
  request it (conceptually `siralos domains install godot`). No
  marketplace or generalized plugin ecosystem is implemented.
- Domain direction: `Godot Domain → Siralos Core APIs`; never the
  reverse.

## Consequences

- Project-owned content uses only the Siralos identity; the identity
  ratchet fails the build on regressions.
- The TypeScript implementation remains the behavioral oracle and keeps
  passing its own validation; snapshot changes after the rename are
  deliberate identity effects and were reviewed, not bulk-regenerated.
- Rust code is held to the style guide by fmt, Clippy with warnings
  denied, and the architecture ratchet; the `siralos` binary builds and
  runs with no Godot package present.
- Later Stage 3R milestones port subsystems under the
  behavior-extraction → idiomatic-redesign → parity → review →
  measurement → differential-verification pipeline; R1 does not port
  major subsystems.
- Stage 4 (runtime and visual QA) and later stages are unchanged in
  product terms but are reached after Stage 3R.
