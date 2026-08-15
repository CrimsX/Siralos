# Siralos Rust migration track

Status: authoritative migration sequence.

Stage 3R is an internal migration track. It does not replace Siralos's six
public product stages. The TypeScript implementation remains the behavioral
reference until R12's retirement gate is satisfied.

Stable cross-project requirement identifiers are repository-owned in the
[normative requirements register](../requirements/REQUIREMENTS.md). The
[RFC ownership index](../architecture/RFC_INDEX.md) and
[golden trace registry](GOLDEN_TRACES.md) preserve their decision and
verification work items without manufacturing duplicate documents. Similar
repository concepts never satisfy those identifiers by implication.

## Status vocabulary

- **Verified**: implemented, wired, and supported by executable evidence.
- **Remediation**: implemented surface exists, but a blocking acceptance or
  verification defect remains.
- **Not due**: sequenced after the active milestone.

## R1-R12 sequence

| Milestone | Scope                                                                                                                           | Current status |
| --------- | ------------------------------------------------------------------------------------------------------------------------------- | -------------- |
| R1        | Siralos rename, Rust workspace, and engineering standards                                                                       | Verified       |
| R2        | Differential Behavioral Harness                                                                                                 | Verified       |
| R3        | Domain-neutral core: authoritative task, state, acceptance, evidence, identity, and transition semantics                        | Verified       |
| R4        | Generic workspace and project foundation: reads, revisions, search, prepared effects, checkpoints, and optional Git integration | Verified       |
| R5        | Generic language intelligence: diagnostics, symbols, definitions, references, structural parsing, and validation                | Not due        |
| R6        | Minimal domain capability architecture and synthetic conformance domain                                                         | Not due        |
| R7        | Provider, tool-loop, projection, configuration, and CLI parity                                                                  | Not due        |
| R8        | Optional Godot Stage-2 parity                                                                                                   | Not due        |
| R9        | Optional Godot Stage-3 parity                                                                                                   | Not due        |
| R10       | H1 content identity, H2 determinism/replay, ICM context, and H3 runtime-readiness parity                                        | Not due        |
| R11       | Full differential, effect-boundary, security, recovery, and cross-platform parity                                               | Not due        |
| R12       | TypeScript reference retirement or an explicit evidence-backed retention disposition                                            | Not due        |

R2 is verified: its versioned corpus is mechanically bound by scenario and
overall digests; its reference and candidate runners have symmetric bounded
lifecycle supervision; and its semantic comparator reports typed JSON-path
differences. The audit records source, corpus, and exact protocol-document
provenance plus explicit scenario and per-subject coverage, and the complete
local repository gate passes. The Tier-1 CI matrix reruns parity on Linux,
Windows, and macOS and retains the exact reference, candidate, audit, and
failure records; a failed matrix run blocks integration.

R3 is verified: the domain-neutral host-owned task kernel lives in `siralos-core` (revisioned contracts with the reference content-digest
contract, materialized authoritative state, the explicit phase transition
table, bounded evidence with exact contract-revision/digest binding,
host-owned acceptance, the completion gate, terminal immutability,
append-only activity, and host-observed progress). The corpus gained 17
`task-contract` scenarios executed by both implementations under the R2
protocol; all required applicable scenarios match byte-for-byte and the
complete local repository gate passes. R3 verification does not authorize
R4 work or satisfy any later migration or Stage-4 entry gate.

