# Architecture

**What this document owns:** dependency ownership — the crate layout, the
direction dependencies may run, and which components hold which authority.
Other documents own other things and are not restated here:

| Document                                                   | Owns                                           |
| ---------------------------------------------------------- | ---------------------------------------------- |
| [README.md](README.md)                                     | what the product is, and how to run it         |
| [ROADMAP.md](ROADMAP.md)                                   | milestone status — the canonical status source |
| [SECURITY.md](SECURITY.md)                                 | the security contract                          |
| [docs/adr/](docs/adr/)                                     | the decision history                           |
| [docs/architecture/README.md](docs/architecture/README.md) | the architecture index                         |
| this file                                                  | dependency ownership                           |

The TypeScript-era version of this document is archived at
[docs/archive/architecture-typescript-era.md](docs/archive/architecture-typescript-era.md).
It is history, not guidance, and it describes a tree that no longer exists.

## The workspace

The root workspace has exactly three members, and two exclusions:

```toml
members = ["crates/siralos-core", "crates/siralos-adapters", "crates/siralos-cli"]
exclude = ["fuzz", "harness"]
```

- `fuzz/` is nightly-only and must never enter the stable quality gate.
- `harness/` is the differential harness ([ADR 0033](docs/adr/0033-differential-behavioral-harness.md)).
  It is a **separate workspace** with its own `Cargo.lock`, excluded so the
  product workspace carries no external domain dependency and builds from a
  bare `git clone`.

The Godot domain lives outside this repository, in the standalone plugin
repository, and is reached here through the path dependency
`siralos-godot = { path = "../siralos-godot" }`. That dependency belongs to
the **excluded harness workspace only** — no product member may carry it.

## Dependency direction

One normative rule, enforced mechanically by
[`npm run check:rust`](scripts/check-rust-architecture.mjs), which fails the
build on any violation:

> `siralos-core` depends on no workspace crate and on no infrastructure or
> domain. `siralos-adapters` may depend only on `siralos-core`.
> `siralos-cli` may depend only on `siralos-core` and `siralos-adapters`.

| Crate                      | May depend on                                                      | Authority it holds                                                                          |
| -------------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------- |
| `siralos-core`             | nothing                                                            | domain-neutral host semantics: contracts, state, transitions, policy, identity, determinism |
| `siralos-adapters`         | `siralos-core`                                                     | infrastructure: filesystem, config, provider transport, process, the sandbox boundary       |
| `siralos-cli`              | `siralos-core`, `siralos-adapters`                                 | frontends and presentation only (TUI, stdio, headless)                                      |
| `harness/` (excluded)      | `siralos-core`, `siralos-adapters`, `siralos-cli`, `siralos-godot` | dev-only verification tooling; never shipped                                                |
| `siralos-godot` (external) | `siralos-core`                                                     | the optional Godot domain                                                                   |

The one non-obvious edge: `godot → core` crosses a repository boundary as an
external path dependency, and `harness → godot` is what pulls it into a build.
Nothing in the product does.

A second invariant follows: **`siralos-core` contains no Godot or other
domain-specific symbol.** Domain intelligence arrives from outside the core,
through the host boundary
([ADR 0034](docs/adr/0034-godot-domain-host-boundary.md),
[ADR 0035](docs/adr/0035-domain-neutral-controlled-runtime-boundary.md)).

## The crates

**`siralos-core`** — domain-neutral host semantics, with no I/O. Its modules
are the host's own vocabulary: `composition` (profiles, lock, skills, context
controls), `context` (the interpretable context architecture), `determinism`
(the explicit clock, random and ordering ports), `domain` (the host boundary
and its capability vocabulary), `identity` (canonical artifact digests),
`language` (language-neutral structural documents and diagnostics),
`projection`, `provider` (provider-neutral request and event types),
`runtime` (supervision, budgets, the failure taxonomy), `task`, `tool`,
and `workspace`.

**`siralos-adapters`** — everything that touches the world: the canonical
workspace root and containment-safe resolution, bounded reads and searches,
configuration parsing, provider transport, process runners, and the
sandbox-enforcement boundary.

**`siralos-cli`** — the `siralos` binary and its frontends. It composes the
coordination seams; it does not own host semantics.

## The model-facing surfaces

The tool surface is the seam the model actually reaches. Exactly three tools
are registered unconditionally, and **all three are read-only**:
`workspace.list`, `workspace.read`, `workspace.search`. Further read-only
context tools join them only when the default-off context subsystem is opted
in. Permission evaluation is allow / ask / deny
([ADR 0002](docs/adr/0002-provider-neutral-tool-loop.md),
[ADR 0004](docs/adr/0004-sandbox-and-permission-boundary.md)).

The Application Tool Loop pairs one call with one result, and a cancelled tail
stays paired. The projection decides what the model may see; the permission
evaluator decides what it may do. Neither implies the other.

## Composition

A **Profile** is the composition unit
([ADR 0036](docs/adr/0036-lean-product-composition-and-extension-model.md)):
declarative, versioned, and **narrowing-only** — a profile may restrict what the
Host permits, never widen it. `siralos.lock` resolves the portable identities
it declares.

A **Plugin** is the only optional executable extension package type. A Domain is
a semantic specialization a Plugin contributes, not a separate package
ecosystem. Skills are declarative, digest-bound model guidance holding **no
authority**.

## Sandbox enforcement

`SandboxBackend` owns enforcement capability, and enforcement is distinct from
availability: a backend that cannot enforce must be reported as unavailable, and
a skipped live probe is never a pass. The backend is pinned, so availability
claims are per-platform, not general. [SECURITY.md](SECURITY.md) owns the
contract; this document claims only the ownership.

## Verification

The differential behavioral harness
([ADR 0033](docs/adr/0033-differential-behavioral-harness.md)) is the mechanism
by which behavior claims are checked: a scenario corpus, run against a pinned
oracle and the Rust candidate, with typed canonical outcomes compared. Corpus
and scenario digests are checked in. Rust is the sole source of truth
([ADR 0032](docs/adr/0032-rust-migration-and-siralos-rename.md)); the
TypeScript-era oracle is retained only as digest-bound evidence.

## Fail-closed posture

Several surfaces intentionally report a typed `unavailable` and perform no
filesystem mutation or process launch: workspace create/edit/delete application
and safe undo, new checkpoint creation and pruning, private run-directory
creation or cleanup, command execution, Git inspection, and every engine or
language-server probe.

**Why they are closed is a ported-parity decision, not a language limitation.**
The Rust implementation was ported to byte-match a frozen TypeScript oracle that
also lacked an identity-bound commit primitive, and the corpus **pins that
outcome** —
[tests/differential/corpus/workspace-apply.apply-unavailable.json](tests/differential/corpus/workspace-apply.apply-unavailable.json)
and
[tests/differential/corpus/workspace-prepare.unavailable.json](tests/differential/corpus/workspace-prepare.unavailable.json)
are among the scenarios asserting it. Reopening one is therefore a deliberate,
reviewed oracle amendment, never a code change alone.

Do not weaken this posture with another pathname recheck, hashing window,
private filename, patch, comment, or documentation claim. A capability becomes
available only when its security property is mechanically enforceable and
covered by adversarial tests.

## Not committed

Persistence, multi-agent machinery, general Hooks, TaskGraph, workflow engines,
marketplaces, and automatic acquisition are **not committed**
([ADR 0036](docs/adr/0036-lean-product-composition-and-extension-model.md)).
The lean model is deliberate: sophistication belongs in declarative
configuration, Skills, and explicitly installed Plugins.
