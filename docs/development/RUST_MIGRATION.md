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
| R5        | Generic language intelligence: diagnostics, symbols, definitions, references, structural parsing, and validation                | Verified       |
| R6        | Minimal domain capability architecture and synthetic conformance domain                                                         | Verified       |
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
that rejects symlink/junction escapes, bounded complete exact reads with
EOF-verified whole-file SHA-256 identity, deterministic bounded listing
and search, the fail-closed mutation-preparation boundary, checkpoint
storage inspection and startup reconciliation over the reference metadata
layout, and the typed unavailable Git inspection boundary). The exact-read
hardening gate corrected the one-shot read assumption on both sides: the
TypeScript reference and the Rust candidate now use bounded complete-read
semantics (one short read is never treated as EOF, a partial prefix is
never returned as complete, and files over the bound yield the typed
too-large outcome), with deterministic short-read regression, boundary,
and whole-file identity tests on both implementations. Workspace
file-state reads for checkpoint reconciliation and undo inspection
resolve the canonical parent inside the workspace and fail closed on any
escape, so a corrupted or malicious checkpoint record can never widen
read scope. The differential corpus gained 23 scenarios across the
`workspace-read`, `workspace-list`, `workspace-search`,
`workspace-revision`, `workspace-prepare`, `checkpoint`, and
`git-inspection` subjects, executed by both implementations under the R2
protocol (corpus schema 3, corpus version 7, 47 scenario files); all
required applicable scenarios match, the harness self-tests and replay
stress pass, and the complete local repository gate passes. Deliberately
unavailable effects (mutation application, new checkpoint creation, Git
inspection) report the same typed outcomes on both sides. R4 verification
does not authorize R5 work or satisfy any later migration or Stage-4
entry gate.

R5 is verified: the generic language-intelligence foundation lives in
`siralos-core::language` (one-based positions/ranges with typed
validation and explicit 0-based LSP conversion at the adapter boundary;
the bounded sanitized diagnostic model with closed severities,
deterministic dedup/ordering, and explicit truncation; generic
symbol/definition/reference query models with deterministic ordering
and bounds; the language-neutral structural-document representation
with the deterministic byte-bounded advisory summary formatter; typed
validation result semantics that never conflate source-invalid with
infrastructure failure; the reference-extracted generic limits; and R4
revision binding throughout) plus the generic language-service URI
mapping in `siralos-adapters::language::uri`. The TypeScript reference
gained matching generic language modules
(`packages/core/src/language`: position, sanitize, truncate,
diagnostic, definition, structure) extracted behavior-preservingly from
the Godot diagnostics/LSP normalization code, which now consumes them;
all existing TypeScript tests stay green. The generic structural
representation is language-neutral by construction: `siralos-core::language::structure`
owns only cross-language kinds (type/function/method/field/variable/
constant/enum/event/module/other), opaque attributes/modifiers, and
generic advisory summary wording, and carries no GDScript/Godot
semantics (no signal/property/function info models, no `extends`/
`class_name`, no annotation or `export` interpretation); the
GDScript-specific scanner and summary remain exclusively the
TypeScript reference for R8/R9. The differential corpus gained 16
scenarios across the `language-diagnostics`,
`language-structure`, and `language-definition` subjects (corpus
schema 3, corpus version 9, 63 scenario files) executed by both
implementations under the R2 protocol, with language-neutral structure
fixtures; all required applicable scenarios match, the harness
self-tests and replay stress pass, and the complete local repository
gate passes. R5 deliberately does not
port GDScript/Godot parsing (the GDScript structural scanner remains
the TypeScript reference for R8/R9), LSP/JSON-RPC transport, language
process execution, provider tool integration, or Domain architecture;
execution-dependent language intelligence reports typed unavailable
dispositions. R5 verification does not authorize R6 work or satisfy
any later migration or Stage-4 entry gate.

Bounded recovery is a Host/run design property, not a milestone subsystem.
R11 remains the owner of full recovery parity (table row R11). Earlier
milestones preserve typed failure information needed by later recovery:
typed Host-observed failure state must stay distinguishable
(retryable / non-retryable / capability denied / stale/conflict / resource
exceeded / unavailable/unsupported / uncertain / invalid / terminal) so
recovery decisions never depend on substring matching or model judgment.
R6 inherits only typed-failure and recovery-readiness constraints for its
Domain boundary and does not implement recovery orchestration.

R6 is verified: the minimal Domain capability architecture lives in
`siralos-core::domain` (validated package identity: stable id, exact
SHA-256 package digest, versioned ABI; the explicit
absent/installed/enabled/active state machine with typed transitions;
declared capability requests and the Host-authoritative grant decision;
exact activation binding; typed recovery-ready failure outcomes with
stable codes; and the explicit absence of implicit acquisition —
workspace contents are opaque to the lifecycle) and the production
Component Model / WIT boundary lives in `siralos-adapters::domain`
(the versioned `siralos:domain-abi@1.0.0` world in
`crates/siralos-adapters/wit/domain-abi.wit`; component
loading/instantiation with the versioned export-identity check so
unknown or incompatible ABI versions fail closed; exact-byte digest
verification at install and activation; fuel/memory/input/output/
host-call bounds; trap containment with typed fault outcomes and
session stop; and host-mediated effects: grant-checked bounded
workspace reads, process execution denied). The deterministic synthetic
conformance Domain (`tests/domain-conformance/`) is product-neutral and
proves the production boundary on the checked-in component bytes,
including trap and unbounded-loop pathological behaviors. The
differential corpus gained 23 scenarios across the `domain-lifecycle`
and `domain-capability` subjects (corpus schema 3, corpus version 11,
86 scenario files) executed by both implementations under the R2
protocol; all required applicable scenarios match, the Rust Component
conformance suite passes, and the complete local repository gate
passes. R6 implements no Plugin system, no marketplace, no automatic
acquisition, no provider/tool integration, and no Godot Domain; it
makes the Domain boundary recovery-ready without implementing recovery.

The R6 remediation gate hardened the activation path: activation is
transactional (prepare/commit — every fallible runtime step, including
guest bind, runs before the authoritative Enabled -> Active
transition, so a failed activation can never leave the lifecycle
Active without a session), Core rejects Active -> Active with the
typed active failure and reports active eligibility as not ready,
activation requests are mechanically bounded by the installed
package declared capabilities (typed undeclared-capability failure
with canonical ordering; a request may only narrow the declaration),
semantic output obeys one aggregate byte bound over the complete
returned representation (guest rejection reasons and effect-answer
payloads included), and Host-call exhaustion is a Host-observed
typed ResourceExceeded(HostCalls) failure independent of the bounded
guest disposition. Every prepared activation is additionally bound to
the lifecycle generation validated at preparation: every successful
material transition (install, uninstall, enable, disable, activation
commit, deactivate) advances the generation, and commit revalidates
the generation, the Enabled state, and the complete exact package
binding (stable id, exact digest, and ABI) before any mutation, so a
stale preparation fails typed (`STALE_ACTIVATION`) without state
change, session-id consumption, or a published HostSession. Activation
identity is exact in all three dimensions: the request ABI must
identify the installed package ABI (a Host-compatible request can
never substitute for a differently declared package ABI — typed
identity mismatch) and must also satisfy Host compatibility (typed
unsupported-ABI failure), so every successful activation satisfies
`ActivationBinding::matches(installed_package)` by construction.
R6 verification does not authorize R7 work or satisfy any later
migration or Stage-4 entry gate.

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
