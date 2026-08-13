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

## Engineering priority order

Siralos resolves conflicts between engineering goals in this order:

```text
1. Correctness
2. Security / authority preservation
3. Determinism
4. Type-driven invariant design
5. Ownership clarity
6. Simple local reasoning
7. Maintainability
8. API quality
9. Testability
10. Performance evidence
11. Consistent formatting/style
```

Style must never override correctness. Performance must never override
correctness, security, or determinism.

## Migration philosophy

Siralos Rust is not TypeScript, Node.js, C, C++, Java, or another
language expressed with Rust syntax. During migration:

```text
existing behavior + invariants
        ↓
understand semantics
        ↓
identify accidental source-language structure
        ↓
design idiomatic Rust representation
        ↓
preserve observable behavior
        ↓
verify
        ↓
measure
        ↓
optimize where justified
```

Do not preserve source-language structure merely for visual similarity.
Behavioral parity does not require structural parity.

## Toolchain and edition

- Rust **edition 2024** for the entire workspace.
- An explicit, pinned toolchain lives in `rust-toolchain.toml`. Do not
  silently depend on whatever compiler happens to be installed.
- **Stable** Rust is the default and the production-build requirement.
  Nightly is used only for separate quality jobs (fuzzing, Miri,
  sanitizers, coverage) and requires an evidenced requirement.
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
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo check --workspace --all-targets --all-features --locked
cargo test --workspace --all-targets --all-features --locked
npm run check:rust
```

`npm run check` runs the TypeScript and Rust gates together. Locked
dependency resolution is required; `Cargo.lock` is authoritative for the
application and is never silently updated by CI.

## Type-driven design

Use the type system to make invalid states difficult or impossible to
construct. Prefer enums, validated constructors, newtypes where
semantically meaningful, explicit state types, `Option<T>`, and
`Result<T, E>`, and private fields protecting invariants.

Avoid sentinel values, magic strings, integer status codes, groups of
booleans representing one state machine, stringly typed identifiers,
and generic JSON objects where a stable type is known.

### Option over sentinels

Use `Option<T>` when absence is semantically meaningful. Do not
represent absence with arbitrary sentinel values such as `-1`, `0`, `""`,
`"none"`, or `"null"` unless that sentinel is part of an external
protocol Siralos must faithfully model. At an external boundary, parse
the sentinel into the internal typed representation as early as
practical.

### Result over status flags

Recoverable operations return `Result<T, E>` rather than a `bool`,
integer error code, magic string, or panic when callers need failure
information. Do not introduce a custom `Result`-like abstraction; use
Rust conventions.

### State modeling

When multiple primitive fields jointly encode one logical state,
replace them with one explicit state type unless there is a concrete
reason not to. For example, prefer:

```text
NotInstalled
InstalledDisabled
InstalledEnabled
```

over `installed: bool` + `enabled: bool` when the boolean representation
admits invalid combinations. Apply this especially to task lifecycle,
approvals, revisions, capabilities, domain installation, evidence
status, validation state, acceptance state, process lifecycle, and
cancellation state. Do not create an enum where the domain is genuinely
binary and a boolean is clearer.

### Boolean policy

There is no blanket prohibition on `bool`. Use `bool` when the concept
genuinely represents yes/no, enabled/disabled, or a present/absent
property with no additional semantic state. The decision is semantic,
not stylistic.

### Newtype policy

Use newtypes when they materially protect identity, unit, authority,
validation, range, or cross-domain confusion — for example `TaskId`,
`RevisionId`, `ArtifactDigest`, `CapabilityId`, `DomainPackageId`,
`RunId`. Do not wrap every primitive merely for stylistic consistency;
every newtype must earn its existence by improving correctness or
meaning.

### Build values directly

Prefer constructing a value from an expression when that improves local
reasoning:

```rust
let mode = match config {
    Config::Fast => Mode::Fast,
    Config::Safe => Mode::Safe,
};
```

over a mutable variable assigned inside branches, when the value is
conceptually the result of one decision. Do not contort naturally
iterative or stateful algorithms to eliminate every mutable local;
mutation is valid when mutation is the clearest model.

## Ownership and borrowing

Prefer borrowing when ownership is not required; prefer owned values
when ownership materially simplifies lifetime management. Do not create
complex lifetimes solely to avoid a small, cold-path allocation.

A `.clone()` introduced merely to satisfy the borrow checker is a
**design-review trigger**: determine which component should own the
value, whether the lifetime/borrow scope can be simplified, whether
ownership should move, and whether restructuring removes the clone.
Keep the clone only when copying the value is the correct semantic
operation. Valid clones include intentionally duplicated immutable
data, ownership transfer across long-lived boundaries, small cold-path
values, snapshots, and explicit replication semantics. For non-trivial
or repeated-path clones, review frequency, data size, ownership reason,
alternative designs, readability, and performance relevance. Do not
optimize away inexpensive clones at the cost of incomprehensible
lifetimes.

## API design

### Function parameters

There is no blanket rule that every public parameter uses
`impl AsRef<T>` or `impl Into<T>`. Use the least-assumptive parameter
type that materially improves callers without obscuring semantics. A
public path utility with heterogeneous callers may use
`impl AsRef<Path>`; an internal function with one clear borrowing
contract uses `&Path`; a function whose semantics require ownership
takes `String`. Use generic convenience deliberately, not
automatically.

### Traits

Introduce a trait when there is a real behavioral seam: multiple real
implementations, meaningful test substitution, a provider boundary, an
execution-environment boundary, a host/domain capability boundary, or a
protocol abstraction. Prefer concrete types when only one implementation
exists and polymorphism provides no present value. Do not design trait
hierarchies for hypothetical future consumers. Do not translate every
TypeScript interface into a Rust trait.

### From / TryFrom

Prefer the standard conversion traits when conversion semantics are
natural: `From` for infallible semantic conversion, `TryFrom` when
validation or failure is required. Do not implement them when conversion
would be surprising or hide expensive/domain-significant behavior, and
do not add conversions merely to make `.into()` available everywhere.

### Pattern matching and match exhaustiveness

Use `match`, `if let`, and `let ... else` where they improve clarity
with enums, `Option`, `Result`, destructuring, and explicit state
transitions. Do not require pattern matching when a straightforward
method or conditional is clearer. Prefer explicit exhaustive matching
for closed internal enums; avoid `_ =>` catch-all arms that would cause
newly added variants to be silently ignored. Wildcard arms are
acceptable when the remaining variants are intentionally equivalent,
the input type is externally extensible/non-exhaustive, or ignoring
remaining cases is genuinely part of the contract; document non-obvious
wildcard behavior.

### Tuples

Tuples are allowed. Use them for short-lived local grouping, iterator
items, closure inputs/outputs, and simple two-value relationships whose
semantics are obvious. Prefer a named struct when the value crosses an
API boundary, fields have distinct semantic meaning, there are several
fields, callers depend on positional knowledge, or the representation is
likely to evolve. Do not create a struct for every two-value iterator
pair.

### Turbofish

There is no mandatory turbofish style. Use whichever form is clearest in
context; prefer inference when unambiguous and explicit types when they
clarify domain meaning or resolve ambiguous inference. Do not create
style churn solely to change type-annotation placement.

### Iterators and loops

Prefer iterators when they make the transformation clearer; prefer
ordinary loops when they make mutation, branching, state, early
termination, error handling, or ownership clearer. Avoid index loops
when iterating directly over the collection provides the same semantics.
Do not force `.map().filter().fold()` chains when a loop is easier to
understand. Readability outranks iterator purity.

### Builders and generics

Use builders for configuration-heavy objects with many options, sensible
defaults, unclear constructor parameter ordering, or optional
configuration; builder output must validate invariants. Do not use
builders for simple two- or three-field types. Use generics when they
provide meaningful static polymorphism or reusable algorithms; do not
parameterize speculatively.

## Error handling

### Layered error architecture

```text
siralos-core            typed semantic errors
domain/protocol bounds  typed errors appropriate to caller decisions
adapters                preserve meaningful source/context
CLI / application       contextual aggregation/presentation
```

Use `anyhow` or equivalent only at application/orchestration boundaries
where callers no longer need to branch on detailed error types. Do not
use `anyhow::Error` as the universal internal error representation, and
do not create a bespoke error enum for every tiny helper.

### Error types

Meaningful errors preserve information required for recovery,
deterministic decisions, diagnostics, policy, tests, and provenance.
Errors distinguish failure categories where Siralos behavior depends on
them. Never build control flow around human-readable error strings;
never parse error strings to determine control flow.

### Panics, unwrap, expect

Panics must not represent expected user, workspace, provider, protocol,
stale-revision, process, optional-domain-absence, or configuration
failures; expected failure uses `Result`, `Option`, or an explicit
state. Panics may represent impossible internal states where the
invariant has already been enforced and continuation would indicate a
programming defect. Production code does not use `.unwrap()` for
expected failure paths. `expect()` is acceptable when the invariant is
genuinely guaranteed by construction, and the message must explain the
violated invariant — not `should work` but e.g. `validated task plans
always contain at least one phase`. Tests may use `unwrap()`/`expect()`
freely where failure means the test itself should fail.

## Modules and visibility

### Visibility

Default to private. Increase visibility in this order, only when the
wider visibility is actually required:

```text
private
→ pub(super)
→ pub(crate)
→ pub
```

Do not expose implementation details for convenience. Public exports
form a deliberate API surface; prefer explicit re-exports from
crate/module boundaries over leaking the internal source layout.

### Public API surface

Reduce unnecessary `pub` items. A public Rust API creates documentation,
compatibility, testing, and semantic-expectation burden. Do not expose
migration internals or implementation structure merely to simplify
tests; test through meaningful boundaries where practical.

### Module organization and ordering

Keep type definitions and their principal implementations near each
other where practical; organize by coherent responsibility, not
arbitrary type count. Do not enforce one-type-per-file, and do not place
an entire subsystem in one enormous file to avoid modules. Split at
meaningful responsibility, ownership, dependency, or conceptual
boundaries. A common internal ordering is principal type, principal
impl, supporting types, error types, private helpers, tests — but do not
enforce this mechanically when another organization is clearer.

### Crate design

A new crate requires a real reason: dependency isolation, distribution
boundary, capability/security boundary, an independently useful library
boundary, a compilation boundary with measurable value, or an external
protocol boundary. Invalid reason: _this class/interface had its own
TypeScript file_. The workspace is `crates/siralos-core`,
`crates/siralos-adapters`, `crates/siralos-cli`; no placeholder or
hypothetical domain crates exist. The target remains a modular monolith.

## Collections and determinism

Choose collections based on semantics — not `HashMap` by default.
Consider stable ordering, deterministic iteration, lookup complexity,
insertion behavior, memory usage, and expected size. Siralos
deterministic host decisions must not accidentally depend on randomized
hash iteration order. Where ordering affects observable behavior, make
ordering explicit. Do not globally replace every `HashMap` with an
ordered collection; use deterministic normalization/sorting at the
appropriate boundary when that better represents the semantics.

## Paths and strings

Do not assume filesystem paths are valid UTF-8. Keep filesystem
identities in `Path`/`PathBuf`/`OsStr`/`OsString` for as long as
practical. Do not use lossy string conversion for authority, revision
identity, target equality, mutation targeting, or security decisions.
Lossy/display conversion is acceptable only for user-facing diagnostics
clearly separated from authoritative identity. Byte-oriented processing
is appropriate where text validity is not a semantic requirement. Do not
introduce `bstr` automatically; choose dependencies based on actual
need.

## Async and concurrency

Async must exist because the operation benefits from asynchronous
execution (asynchronous I/O, concurrency, cancellation, latency hiding,
or an async protocol boundary). Do not migrate every TypeScript `async`
function to a Rust `async fn`. Use synchronous Rust for synchronous
work. Do not introduce an async runtime into `siralos-core` without an
evidenced architectural requirement.

Concurrency must be explicit and bounded. Prefer ownership transfer and
message passing where appropriate; use locks where shared mutable state
is genuinely the correct representation. Do not introduce
`Arc`/`Rc`/`Mutex`/`RwLock` because ownership is difficult. For every
shared synchronization primitive, be able to answer: who owns this
state, why must it be shared, what invariant does the lock protect, and
what operations occur while locked? Parallel execution must not make
authoritative Siralos decisions nondeterministic; normalize concurrent
observations before they influence deterministic state.

## Unsafe Rust

Siralos starts from `#![forbid(unsafe_code)]` and `unsafe` is forbidden
in the foundation. Do not add unsafe Rust because it may be faster,
another project uses it, a benchmark might improve, or an FFI shortcut
is convenient. Any future exception requires an unavoidable, evidenced
requirement and the isolation, documentation (`SAFETY:` explanation),
testing, fuzzing, and benchmarking protocol in this guide. Do not
propagate unsafe assumptions across the architecture.

