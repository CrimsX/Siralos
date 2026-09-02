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

> **Status: active development / Rust migration.** The TypeScript implementation is **archived** at `5da5cde` (freeze v32, 234/234, pinned). Rust is the **sole source of truth**, migrated under differential verification. Stage 3R R4 —
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
> status matrices); **R7.3 — Projection parity** contract frozen and
> reconciled with locally pinned terminal-marker precedence; independent
> review has returned **PASS** — implementation is complete and evidence-backed
> (13 projection/application integration tests plus 11 required
> `context-projection` scenarios); **R7.4 — Configuration parity** is
> complete and evidence-backed (2 required
> `user-config` scenarios, corpus version 15, 133 scenario files); **R7.5 —
> `/context` and `/tools` CLI rendering** — complete and evidence-backed (deterministic real-session rendering over the
> existing projection and Tool authority seams; 51 focused Rust CLI tests (10 sanitize) plus TypeScript oracle coverage; advisory P2 closed); R7 **Verified** at 61fbf997d781. **R8 — Optional Godot Stage-2 parity** (discovery/profiling, recovery contracts, API knowledge, check-only diagnostics, bounded LSP, read-only scene/resource intelligence) is complete and evidence-backed: corpus version 16, 155 scenario files, all five frozen `godot-*` differential subjects at required parity (150/150 applicable required scenarios), fail-closed posture mechanically preserved; R8 **Verified** at c075b3cf5e52. **R9 — Optional Godot Stage-3 parity** (review context & impact intelligence, prepare-only mutation contracts, deterministic `/develop` core) is complete and evidence-backed: corpus version 17, 167 scenario files, all three frozen subjects at required parity (162/162 applicable required scenarios); apply/checkpoints stay typed `unavailable`; R9 **Verified** at 1623e800f8034. **R10 — H1/H2/ICM/H3 runtime-readiness parity** is complete and evidence-backed as one milestone with three entry-reviewed sub-slices: corpus version 18 (`siralos_core::identity` + `siralos_core::determinism`), version 19 (`siralos_core::context`), and version 20 (210 scenario files, 205/205 applicable required scenarios) for `siralos_core::runtime`; no real process is ever launched; R10 **Verified** at a456afb71ab64c5504cd19e8eb7988d32d60a9dc.

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

### Target concepts and current status

- Profiles, portable `siralos.toml`/`siralos.lock` semantics, Context
  controls (Live / Pinned / Frozen), Skills, Plugins, the generic
  Controlled Runtime surfaces, and the bounded `/evolve` workflow are
  implemented and Verified (Stages 4–6, decisions 41–59); Views where
  justified and additional Domains remain targets in their owning stages
  (ADR 0036)
- Real provider integrations are **in progress** per decisions 66–68
  (user-directed 2026-08-31): `ProfileRecord`
  provider/model/credential/endpoint, env-only `HostCredential`, and
  `reqwest` blocking OpenAI/Anthropic adapters plus the all-purpose
  `GenericProvider`; completion and verification are the active slice

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

The live repository is **Rust-only**. The TypeScript historical oracle is **archived** at `5da5cde` (freeze v32, 234/234, pinned at `tests/differential/evidence/typescript-freeze-v32/`; audit `v31 e24f4bb` retained as historical) and retained only as digest-bound evidence:

```text
TypeScript historical oracle (archived at 5da5cde, v32 234/234, pinned; prior v31 e24f4bb retained)
  apps/cli → packages/adapters → packages/core — removed from live tree; historical replay requires worktree at freeze SHA

Rust successor — sole source of truth (live)
  siralos-cli → siralos-adapters → siralos-core
  siralos-godot → siralos-core (standalone plugin repo, pinned path dep)
  (siralos-godot was externalized to github.com/CrimsX/siralos-godot at
   1bf2ca3 per decisions 60–65; the monorepo is a 3-member workspace with
   `siralos-godot = { path = "../siralos-godot" }`; siralos-core stays
   domain-neutral)
```

The core owns domain-neutral policy and contracts. `siralos-godot` (standalone
repo, pinned path dep) owns the Godot domain surfaces (moved per decisions 37
and 60–65); adapters own infrastructure and the provider HTTP adapters. The
CLI is the composition and terminal boundary. The historical TypeScript oracle is retained by SHA as pinned digest-bound evidence; the differential harness runs in **pinned mode** (historical replay requires worktree at freeze SHA); it never grants product authority.

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