R4 is verified: the generic workspace/project foundation lives in
`siralos-core` (validated workspace-relative paths with NUL/absolute/drive/
traversal rejection and protected-path classification, the reference
bounds, deterministic revision handles and the bounded session registry,
typed prepared create/edit/delete effect models, the checkpoint model with
operation-state invariants, undo planning, and reconciliation
classification, and the read-only Git error/disposition contract) and
`siralos-adapters` (canonical root resolution, containment-safe resolution
that rejects symlink/junction escapes, bounded exact reads with
whole-file SHA-256 identity, deterministic bounded listing and search,
the fail-closed mutation-preparation boundary, checkpoint storage
inspection and startup reconciliation over the reference metadata layout,
and the typed unavailable Git inspection boundary). The differential
corpus gained 19 scenarios across the `workspace-read`, `workspace-list`,
`workspace-search`, `workspace-revision`, `workspace-prepare`,
`checkpoint`, and `git-inspection` subjects, executed by both
implementations under the R2 protocol (corpus schema 3, corpus version 6);
all required applicable scenarios match, the harness self-tests and replay
stress pass, and the complete local repository gate passes. Deliberately
unavailable effects (mutation application, new checkpoint creation, Git
inspection) report the same typed outcomes on both sides. R4 verification
does not authorize R5 work or satisfy any later migration or Stage-4
entry gate.

## Porting gate

Every R3-R11 subsystem follows the same sequence:

```text
behavior extraction
-> idiomatic Rust design
-> differential and effect parity
-> security and architecture review
-> measurement
-> milestone acceptance
```

Matching names or independently passing tests are not parity. The real Rust
execution path, failure semantics, authority boundaries, deterministic
decisions, and final effects must be compared with the TypeScript reference.

Refactoring during a port is expected when it removes TypeScript/Node
accidental complexity. Broad refactoring of the reference ahead of its port is
discouraged because it changes the migration oracle without advancing the
candidate.

### Lean porting discipline (ADR 0036)

R3 remains **Domain-Neutral Core**, but the porting guidance is explicit:
R3 must not mechanically port planning, orchestration, context-projector
proliferation, workflow abstractions, agent hierarchies, or other
TypeScript-era structures when their required behavior can be represented by
fewer idiomatic Rust types. Behavioral parity does not require structural
parity (ADR 0032); the lean product model (ADR 0036) additionally forbids
designing core around orchestration layers, generic workflow engines,
multi-agent machinery, or extension abstractions that are not committed
product concepts. Evaluate each ported surface as Profile behavior, a Skill,
an ordinary model artifact, or a small Host contract before mirroring a
TypeScript structure (for example the planning contracts), and prefer the
smallest structure that preserves required observable behavior, authority,
and determinism.

### Rust leverage principle

Siralos uses Rust's ownership, borrowing, enums, exhaustive matching,
strong types, generics, and zero-cost abstractions to make the Host simpler
and more efficient by construction. The migration must not preserve
source-language allocation, concurrency, serialization, or object patterns
when Rust provides a simpler representation. Performance-sensitive code is
measured before specialized optimization. Unsafe code, synchronization,
dynamic dispatch, unnecessary allocation, and unnecessary async require
concrete justification. The full standard is
[RUST_STYLE.md](RUST_STYLE.md); do not create a separate optimization
milestone.

### Port review clause

Every ported subsystem review must check:

- states represented with types instead of flag combinations;
- one clear owner for mutable authoritative state;
- borrowing rather than cloning where natural;
- no internal JSON/serialization churn without a real boundary;
- no unnecessary heap allocation;
- no unnecessary dynamic dispatch;
- no unnecessary `Arc`/`Mutex`/`RwLock`;
- no unnecessary async;
- deterministic collection/order semantics;
- no repeated parsing/hashing/canonicalization without reason;
- standard zero-cost abstractions preferred over low-level tricks;
- future revision-aware incremental reuse remains possible.

Do not micro-optimize without measurement.

## Stage 4 entry

Stage 4 begins only after R1-R11, the Stage 1-3 migration audit, R12's
retirement/retention disposition, and
[the Stage-4 entry gate](stage4-entry-gate.md) all pass.

The first Stage 4 capability is generic **Controlled Runtime Execution**. It
supervises a host-authorized process and produces structured runtime evidence;
it is not a Godot-specific launcher. The intended Stage 4 sequence is:

1. Controlled Runtime Execution
2. Runtime Evidence
3. Godot Runtime Adapter
4. Visual Evidence
5. Controlled Interaction
6. QA Workflows
7. Profiling

Godot is the first specialization to consume the generic runtime boundary.
It does not define that boundary in `siralos-core`.