## Dependencies

Dependency versions are selected deliberately, not "always newest".
Before adding a dependency consider purpose, maintenance, license,
security history, transitive graph, default features, platform support,
MSRV/toolchain requirements, binary-size impact, compilation impact,
and supply-chain implications. Prefer the standard library when it
provides a clear adequate solution; disable default features when
unused; do not build inferior local implementations of complex
well-solved infrastructure; do not add dependencies for hypothetical
future milestones. Use the committed lockfile for deterministic
application builds. Do not silently upgrade unrelated dependencies as
part of ordinary implementation work.

Prefer workspace-level dependency declarations when multiple workspace
crates intentionally share the dependency and centralization reduces
drift; do not force single-crate dependencies into
`[workspace.dependencies]` when centralization provides no benefit.

## Feature flags

Feature flags represent real optional composition. Cargo features are
compile-time composition; they are not the end-user Godot installation
mechanism. Avoid feature combinations whose behavior is difficult to
reason about, and test supported feature combinations.

## Comments and documentation

### Comments

Comments explain _why_: invariants, tradeoffs, safety constraints,
performance reasons, platform behavior, protocol subtleties, and
unexpected workarounds. Do not narrate syntax. Do not remove useful
existing rationale comments merely to reduce comment count.

### Documentation

Public APIs must be intentionally documented: semantics, errors, panics,
safety, side effects, cancellation, security implications, and examples
where relevant. Compiler-enforced documentation quality is on by
default (`missing_docs = "deny"`, `broken_intra_doc_links = "deny"`).
Do not require verbose documentation for intentionally private
implementation details.

