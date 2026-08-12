# Siralos Rust Style & Engineering Guide

- Status: authoritative for all Rust code in the Siralos repository
- Applies to: every crate under `crates/` created by this or any future
  milestone (Stage 3R and later)
- Enforcement: `npm run check:rust-format`, `npm run check:rust-clippy`,
  `npm run test:rust`, and `npm run check:rust` (see
  [Required gates](#required-gates))
- Recorded in: ADR 0032 (Rust migration)

Future executor instructions reference this document instead of
restating its contents.

## Purpose

Siralos is a deterministic, security-first, context-efficient
software-development and QA harness with a domain-neutral core and
explicitly installed optional domain intelligence. Rust code must serve
that posture: local reasoning, explicit authority, deterministic
behavior, and mechanical enforcement of boundaries.

This guide distills engineering principles from high-quality Rust
practice — including the patterns visible in Andrew Gallant's
(BurntSushi's) ripgrep, aho-corasick, bstr, memchr, rust-csv, and regex
ecosystem work — combined with the official Rust conventions, the Rust
API Guidelines, rustfmt, Clippy, Cargo best practices, and Siralos's own
architecture and security requirements. It does not copy source code and
does not claim BurntSushi authorship or endorsement. The result is the
Siralos style.

## Toolchain and edition

- Rust **edition 2024** for the entire workspace.
- An explicit, pinned toolchain lives in `rust-toolchain.toml`. Do not
  silently depend on whatever compiler happens to be installed.
- **Stable** Rust is the default. Nightly requires an evidenced
  requirement.
- The declared MSRV (`[workspace.package] rust-version`) is the floor;
  the pinned toolchain is what gates are run with. Bumping either
  requires passing the full Rust gate.
- On Windows, the GNU host target is used with an explicit MinGW-w64
  toolchain; see `rust-toolchain.toml` and the repository README.

## Formatting

rustfmt is authoritative. Do not fight rustfmt, and do not hand-format
to bypass it. The repository-owned configuration (`rustfmt.toml`) is:

```toml
max_width = 79
use_small_heuristics = "max"
edition = "2024"
```

Formatting must be deterministic:

```text
cargo fmt --all --check
```

## Required gates

Every Rust change must pass, before handoff:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace
cargo test --workspace
npm run check:rust
```

`npm run check` runs the TypeScript and Rust gates together.

## Readability

Optimize code first for correctness, clarity, local reasoning, and
maintainability — before cleverness. Prefer code whose invariants can be
understood from the local module. Use straightforward control flow;
early returns are encouraged where they reduce nesting. Pattern matching
is preferred when it makes state distinctions explicit. Do not compress
meaningful state transitions into dense expressions merely to reduce
line count.

## Comments

Comments explain _why_: invariants, tradeoffs, safety conditions,
performance reasons, platform reasons, protocol reasons, and
non-obvious behavior. Avoid comments that narrate syntax.

Good comments explain:

- why this cache may be reused
- why this ordering must remain stable
- why this allocation is intentionally retained
- why this lock cannot be removed
- why Windows needs a separate path

When implementation complexity exists because of an important tradeoff,
explain it near the code. Large or subtle subsystems may have a design
document if the explanation cannot reasonably live in source comments;
do not create architecture documentation for trivial code.

## Public API documentation

Public APIs must be intentionally documented: semantics, invariants,
errors, panics where applicable, cancellation behavior where applicable,
side effects, security implications where applicable, and examples for
non-obvious APIs.

Compiler-enforced documentation quality is on by default:

```text
missing_docs = "deny"      (workspace lint)
broken_intra_doc_links = "deny"   (workspace lint, rustdoc)
```

Do not suppress documentation lints globally for convenience.

## Visibility

Default to private. Increase visibility in this order, only when the
wider visibility is actually required:

```text
private
→ pub(crate)
→ pub(super)
→ pub
```

Do not make implementation details public for convenience. Public
exports form a deliberate API surface; prefer explicit re-exports from
crate/module boundaries over leaking the internal source layout.

## Module design

Modules own coherent concepts: one understandable subsystem, not one
file per type. Do not create tiny modules merely to reduce file size,
and do not create enormous modules to avoid architectural thinking.
Split modules when there is a real cohesion or ownership boundary. The
target remains a modular monolith.

## Crate design

A new crate requires a real reason:

- dependency isolation
- distribution boundary
- capability/security boundary
- independently useful library boundary
- compilation boundary with measurable value
- external protocol boundary

Invalid reason: _this class/interface had its own TypeScript file_.
There is no crate-per-concept architecture. The workspace is
`crates/siralos-core`, `crates/siralos-adapters`, `crates/siralos-cli`;
no placeholder or hypothetical domain crates exist.

## Type design

Use the type system to make invalid states difficult or impossible to
construct:

- enums for mutually exclusive states
- newtypes for semantically distinct identifiers
- explicit structs for meaningful records
- private fields when construction requires invariants
- non-zero or bounded integer types where useful

Avoid magic strings, generic `HashMap<String, Value>` state when a type
is known, unrelated boolean flags encoding state, sentinel integer
values, and stringly typed protocols. Do not wrap every primitive in a
newtype; create types where they protect meaning or invariants.

## Enums over boolean state machines

When several booleans form mutually exclusive or constrained states, use
an enum or a dedicated state type. For example, prefer:

```text
NotInstalled
InstalledDisabled
InstalledEnabled
```

over `installed: bool` + `enabled: bool`, where invalid combinations
could otherwise exist.

## Ownership

Prefer borrowing when ownership is not required; prefer owned values
when ownership materially simplifies lifetime management. Do not create
complex lifetimes solely to avoid a small, cold-path allocation. Do not
clone merely to satisfy the borrow checker without understanding why the
clone is needed; every non-trivial clone in a hot or repeated path is
reviewed.

Avoid pervasive `Arc`/`Rc`/`Mutex`/`RwLock` as architectural defaults.
Use shared ownership only when shared ownership is actually required.

## Collections

Choose collections based on semantics — not `HashMap` by default.
Consider stable ordering, deterministic iteration, lookup complexity,
insertion behavior, memory usage, and expected size.

Siralos deterministic host decisions must not accidentally depend on
randomized hash iteration order. Where ordering affects observable
behavior, make ordering explicit (for example a `Vec` of sorted entries
or an explicit sort before iteration).

## Strings, bytes, and paths

Do not assume filesystem paths are valid UTF-8. Keep filesystem
identities in `Path`/`PathBuf`/`OsStr`/`OsString` for as long as
practical and convert to UTF-8 only at boundaries that genuinely require
UTF-8. Do not use lossy conversion for authoritative identity, revision,
security, comparison, or mutation logic unless explicitly designed and
tested. Byte-oriented processing is appropriate where text validity is
not a semantic requirement. Do not introduce `bstr` automatically;
choose dependencies based on actual need.

## Errors

Core/domain code exposes meaningful typed errors where callers need to
distinguish failure kinds. Error types preserve the failure category and
relevant identifiers/limits, and wrap source errors where appropriate.
Never parse error strings to determine control flow.

Conceptually:

```text
core        typed errors
adapters    typed/contextual errors as appropriate
CLI/app     contextual presentation
```

Do not make `anyhow::Error` the universal internal error type, and do
not create a unique error enum for every trivial helper.

## Panics

Panics must not represent expected user, workspace, provider, or runtime
failures; expected failure uses `Result`, `Option`, or an explicit
state. `unwrap()` and `expect()` are acceptable when the invariant is
genuinely guaranteed by construction, or in tests. Production
`expect()` messages explain the violated invariant:

```text
validated task plans always contain at least one phase
```

Avoid meaningless messages such as `should work`.

## Traits

Introduce a trait when there is a real behavioral seam: multiple real
implementations, test substitution at a meaningful boundary, a
host/domain capability boundary, a provider boundary, or an
execution-environment boundary. Do not mirror every TypeScript interface
as a Rust trait. Prefer concrete types until polymorphism is needed.
Avoid deep trait inheritance and generic trait frameworks.

## Generics

Use generics when they provide meaningful static polymorphism or
reusable algorithms. Do not parameterize types speculatively, and prefer
a concrete understandable API over complex generic machinery with one
caller. Avoid generic parameters that merely move complexity from the
implementation to call sites.

## Builders

Use builders for configuration-heavy objects: many options, sensible
defaults, unclear constructor parameter ordering, or optional
configuration that would otherwise create many constructors. Do not use
builders for simple two- or three-field types with obvious construction.
Builder output must still validate invariants.

## Iterators

Prefer iterator-based transformations where they remain clear. Do not
force iterator chains when an ordinary loop makes mutation, error
handling, branching, early termination, or state clearer. Clarity beats
stylistic purity.

## Async

Async must exist because the operation benefits from asynchronous
execution. Do not migrate every TypeScript `async` function to a Rust
`async fn`. Before porting an async operation, determine whether it is
truly asynchronous I/O, concurrent, or cancellation-sensitive — or
merely async because the TypeScript API forced it. Use synchronous Rust
for inherently synchronous operations. Do not introduce an async runtime
into `siralos-core` without an evidenced architectural requirement.

## Concurrency

Concurrency must be explicit and bounded. Prefer ownership transfer and
message passing where appropriate; use locks where shared mutable state
is genuinely the correct representation. Do not introduce
`Arc<Mutex<...>>` by default. For each lock, understand its owner, the
protected invariant, its scope, contention expectations,
poisoning/error semantics, and cancellation interactions.

Parallel execution must not make authoritative Siralos decisions
nondeterministic. Normalize concurrent observations before they
influence deterministic state.

## Unsafe Rust

Siralos starts from `#![forbid(unsafe_code)]` (workspace lint) and
`unsafe` is forbidden in the foundation. Do not add unsafe Rust without
an unavoidable, evidenced requirement; performance alone is not
sufficient without measurement. If a future milestone genuinely requires
unsafe Rust:

1. isolate it behind a small safe abstraction,
2. document the invariant,
3. include an explicit `SAFETY:` explanation,
4. test the boundary,
5. fuzz it where appropriate,
6. benchmark the need,
7. document why a safe implementation was inadequate,

and do not propagate unsafe assumptions across the architecture.

## Dependencies

Prefer the standard library when it provides a clear adequate solution.
Add a dependency when it provides meaningful correctness, security,
interoperability, maintainability, or performance value. Before adding
one, consider maintenance status, license, transitive dependencies,
feature set, default features, platform behavior, MSRV/toolchain
implications, and supply-chain implications. Disable default features
when unused and when doing so materially reduces unnecessary
capability/dependency weight. Do not build inferior local
implementations of complex well-solved infrastructure merely to avoid
all dependencies, and do not add dependencies for hypothetical future
milestones.

## Feature flags

Feature flags represent real optional composition. Cargo features are
compile-time composition; they are not the end-user Godot installation
mechanism. Siralos optional domains require a runtime package lifecycle.
Avoid feature combinations whose behavior is difficult to reason about,
and test supported feature combinations.

## Logging and diagnostics

Logs are diagnostics, not state. Correctness never depends on log
parsing. Prefer structured diagnostic fields where the logging stack
supports them. Avoid noisy logs in hot paths; expensive diagnostic
construction must not occur when the relevant level is disabled. Never
log secrets or credentials.

## Test style

Tests are deterministic. Cover ordinary behavior, boundary cases,
invalid inputs, state transitions, error categories, stale state,
platform-sensitive behavior, and meaningful configuration combinations.
Prefer compact reusable fixtures over duplicated setup; use table/matrix
tests when multiple configuration knobs must preserve the same behavior.
Do not generate enormous abstraction frameworks to avoid several clear
test cases. Tests may use `unwrap()` freely where failure means the test
itself should fail.

## Property testing and fuzzing

Consider property tests or fuzzing for parsers, protocol decoding,
revision handling, canonical serialization, path normalization,
state-machine transitions, structured mutation, and security-sensitive
input boundaries. Fuzzing complements deterministic regression tests; it
does not replace them. Do not fuzz trivial getters/setters.

## Documentation tests

Public examples should compile where practical. Use doctests for
meaningful library API examples; examples represent real supported
behavior. Do not maintain pseudo-code presented as compilable Rust.

## Clippy

Clippy is a required quality gate:

```text
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Do not blindly enable the entire `restriction` group. Selected
additional lints may be enabled when they align with Siralos's
engineering requirements. Lint suppressions must be narrow, local where
practical, and justified — never crate-wide `allow` merely to silence
inconvenient warnings.

## Compiler warnings

Production Rust compiles without warnings under the supported toolchain;
warnings are CI failures. Do not suppress `dead_code`, `unused`, or
`unreachable` globally to make incomplete architecture compile — remove
dead speculative code instead.

## Release profiles

Do not blindly copy another project's Cargo release profile. Create a
Siralos profile based on Siralos's actual runtime characteristics;
candidates to evaluate with measurement include `opt-level`, LTO,
`codegen-units`, panic strategy, debug information, strip, and
incremental. Aggressive release optimization belongs behind measurement.
Build-time cost, binary size, debuggability, and runtime performance are
all legitimate tradeoffs.

## API quality

Public APIs follow established Rust conventions: conventional naming,
common traits where semantically correct, `From`/`TryFrom` where
appropriate, `AsRef` where borrowing conversion is appropriate,
iterator support where natural, useful `Debug`, explicit fallibility,
and predictable ownership. Do not implement traits merely because they
are available; trait implementations must preserve expected semantics.

## Debuggability

Important host types have useful diagnostic representations. Do not leak
secrets, credentials, or full sensitive workspace contents through
`Debug`. Where a derived `Debug` would expose sensitive information,
implement or omit it deliberately.

## Deterministic code

Siralos authoritative decisions must be reproducible. Do not depend on
unordered iteration, wall-clock time without an injected clock,
uncontrolled randomness, process scheduling, filesystem enumeration
order, or thread completion order for authoritative decisions. Normalize
these inputs explicitly. This requirement overrides convenience.

## Security style

Capability enforcement remains explicit. Do not encode authorization in
comments, naming, instructions, documentation, or assumed call ordering;
security is enforced by host-owned state and code. Lower-trust layers
must not acquire authority through generic helper APIs.
Security-sensitive methods make the authority boundary visible.

## Performance style

Prefer architectural performance improvements over clever
micro-optimizations: avoid unnecessary I/O, repeated parsing, repeated
hashing, and unnecessary allocation; stream rather than buffer entire
inputs; bound concurrency; avoid unnecessary process launches; reduce
serialization boundaries; reduce unnecessary context reconstruction.
Only then consider instruction-level or data-layout optimization.

Optimization is evidence-driven: establish a representative benchmark or
measurable fixture, record the baseline, apply the change, measure
again, verify behavioral equivalence, and retain the optimization only
if the tradeoff is justified. A faster incorrect host decision is a
regression. Optimization must never weaken determinism, security,
capability enforcement, revision correctness, evidence provenance,
cancellation semantics, approval binding, validation, or acceptance.

## Review standard for new Rust

Every meaningful Rust change is reviewed for: correctness, invariants,
ownership, error behavior, visibility, allocation, cloning,
determinism, security, platform behavior, test coverage, documentation,
and performance implications. This review is especially important during
TypeScript migration.
