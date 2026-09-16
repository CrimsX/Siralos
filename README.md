# Siralos

Siralos is a minimal, declarative AI coding harness with an inspectable
execution environment.

Profiles define how the model works. Context shows what Siralos gives it. The
Host controls what it can do.

_Probabilistic reasoning. Deterministic execution._ The host places host-owned
validation, policy, evidence, and controlled effects around probabilistic model
reasoning, and stays small by moving sophistication into declarative
configuration, Skills, and explicitly installed Plugins
([ADR 0036](docs/adr/0036-lean-product-composition-and-extension-model.md)).

[![Rust CI](https://github.com/CrimsX/Siralos/actions/workflows/rust.yml/badge.svg)](https://github.com/CrimsX/Siralos/actions/workflows/rust.yml)
[![CodeQL](https://github.com/CrimsX/Siralos/actions/workflows/codeql.yml/badge.svg)](https://github.com/CrimsX/Siralos/actions/workflows/codeql.yml)

> **Siralos is pre-1.0 and under active development**, so the surface is still
> moving. Milestone status, verification records, and the current frontier live in
> [ROADMAP.md](ROADMAP.md). This page describes what the product is and how to run
> it.

## Contents

- [What Siralos is](#what-siralos-is)
- [What Siralos is not](#what-siralos-is-not)
- [Status vocabulary](#status-vocabulary)
- [Requirements](#requirements)
- [Quickstart](#quickstart)
- [Configure a profile](#configure-a-profile)
- [Run it](#run-it)
- [Optional Godot support](#optional-godot-support)
- [Architecture](#architecture)
- [Verification](#verification)
- [Documentation](#documentation)
- [Security](#security)
- [License](#license)

## What Siralos is

A model can reason and propose. It does not own Siralos state or authority. The
host owns the path from proposal to evidence:

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

The core is provider-neutral and domain-neutral, and optional domains add
specialized intelligence without gaining host capabilities. Godot Engine is the
first and currently only optional domain.

What that buys you:

- Deterministic Host decisions around probabilistic reasoning
- Inspectable, provenance-bearing model Context
- Explicit capability and fail-closed authority boundaries
- Revision-bound, verifiable effects
- Evidence, replay, and differential verification
- Optional, capability-scoped specialization instead of core feature growth

## What Siralos is not

- **Not an agent framework.** No multi-agent machinery, no TaskGraph, no generic
  workflow engine, no plugin marketplace, no automatic acquisition, and no
  general Hooks. These are deliberately not committed, and may be reconsidered
  only from concrete demand and evidence
  ([ADR 0036](docs/adr/0036-lean-product-composition-and-extension-model.md)).
- **Not a state owner.** Task state is host-owned; model completion is a request
  the host evaluates against its own acceptance evidence.
- **Not a security boundary by itself.** The architecture checks in this
  repository are developer guardrails, not an OS boundary. The enforceable
  boundary is the sandbox backend described in [SECURITY.md](SECURITY.md).
- **Not a Godot tool.** Godot is one optional domain, installed explicitly.
  Nothing is enabled merely because `project.godot` exists.
- **Not a credential store.** Credentials are resolved from the environment when
  a provider is called, and are never written to configuration, context, or logs.

## Status vocabulary

- **CURRENT** — implemented and verified in the repository today.
- **TARGET** — committed product direction for a future stage (Views where
  justified, additional Domains). A target item is described as direction, never
  as shipped.
- **FUTURE / NOT DUE** — deliberately not committed, and reconsidered only from
  concrete demand and evidence.

## Requirements

- Rust, pinned by `rust-toolchain.toml`
- Node.js 24 and npm 11.13.0 (`.nvmrc`) for the repository quality gate
- Git
- On Windows, a modern MinGW-w64 toolchain on `PATH` for the pinned
  `x86_64-pc-windows-gnu` Rust host, including GNU `dlltool` and `as`
  ([Rust platform requirements](https://doc.rust-lang.org/rustc/platform-support/windows-gnu.html))

## Quickstart

```bash
git clone https://github.com/CrimsX/Siralos.git siralos
cd siralos
npm ci
cargo run --locked --bin siralos -- --version
```

`--help` lists the whole command surface:

```bash
cargo run --locked --bin siralos -- --help
```

### An offline first turn

Siralos ships a deterministic fake provider, so a first turn needs no credential
and no network. Create a workspace directory with a profile in it:

```bash
mkdir demo
```

```toml
# demo/siralos.toml
[profile]
name = "offline-demo"
provider = "deterministic-fake"
model = "echo"
```

and run one turn headlessly against it:

```bash
cargo run --locked --bin siralos -- --cwd demo --print "hello" --json
```

`--print` answers one prompt without any interactive frontend, `--json` emits
one structured record instead of the answer text, and `--cwd` chooses the
workspace root (the default is the working directory).

### The interactive frontend

```bash
npm run siralos            # debug build
npm run siralos:release    # release build
```

The terminal frontend reveals text one character per painted frame, so the build
sets its ceiling: an unoptimized debug build sustains roughly 650 characters a
second and a release build roughly 2800 (measured; decision 173). Use the release
path when a fast model should be tracked exactly.

## Configure a profile

A profile is the composition unit: declarative, versioned, and **narrowing-only**
— it may restrict what the host permits, never widen it. It lives in
`siralos.toml` at the workspace root. The file is git-ignored by default, because
it is where provider details belong.

```toml
[profile]
name = "openrouter-default"
provider = "openrouter"                  # any OpenAI-compatible endpoint
model = "cohere/north-mini-code:free"
endpoint = "https://openrouter.ai/api/v1"
credential = "env:OPENROUTER_API_KEY"    # resolved from the environment at use time
context = "live"                         # live | pinned | frozen
skills = []                              # declarative guidance by name, e.g. ["my-skill"]
plugins = []                             # installed plugin ids to activate
model_display_name = "OpenRouter"

[profile.permissions]
"workspace.read" = "allow"               # allow | ask | deny
"workspace.write" = "deny"
"command.run" = "deny"
```

Every field is validated by the composition parser: an unknown key, a permission
rule that is not `allow`/`ask`/`deny`, a malformed capability id, or a malformed
credential reference leaves the profile unapplied rather than half-applied.

Credential references come in two forms:

- `env:NAME` (recommended) — resolved from the environment when the provider is
  called, held in memory for that call only, and redacted in every log and report;
- `key:VALUE` — a literal token. It works, but it puts the secret in a file;
  prefer the environment.

## Run it

| Command                                                  | What it does                           |
| -------------------------------------------------------- | -------------------------------------- |
| `npm run siralos`                                        | interactive frontend, debug build      |
| `npm run siralos:release`                                | interactive frontend, release build    |
| `cargo run --locked --bin siralos -- --print "<prompt>"` | one headless turn                      |
| `... --print "<prompt>" --json`                          | the same turn as one structured record |
| `... --print "<prompt>" --cwd <dir>`                     | run against another workspace root     |
| `... --stdio`                                            | interactive stdio frontend             |

Effects that cannot be enforced are reported as a typed `unavailable` before
execution, approval, checkpoint creation, or cleanup — never after. The closed
surfaces are listed in [ARCHITECTURE.md](ARCHITECTURE.md) and
[SECURITY.md](SECURITY.md).

## Optional Godot support

Godot Engine is Siralos's first optional specialization. The domain package lives
in the standalone plugin repository
[github.com/CrimsX/siralos-godot](https://github.com/CrimsX/siralos-godot) and
depends on the core only. It can statically inspect Godot projects, scenes, and
resources without executing project code. Dynamic engine probes and project
execution stay fail-closed: they report `unavailable` and launch nothing.

Siralos does not install Godot, silently enable a domain, or acquire a domain
merely because `project.godot` exists. The domain package and the Godot Engine
installation are separate, explicit concerns.

## Architecture

The product ownership model ([ADR 0036](docs/adr/0036-lean-product-composition-and-extension-model.md))
is:

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
- **Siralos Host** — State, Revision, Capability, Tools, Effects, Evidence; the
  small privileged, non-replaceable kernel
- **Optional Plugins** — Tools, future Views, optional Domains

The implementation is a three-crate Rust workspace with one direction of
dependency:

```text
siralos-cli → siralos-adapters → siralos-core
siralos-godot → siralos-core            (standalone plugin repository)
```

`siralos-core` is domain-neutral and depends on no workspace crate.
`siralos-adapters` may depend only on `siralos-core`. `siralos-cli` composes them
and owns the terminal boundary. Orchestration is not a foundational Host layer.

Dependency details live in [ARCHITECTURE.md](ARCHITECTURE.md), and
[docs/architecture/README.md](docs/architecture/README.md) maps subsystems to code.

## Verification

The repository ships one quality gate:

```bash
npm run check
```

It covers formatting, linting, documentation links, documentation truth,
project-context, the identity and public-hygiene ratchets, Rust architecture, the
differential behavioral harness, Rust formatting, Clippy with warnings denied, and
the Rust test suites.

The differential behavioral harness
([ADR 0033](docs/adr/0033-differential-behavioral-harness.md)) is the mechanism
behind behavioral claims: a scenario corpus is run against a pinned oracle and the
Rust candidate, and typed canonical outcome records are compared. Corpus and
scenario digests are checked in, and the harness is pinned so results are
reproducible.

Run only the parity decision:

```bash
npm run check:differential
```

### Compare models (owner-run)

Run one fixed, digest-bound task set through more than one configured
provider/model and print one INFORMATIONAL comparison — evidence only, never a
gate:

```bash
npm run evaluate -- --run first=<workspace-a> --run second=<workspace-b> --out eval.json
```

Each `--run <label>=<dir>` composes a session for that workspace's own
`siralos.toml`, so the provider, model, and credential are the profile's. This
spends the profiles' real provider budget, so it is never part of `npm run check`.

## Documentation

| Document                                                                   | Owns                                            |
| -------------------------------------------------------------------------- | ----------------------------------------------- |
| [ROADMAP.md](ROADMAP.md)                                                   | milestone status — the canonical status source  |
| [ARCHITECTURE.md](ARCHITECTURE.md)                                         | dependency ownership                            |
| [SECURITY.md](SECURITY.md)                                                 | the security contract                           |
| [ENGINEERING.md](ENGINEERING.md)                                           | implementation rules and validation conventions |
| [CONTRIBUTING.md](CONTRIBUTING.md)                                         | development setup, checks, scope, and review    |
| [docs/development/PROJECT_CONTEXT.md](docs/development/PROJECT_CONTEXT.md) | development bootstrap                           |
| [docs/architecture/README.md](docs/architecture/README.md)                 | subsystem-to-code and ADR map                   |
| [docs/adr/](docs/adr/)                                                     | the decision history                            |
| [docs/requirements/REQUIREMENTS.md](docs/requirements/REQUIREMENTS.md)     | normative requirement registers                 |
| [docs/development/RUST_STYLE.md](docs/development/RUST_STYLE.md)           | the authoritative Rust style guide              |
| [docs/archive/](docs/archive/)                                             | historical records, never guidance              |

For development work or a new coding-agent session, read [AGENTS.md](AGENTS.md)
and the [project context](docs/development/PROJECT_CONTEXT.md) first.

## Security

The model cannot grant itself authority. Effects are host-controlled, approvals
bind only to exact prepared operations, and missing enforcement fails closed.
Repository content, provider output, and tool output are untrusted data, not
policy. Read [SECURITY.md](SECURITY.md) before changing an authority or process
boundary, or before reporting a vulnerability.

## License

No project license has been published yet.