## Testing

### Test philosophy

Tests must verify meaningful behavior. Merely asserting `is_ok()` or
`is_some()` is insufficient when the contained value matters. Prefer
test names describing scenario + expected behavior (e.g.
`rejects_write_when_revision_is_stale`) over `test_revision`.

### Test structure

Literal `// Arrange` / `// Act` / `// Assert` comments are not required.
Require clear conceptual separation between setup, action, and
verification when the test is large enough to benefit; small obvious
tests need no structural comments.

### Deterministic tests

Tests must not accidentally depend on map iteration order, filesystem
enumeration order, wall-clock time, uncontrolled randomness, external
network access, or process scheduling, unless that condition is the
explicit behavior under test. Inject or normalize nondeterminism where
required by Siralos architecture.

### Test style

Cover ordinary behavior, boundary cases, invalid inputs, state
transitions, error categories, stale state, platform-sensitive behavior,
and meaningful configuration combinations. Prefer compact reusable
fixtures over duplicated setup; use table/matrix tests when multiple
configuration knobs must preserve the same behavior. Do not generate
enormous abstraction frameworks to avoid several clear test cases.

### Property testing and fuzzing

Consider property tests or fuzzing for parsers, protocol decoding,
revision handling, canonical serialization, path normalization,
state-machine transitions, structured mutation, and security-sensitive
input boundaries. Fuzzing complements deterministic regression tests; it
does not replace them. Do not fuzz trivial getters/setters.

