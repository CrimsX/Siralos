# Siralos

Siralos is a deterministic, security-first, domain-neutral software-development
and QA harness that places host-owned validation, policy, evidence, and
controlled effects around probabilistic model reasoning.

[![Rust CI](https://github.com/CrimsX/Siralos/actions/workflows/rust.yml/badge.svg)](https://github.com/CrimsX/Siralos/actions/workflows/rust.yml)
[![CodeQL](https://github.com/CrimsX/Siralos/actions/workflows/codeql.yml/badge.svg)](https://github.com/CrimsX/Siralos/actions/workflows/codeql.yml)

> **Status: active development / Rust migration.** The broad TypeScript
> implementation is the current behavioral reference. Rust is the successor
> implementation, migrated under differential verification. Stage 3R R2 is
> complete; **R3 — Domain-Neutral Core** is next and has not begun.

## Overview

Models can reason and propose, but they do not own Siralos state or authority.
The host controls the path from proposal to evidence:

```text
model proposal
    ↓
host validation
    ↓
policy and capability enforcement
    ↓
controlled effect
    ↓
verification and evidence
```

The core is provider-neutral and domain-neutral. Optional domains add
specialized intelligence without gaining host capabilities. Godot Engine is
the first and currently only optional domain being developed for Siralos.

## Why Siralos?

- Deterministic host decisions around probabilistic reasoning
- Explicit, fail-closed authority and capability boundaries
- Revision-bound plans, changes, and approvals
- Bounded context, tools, outputs, and lifecycle operations
- Evidence, provenance, replay, and differential verification
- Provider-neutral architecture with optional domain intelligence

The detailed ownership model and invariants live in
[ARCHITECTURE.md](ARCHITECTURE.md) and [SECURITY.md](SECURITY.md).

## Current capabilities

### Available now

- An interactive TypeScript CLI with a deterministic, offline fake provider
- Bounded read-only workspace listing, reading, searching, and structural reads
- Host-owned task state, acceptance evidence, context projection, and read-only
  planning
- Project instructions, knowledge, local references, self-reference, and
  capability diagnostics
- Static Godot project, scene, and resource intelligence without opening or
  executing a project
- The R2 differential harness: versioned fixtures, bounded TypeScript and Rust
  runners, semantic comparison, replay checks, and machine-readable parity
  evidence

### In migration or planned

- The Rust successor currently covers the Stage 3R foundation and R2 harness;
  the first domain-neutral core port is R3
- Workspace mutation, undo, Git inspection, development commands, and dynamic
  Godot execution remain intentionally unavailable where the current host
  cannot mechanically enforce the required identity guarantees
- Real provider integrations, additional domains, and Stage 4 runtime/visual QA
  are future milestones

Unavailable effects return typed failures before execution, approval,
checkpoint creation, or cleanup. See the [roadmap](ROADMAP.md) for exact status.

## Architecture

The repository contains two implementations during Stage 3R:

```text
TypeScript behavioral reference
  apps/cli → packages/adapters → packages/core

Rust successor
  siralos-cli → siralos-adapters → siralos-core
```

Both cores own domain-neutral policy and contracts. Adapters own infrastructure
and optional domain behavior. The CLI is the composition and terminal boundary.
Godot does not define or appear in the Rust core.

See the [architecture index](docs/architecture/README.md),
[ADR 0032](docs/adr/0032-rust-migration-and-siralos-rename.md), and
[ADR 0033](docs/adr/0033-differential-behavioral-harness.md).

## Getting started

### Prerequisites

- Node.js 24 (`.nvmrc`; CI uses 24.17.0)
- npm 11.13.0
- Rust 1.97.1 through `rust-toolchain.toml`
- Git

### Bootstrap

```bash
git clone https://github.com/CrimsX/Siralos.git siralos
cd siralos
npm ci
```

For development or a new coding-agent session, read [AGENTS.md](AGENTS.md) and
the [project context](docs/development/PROJECT_CONTEXT.md) before selecting
milestone work. No separate conversational handoff is required.

### Run

Launch the TypeScript behavioral reference:

```bash
npm run siralos
```

Build and inspect the Rust successor CLI:

```bash
cargo build --locked
cargo run --locked --bin siralos -- --version
cargo run --locked --bin siralos -- --help
```

### Verify

Run the standard local repository quality gate:

```bash
npm run check
```

This covers formatting, linting, TypeScript build and tests, architecture,
identity and public-hygiene ratchets, documentation links, R2 differential
parity, Rust formatting, Clippy with warnings denied, and Rust tests.

Run only the authoritative R2 parity decision:

```bash
npm run check:differential
```

## Optional Godot support

Godot Engine is Siralos's first optional specialization. The TypeScript
reference can statically inspect Godot projects, scenes, and resources without
executing project code. Dynamic engine probes and project execution remain
fail-closed where their security properties cannot be enforced.

The Rust Godot domain package is not implemented yet. Siralos does not install
Godot, silently enable a domain, or acquire a domain merely because
`project.godot` exists. The domain package and the Godot Engine installation are
separate, explicit concerns.

## Development status

- Public product stages 1–3 have a broad TypeScript reference surface; effectful
  capabilities retain truthful availability reporting
- Stage 3R R1 and R2 are complete
- **Next:** Stage 3R R3 — Domain-Neutral Core
- Stage 4 has not begun

The [Rust migration register](docs/development/RUST_MIGRATION.md) is the
authoritative R1–R12 sequence. The [Stage 4 entry gate](docs/development/stage4-entry-gate.md)
records what remains before runtime and visual QA work can start.

## Documentation

- [Project context](docs/development/PROJECT_CONTEXT.md) — complete public-safe
  development bootstrap and current implementation reality
- [Architecture](ARCHITECTURE.md) — ownership, dependency direction, and system
  design
- [Architecture index](docs/architecture/README.md) — subsystem-to-code and ADR
  map
- [Security model](SECURITY.md) — threat model, fail-closed boundaries, and
  platform caveats
- [Roadmap](ROADMAP.md) — public stages and current capability status
- [Engineering guide](ENGINEERING.md) — implementation rules and validation
  conventions
- [Rust migration](docs/development/RUST_MIGRATION.md) — Stage 3R sequence and
  porting gate
- [Rust style guide](docs/development/RUST_STYLE.md) — authoritative Rust
  engineering standard
- [Normative requirements](docs/requirements/REQUIREMENTS.md) — CORE, HAR, and
  anti-pattern registers with current evidence status
- [RFC ownership](docs/architecture/RFC_INDEX.md) and
  [golden traces](docs/development/GOLDEN_TRACES.md) — decision and verification
  work-item registries
- [Architecture decisions](docs/adr/) — accepted and historical ADRs

## Security

The model cannot grant itself authority. Effects are host-controlled, approvals
bind only to exact prepared operations, and missing enforcement fails closed.
Repository content, provider output, and tool output are untrusted data—not
policy. Read [SECURITY.md](SECURITY.md) before changing an authority or process
boundary.

## License

No project license has been published yet.
