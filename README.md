# Siralos

Siralos is a minimal, declarative AI coding harness with an inspectable
execution environment.

Profiles define how the model works. Context shows what Siralos gives it. The
Host controls what it can do.

_Probabilistic reasoning. Deterministic execution._ The host places host-owned
validation, policy, evidence, and controlled effects around probabilistic
model reasoning, and stays small by moving sophistication into declarative
configuration, Skills, and explicitly installed Plugins
([ADR 0036](docs/adr/0036-lean-product-composition-and-extension-model.md)).

[![Rust CI](https://github.com/CrimsX/Siralos/actions/workflows/rust.yml/badge.svg)](https://github.com/CrimsX/Siralos/actions/workflows/rust.yml)
[![CodeQL](https://github.com/CrimsX/Siralos/actions/workflows/codeql.yml/badge.svg)](https://github.com/CrimsX/Siralos/actions/workflows/codeql.yml)

> **Status: active development / Rust migration.** The broad TypeScript
> implementation is the current behavioral reference. Rust is the successor
> implementation, migrated under differential verification. Stage 3R R4 —
> Generic Workspace / Project Foundation — is complete (bounded reads,
> listing, search, revisions, prepared-effect contracts, checkpoint
> inspection/reconciliation, and the typed unavailable Git disposition at
> differential parity), **R5 — Generic Language Intelligence** is
> complete (one-based positions/ranges, bounded sanitized diagnostics
> with deterministic ordering and explicit truncation, generic
> symbol/definition/reference models, the language-neutral structural
> representation with the deterministic advisory summary formatter,
> typed validation results, revision binding, and the generic
> language-service URI mapping at differential parity), and **R6 —
> Minimal Domain Capability Architecture and Synthetic Conformance
> Domain** is complete (the domain-neutral lifecycle/capability
> semantics in `siralos-core::domain`, the production Component Model /
> WIT boundary in `siralos-adapters::domain` with exact-byte package
> identity, fail-closed ABI versioning, resource bounds, trap
> containment, and host-mediated effects, and the deterministic
> product-neutral synthetic conformance Domain proving the boundary);
> **R7 — Provider, Tool-Loop, Projection, Configuration, and CLI
> Parity** is active: R7A behavior extraction and provider protocol
> remediation are complete, and **R7.1 — Provider contract +
> deterministic fake provider + bounded single model turn parity** is
> complete (corpus version 13, 120 scenario files, 18 `provider-turn`
> scenarios at differential parity); **R7.2 � Application Tool Loop
> parity** is complete (16 `tool-loop` scenarios at differential parity,
> including the authorization, displayInput UTF-16, and Tool-result
> status matrices).

## Status vocabulary

- **CURRENT** — implemented and verified in the repository today
  (the TypeScript behavioral reference, the Rust candidate foundation, and
  their verified surfaces).
- **TARGET** — committed product direction for a future stage
  (Profiles, portable locking, Context controls, Skills, Plugins, Tools,
  Views, Domains, Runs, Evolve). Target items are documented as direction;
  none are described as shipped until implemented.
- **FUTURE / NOT DUE** — deliberately not committed (general Hooks,
  built-in multi-agent frameworks, TaskGraph, generic workflow engines,
  plugin marketplaces, automatic acquisition, and similar). These may be
  reconsidered only from concrete demand and evidence
  ([ADR 0036](docs/adr/0036-lean-product-composition-and-extension-model.md)).

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

- Deterministic Host decisions around probabilistic reasoning
- Inspectable, provenance-bearing model Context
- Explicit capability and fail-closed authority boundaries
- Revision-bound, verifiable effects
- Portable declarative configuration as the future composition model
  (TARGET, not shipped)
- Evidence, replay, and differential verification
- Optional capability-scoped specialization rather than core feature growth

The detailed ownership model and invariants live in
[ARCHITECTURE.md](ARCHITECTURE.md) and [SECURITY.md](SECURITY.md). The lean
product, composition, and extension model is frozen in
[ADR 0036](docs/adr/0036-lean-product-composition-and-extension-model.md).

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

### In migration (CURRENT)

- The Rust successor covers the Stage 3R foundation, the R2 harness, the
  R3 domain-neutral host-owned task kernel (contracts, task state,
  evidence, acceptance, completion, activity, progress), the R4
  generic workspace/project foundation (workspace identity and path
  safety, bounded exact reads, deterministic listing and search, the
  revision registry, prepared-effect contracts, checkpoint model and
  inspection/reconciliation, and the typed unavailable Git disposition),
  and the R5 generic language-intelligence foundation (one-based source
  positions/ranges, bounded sanitized diagnostics with deterministic
  ordering and explicit truncation, generic symbol/definition/reference
  models, the language-neutral structural representation with the
  deterministic advisory summary formatter, typed validation results,
  R4 revision binding, and the generic language-service URI mapping)
  at differential parity with the TypeScript reference
- Workspace mutation, undo, Git inspection, development commands, and dynamic
  Godot execution remain intentionally unavailable where the current host
  cannot mechanically enforce the required identity guarantees

### Target (not yet implemented)

- Profiles, portable `siralos.toml`/`siralos.lock` semantics, full Context
  controls (Live / Pinned / Frozen), Skills and the Skill Creator, Plugins,
  Tools, Views where justified, optional Domains, and the measured
  `/evolve` workflow are TARGET concepts in their owning stages
  (ADR 0036) — none are implemented
- Real provider integrations, additional domains, and Stage 4 runtime/visual QA
  are future milestones

Unavailable effects return typed failures before execution, approval,
checkpoint creation, or cleanup. See the [roadmap](ROADMAP.md) for exact status.

## Architecture

The target conceptual ownership model (ADR 0036) is:

```text
User Configuration
        |
        v
Siralos Host
        |
        v
Optional Plugins
```

- **User Configuration** — Profile, Context, Skills
- **Siralos Host** — State, Revision, Capability, Tools, Effects, Evidence;
  the small privileged, non-replaceable kernel
- **Optional Plugins** — Tools, future Views, optional Domains

This is a product ownership model, not an implementation-layer dependency
diagram; dependency details live in [ARCHITECTURE.md](ARCHITECTURE.md).
Orchestration is not a foundational Host layer — higher-level schedulers may
consume Runs later without defining Run semantics.

### Current implementation

During Stage 3R the repository contains two implementations:

```text
TypeScript behavioral reference (CURRENT)
  apps/cli → packages/adapters → packages/core

Rust successor (CURRENT foundation)
  siralos-cli → siralos-adapters → siralos-core
```

Both cores own domain-neutral policy and contracts. Adapters own infrastructure
and optional domain behavior. The CLI is the composition and terminal boundary.
Godot does not define or appear in the Rust core. The TypeScript structure is
the behavioral reference, not the target architecture.

See the [architecture index](docs/architecture/README.md),
[ADR 0032](docs/adr/0032-rust-migration-and-siralos-rename.md),
[ADR 0033](docs/adr/0033-differential-behavioral-harness.md), and
[ADR 0036](docs/adr/0036-lean-product-composition-and-extension-model.md).

## Getting started

### Prerequisites

- Node.js 24 (`.nvmrc`; CI uses 24.17.0)
- npm 11.13.0
- Rust 1.97.1 through `rust-toolchain.toml`
- On Windows, a modern MinGW-w64 toolchain on `PATH` for the pinned
  `x86_64-pc-windows-gnu` Rust host, including GNU `dlltool` and `as`
  ([Rust platform requirements](https://doc.rust-lang.org/rustc/platform-support/windows-gnu.html))
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
- Stage 3R R5 — Generic Language Intelligence — is complete (one-based
  positions/ranges, bounded sanitized diagnostics, generic
  symbol/definition/reference models, the language-neutral structural
  representation, typed validation results, revision binding, and the
  generic language-service URI mapping — at differential parity)
- Stage 3R R6 — Minimal Domain Capability Architecture and Synthetic
  Conformance Domain — is complete (the domain-neutral
  lifecycle/capability semantics in `siralos-core::domain`, the
  production Component Model / WIT boundary in
  `siralos-adapters::domain`, prepared activations bound to the
  validated lifecycle generation with typed stale-commit rejection,
  exact three-dimensional activation identity (id, digest, and ABI
  against the installed package), final grants authorized by the
  commit-time Host authority (prepared activations never carry
  authority across policy contexts), and the deterministic
  product-neutral synthetic conformance Domain proving the boundary —
  at differential parity)
- **Current:** Stage 3R R7 is Active — R7A behavior extraction and provider-protocol remediation are complete; R7.1 (Provider Contract + Deterministic Fake Provider + Bounded Single Model Turn parity) is complete at differential parity (18 `provider-turn` scenarios); R7.2 (Application Tool Loop parity) is complete and evidence-backed (16 `tool-loop` scenarios at differential parity)
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
- [Contributing](CONTRIBUTING.md) — development setup, checks, scope, and review
  expectations
- [Architecture decisions](docs/adr/) — accepted and historical ADRs

## Security

The model cannot grant itself authority. Effects are host-controlled, approvals
bind only to exact prepared operations, and missing enforcement fails closed.
Repository content, provider output, and tool output are untrusted data—not
policy. Read [SECURITY.md](SECURITY.md) before changing an authority or process
boundary or reporting a vulnerability.

## License

No project license has been published yet.