Build and inspect the Rust CLI (sole implementation):

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

This covers formatting, linting, documentation links, project-context, identity and public-hygiene ratchets, Rust architecture, R2 differential parity (pinned v33), Rust formatting, Clippy with warnings denied, and Rust tests.

Run only the authoritative R2 parity decision:

```bash
npm run check:differential
```

## Optional Godot support

Godot Engine is Siralos's first optional specialization. The TypeScript
reference can statically inspect Godot projects, scenes, and resources without
executing project code. Dynamic engine probes and project execution remain
fail-closed where their security properties cannot be enforced.

The Rust Godot domain package lives in `crates/siralos-godot` (72 tests,
`siralos-godot → siralos-core` only) — the 6+3 R8/R9 surfaces moved there from
`siralos-core/src/godot` (deleted, domain-neutral again) per decision 37.
Siralos does not install Godot, silently enable a domain, or acquire a domain
merely because `project.godot` exists. The domain package and the Godot Engine
installation are separate, explicit concerns.

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
- **Current:** Stage 3R R12 is **Verified (retired)** — R1–R13 all Verified; R12 retires the TypeScript oracle — **Rust is now the sole source of truth**. **TypeScript archive removal is complete** per decision 40 at `e6c49f1` (freeze `5da5cde` v32 234/234, pinned, corpus v33 `01ba53a…`; live `apps/` + `packages/` removed). **Stage 4 (Controlled Execution) is complete and Verified** at `9566eee` — the frozen seven-step sequence (decision 08) is fully consumed: generic runtime execution + evidence (4.1 with 4.2 folded in, subjects landed at the v32 reconciliation), Godot runtime adapter (`5bedf57`), visual evidence (`4a250d8`), controlled interaction (`42ee5ab`), QA workflows (`a83c2a4`), and run-profiling sessions (`b206a4a`); differential parity is 259/259 applicable required at corpus v38 (264 files) with 25 digest-bound expectation records, the pinned v32 oracle untouched, and zero spawn paths; recorded in decisions 41–46 and the Wayfinder map. **Stage 5 (Composition) is complete and Verified** at `c2c30f0` — ten slices across decisions 47–56 (5.1 Profiles be030e3, 5.2 Profile Composition 4c562c8, 5.3 Context Controls ce3e7dc, 5.4 siralos.lock 0a6d592, 5.5 Plugin Selection 5e1b3e0, 5.6 Skills fcf61c5, 5.7 Session Plugin Activation Gate 926ac71, 5.8 Session Context Controls 6dc830e, 5.9 Session Lock Verification 6e38804, 5.10 Session Skill Consumption 579f1e9) with differential parity 299/299 at v48/304, 65 expectation records, pinned v32 oracle untouched, zero spawn paths; recorded in decisions 47–57 and the Wayfinder map. **Stage 6 (Evolution & Stabilization) is complete and Verified** at `e2c3540` — four slices across decisions 58–59 (6.1 Evaluation Corpus a79f613, 6.2 Workflow 0ba256f, 6.3 Proposal ddb18a4, 6.4 Packaging e2c3540) with differential parity 315/315 at corpus v52/320, 81 expectation records, pinned v32 oracle untouched, zero spawn paths; recorded in decisions 58–59 and the Wayfinder map. **Stage 7 (Godot externalization) is complete** per decisions 60–65 — the Godot domain + host adapters moved verbatim to the standalone `https://github.com/CrimsX/siralos-godot` repo (41 files + host adapters self-contained at `1bf2ca3`), the monorepo pins it as an external path dep (3-member workspace, shim removed at `87bfd35`), and differential parity held (315/315 at v52).
- Next: **Real Model/Provider (decisions 66–68, user-directed 2026-08-31) is in progress** — `ProfileRecord` provider/model/credential/endpoint (67 C1), the provider registry with env-only `HostCredential` (68), `reqwest` blocking OpenAI/Anthropic adapters (68 §3), and the all-purpose `GenericProvider` with the `provider-generic` differential subject at corpus v53 (316/316 applicable required, pinned v32 oracle untouched); completion and verification are the active slice

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