### Documentation tests

Public examples should compile where practical; examples represent real
supported behavior. Do not maintain pseudo-code presented as compilable
Rust.

## Formatting and linting

### Rustfmt

rustfmt is authoritative with the repository-owned configuration; do not
manually format around it. CI runs `cargo fmt --all --check` and never
auto-formats the repository.

### Clippy

Clippy is a required quality gate with warnings denied:

```text
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

Do not enable the complete `restriction` group. Select individual lints
only where they encode a useful Siralos engineering policy: ask whether
the lint prevents a known class of defect in Siralos or merely imposes
someone's preference. Style preference alone is insufficient.

### Lint suppression

Fix the warning by default. A narrow lint suppression is permitted only
when the code is intentionally correct and changing it would make the
implementation less clear, less correct, or materially worse.
Suppressions must be local, narrowly scoped, and justified where
non-obvious — never broad crate-level `allow` merely to make CI green.
Review existing suppressions periodically.

### Compiler warnings

Compiler warnings are CI failures. Do not globally suppress
`dead_code`, `unused`, or `unreachable_code` to preserve unfinished
migration scaffolding; remove dead speculative code. If a future
placeholder genuinely must exist, document and isolate it rather than
weakening warnings globally.

## Performance

Siralos maintains an evidence-first performance rule. Review allocation,
cloning, repeated I/O, parsing, hashing, serialization, synchronization,
process creation, and buffering — but optimize in this order:

```text
correctness
→ invariants
→ ownership
→ architecture
→ measurement
→ optimization
→ re-measurement
```

Do not import micro-optimizations from unrelated Rust projects without
evidence they matter to Siralos. A faster incorrect host decision is a
regression. Optimization must never weaken determinism, security,
capability enforcement, revision correctness, evidence provenance,
cancellation semantics, approval binding, validation, or acceptance.

## Security style

Capability enforcement remains explicit. Do not encode authorization in
comments, naming, instructions, documentation, or assumed call ordering;
security is enforced by host-owned state and code. Lower-trust layers
must not acquire authority through generic helper APIs.
Security-sensitive methods make the authority boundary visible.
Derived or custom `Debug` implementations must not leak credentials,
provider secrets, sensitive tokens, or private workspace content;
review security-sensitive types before deriving `Debug`, and use
redaction or omit diagnostic traits where appropriate.

## Derives

Do not require all types to derive `Debug`/`Clone`/`PartialEq`/`Eq`/
`Serialize`/`Deserialize` automatically. Derive traits when their
semantics make sense: `Debug` when useful (reviewing sensitive fields),
`Clone` only when semantic copying is legitimate, `PartialEq`/`Eq` when
equality has clear semantics, and `Serialize`/`Deserialize` only at
actual serialization boundaries. Do not make internal types serializable
merely for convenience.

## Anti-absolutism

Engineering rules that exist only because an external style guide says
"always" or "never" must be evaluated against Siralos's actual
semantics. Absolute rules are appropriate for genuine invariants such
as: model cannot grant authority; core must remain domain-neutral;
approval cannot authorize changed content; required sandbox unavailable
→ fail closed. They are usually inappropriate for stylistic choices such
as: never use tuples; always use turbofish; always use iterators; never
use bool; never use wildcard matches; always use `AsRef`; always derive
`Clone`/`Debug`/`Eq`/`Serialize`; all tests must contain literal
Arrange/Act/Assert comments; `#[allow]` is always forbidden; always
select the latest dependency version. Each may be useful in particular
contexts; none is a universal Siralos invariant.

## Executor-generated Rust

Because Siralos is developed partly through LLM coding executors, before
finalizing Rust code the executor must review whether it introduced:

- TypeScript-shaped abstractions
- unnecessary traits
- unnecessary `Arc`
- unnecessary locks
- unnecessary clones
- unnecessary owned strings
- stringly typed state
- groups of boolean state flags
- gratuitous generics
- unnecessary async
- pass-through wrappers
- speculative factories/managers
- unnecessary public visibility

This is a review checklist, not an instruction to remove legitimate
constructs.

## Review checklist

For each meaningful Rust implementation task, consider: correctness;
Siralos invariants; invalid states; ownership; borrowing; visibility;
errors; panic behavior; allocation; cloning; async necessity;
concurrency; determinism; filesystem/path handling; security; tests;
documentation; performance implications. Do not require a verbose report
for every trivial change; the executor should perform the review even
when the final response only summarizes material findings.

## Machine-enforceable vs human-review rules

- **Machine enforceable**: rustfmt, Clippy, warnings denied, unsafe
  forbidden, architecture dependency restrictions, documentation lints,
  dead-code warnings.
- **Review enforceable**: ownership, clone justification, state
  modeling, trait necessity, API genericity, comment quality, tuple
  readability, async necessity, module cohesion.

Do not create brittle regex checks for semantic style rules simply
because automation is possible.

## Reusable executor prompt clause

Future Rust implementation prompts should include:

```text
RUST ENGINEERING STANDARD

All Rust code created or modified by this milestone must comply with the
repository-authoritative Siralos Rust Style & Engineering Guide.

Do not mechanically translate TypeScript or other source-language patterns.

Use Rust's type system to represent meaningful invariants and state where doing
so improves correctness and local reasoning.

Before finalizing, review ownership, cloning, traits, visibility, error types,
async usage, shared state, deterministic ordering and path handling.

Any intentional deviation from the guide must be justified by repository
semantics or measured evidence rather than stylistic preference.
```

Do not paste the entire guide into every milestone prompt.

## Reusable review clause

```text
Perform the Siralos Rust self-review before acceptance.

Refactor source-language-shaped Rust where the Rust type/ownership model
provides a simpler and safer representation.

Behavioral parity does not require structural parity.

Do not make stylistic refactors unrelated to the milestone.
```
