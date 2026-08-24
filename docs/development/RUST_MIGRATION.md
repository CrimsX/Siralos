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
- **Active**: sequenced and currently in progress (behavior extraction,
  remediation, or implementation); not yet Verified.
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
| R7        | Provider, tool-loop, projection, configuration, and CLI parity                                                                  | Verified       |
| R8        | Optional Godot Stage-2 parity                                                                                                   | Verified       |
| R9        | Optional Godot Stage-3 parity                                                                                                   | Verified       |
| R10       | H1 content identity, H2 determinism/replay, ICM context, and H3 runtime-readiness parity                                        | Verified       |
| R11       | Full differential, effect-boundary, security, recovery, and cross-platform parity                                               | In progress    |
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
`ActivationBinding::matches(installed_package)` by construction. A
prepared activation never carries authority across Host policy
contexts: the final capability grant is recomputed at commit from the
commit-time Host authority (plus the supported ABI and runtime policy
of the authorizing Host), so a narrower final authority fails typed
(`CAPABILITY_DENIED`) with zero mutation and zero session
consumption, a wider final authority can never widen the activation
request, and equal lifecycle generations provide no
authority-transfer power. Provisional-effect authority is
mechanically proven through the production Component boundary: a
guest CAN invoke host-mediated effects during bind, before the
final commit, and those effects are mediated by exactly the grant
the final commit authorizes, because `DomainHost::activate()` is
synchronous with an immutable Host authority for the whole call
(the conformance guest exercises permitted and out-of-grant
bind-time effects deterministically).
R6 verification does not authorize R7 work or satisfy any later
migration or Stage-4 entry gate.

R7 is Active. Its first stage, R7A, is complete: the R7 behavior extraction
(`docs/development/R7_BEHAVIOR_EXTRACTION.md`) freezes the five R7 surfaces
(provider, tool loop, projection, configuration, CLI) with the R7.1/R7.2
evidence boundary; the provider runtime-event boundary was hardened in the
TypeScript reference before porting (reference protocol hardening discovered
during R7 behavior extraction) so both collectors reject unknown or malformed
event discriminators deterministically with regression coverage; the
differential corpus is unchanged (corpus version 11, schema 3, 86 scenario
files — the `provider-turn` subject lands with R7.1); and the complete local
repository gate passes with verified executable baseline
99ee902c1c61927070f1249ee16aa276eff24b2b. R7A is complete and authorizes
R7.1 to begin under the R7 porting gate. This authorization is limited to
R7.1: it does not satisfy R7 overall, authorize R7.2+ or R8+, or satisfy any
Stage-4 entry gate.

R7.1 is complete: the provider contract and the bounded single model turn
live in `siralos-core::provider` (provider-neutral `ModelRequest`/
`ModelEvent`/`ConversationItem`/`ToolDefinition`/`ToolExecutionResult`
types; the external event trust boundary `validate_external_event` — the
discriminator is authoritative and unknown or malformed runtime events
fail closed before typed acceptance; the seven frozen turn dimensions with
inclusive bounds; the shared `BoundedTurnState` accounting core; the
application collector with transcript validation before provider use,
strict provider-order consumption, completion/EOF/cancellation precedence,
execute/invalid tool-call proposals with deterministic `invalid-call-N`
synthetic ids, and typed failures whose external messages match the
reference exactly; transcript pairing validation; and the bounded
tool-result detach boundary), and the deterministic fake provider plus the
strict bounded-turn collector live in `siralos-adapters::provider`
(identity `deterministic-fake`, deterministic echo in 16-code-point
chunks that never split a Unicode scalar value, generic
workspace.list/read/search proposals gated on tool availability,
previous-turn result isolation, cooperative cancellation). The
differential corpus gained the `provider-turn` subject with 18 required
scenarios (corpus schema 3, corpus version 12, 104 scenario files):
basic echo/tool-call turns, the exact 64 KiB assistant-text boundary,
multibyte UTF-8 accounting, the per-dimension tool-call bounds and the
aggregate 256 KiB boundary, duplicate/empty-call application semantics,
EOF-without-completion and post-completion rejection, deterministic
cancellation, unknown/malformed event protocol failures, structurally
invalid transcript blocking, and the tool-result detach family — all
required applicable scenarios hold differential parity between the
TypeScript reference and the Rust candidate
(`npm run check:differential` exit 0; 100/100 applicable required
scenarios matched). The complete local repository gate
(`npm run check`, `cargo deny check`, `git diff --check`) passes with
verified executable baseline 3a08a86605f0395244a55eaab1b8db84de22d7f7.
R7.1 verification does not satisfy R7 overall, authorize R7.2+ or R8+, or
satisfy any Stage-4 entry gate.

The R7.1 cancellation-authority remediation closed the final known
authority-boundary defect before R7.2: `ModelProvider::stream` now
receives the read-only `CancellationSignal` observation view instead of
`&CancellationToken`, so a provider can observe Host cancellation and
stop cooperatively but can never invoke the Host `cancel()` mutator —
enforced by the type boundary (the signal carries no mutation operation
and no accessor that yields the controller). Host cancellation control
strictly contains provider cancellation capability; mid-stream
cancellation tests are Host-driven through a controller-holding test
wrapper, and the deterministic fake provider, scripted provider, and
strict collector receive only the signal. Externally observable
`provider-turn` behavior is unchanged (differential parity holds, corpus
identity untouched). Proportional R7.1 measurement (from
`git diff --numstat 2c57e8f..3a08a86`): siralos-core provider production
+1,268 lines (conversation/event/result/turn/mod), siralos-adapters
provider production +565 lines (deterministic_fake/strict_turn/mod),
siralos-cli differential provider-turn path +1,015 lines (harness.rs
dispatch, validation, and records); one new direct dependency
(serde_json in siralos-core, already in the tree via adapters/cli); no
async runtime, no Arc/Mutex/RwLock, no unsafe, no dynamic dispatch;
18 `provider-turn` differential scenarios; 69 siralos-core provider
unit tests and 34 siralos-adapters provider unit tests. The complete
local repository gate passes with verified executable baseline
bd335190696f3662e242407c89d7821483d7fa24. R7.1 verification does not
satisfy R7 overall, authorize R7.2+ or R8+, or satisfy any Stage-4 entry
gate.

### R7.1 final acceptance

Comparison base: `2c57e8f` (docs: authorize R7.1 after R7A closure) — the
commit immediately before the R7.1 implementation commits began. Final
executable: `bd335190696f3662e242407c89d7821483d7fa24` (the cancellation-
authority remediation commit; the complete repository gate passes on that
exact worktree).

Production Rust added by R7.1 (`git diff --numstat 2c57e8f..bd33519`):

```text
siralos-core provider production      1,310 lines (conversation 182, event 445,
                                      mod 51, result 259, turn 373)
siralos-adapters provider production    571 lines (deterministic_fake 284,
                                      mod 23, strict_turn 264)
siralos-cli provider-turn integration 1,024 added / 6 removed in harness.rs
                                      (candidate dispatch, validation, and
                                      canonical records; the file mixes
                                      production code and unit tests)
test code (excluded from production): core provider tests.rs 1,350; adapters
                                      provider tests.rs 833
```

Dependency delta: one new direct Core dependency — `serde_json` (an existing
workspace dependency newly referenced directly by siralos-core; already in the
tree via adapters/cli; no new crate entered Cargo.lock). No new direct
adapter or CLI dependencies.

Concurrency/authority delta: async runtime — no; threads — no; Arc — no;
Mutex — no; RwLock — no; atomics — no; unsafe — no (verified by the
`unsafe_code = "forbid"` lint and a token audit of both provider modules).
Dynamic dispatch — no (the provider seam uses a generic associated iterator
type, static dispatch); `Box<dyn>` — none. JSON usage is boundary-justified:
`serde_json::Value` at the external event validation boundary, serialized
Tool-argument byte accounting, Tool-result detach size accounting, and
differential canonicalization — no internal serialize/parse churn.

Determinism: observable ordering is owned by `Vec` order (messages, tools,
events, tool calls, text deltas), a `BTreeSet` of seen call ids (ordered;
iteration never observed), and explicit `invalid-call-N` numbering. No
HashMap-iteration, filesystem-enumeration, wall-clock, randomness,
environment-ordering, or thread-scheduling dependence.

Evidence counts: 69 siralos-core provider unit tests, 34 siralos-adapters
provider unit tests (`cargo test -p <crate> --lib provider -- --list`), 18
`provider-turn` differential scenarios, 104 total corpus scenarios.

Security/architecture review: PASS. Provider output remains untrusted
proposal/data; raw malformed provider events fail `validate_external_event`
before typed `ModelEvent` acceptance; the provider cannot grant capability,
register or execute a Tool, mutate the workspace, mutate Host cancellation
state, or decide Host Context; the Host owns request composition, Tool
surface, cancellation control, protocol acceptance, limits, and the turn
outcome; `TurnToolCall` remains a typed proposal for R7.2. Port-review
checklist: PASS — states are typed enums, one owner for bounded-turn mutable
state, borrowing where natural, no internal JSON churn without a boundary,
no unnecessary heap allocation, no dynamic dispatch, no Arc/Mutex/RwLock,
no async, deterministic collection/order semantics, no repeated
parsing/hashing without reason, zero-cost abstractions, no unsafe, providers
receive least authority, providers cannot mutate Host cancellation, the seam
stays SDK-free for future real providers, and future R7.3 Context can feed
`ModelRequest` without providers owning Context selection. No performance
benchmark was required: R7.1 is not identified as a performance-sensitive
hotspot.

### R7.2 entry review (authorization)

The R7.2 entry review (docs/development/R7_BEHAVIOR_EXTRACTION.md §13,
recorded at HEAD 4dd8aea3c6394be3d751890ccb30c8c33a185364) returned
**PASS**: the generic Application Tool Loop contract is frozen — tool
registry (immutable, registration-ordered, duplicate rejection, exact
case-sensitive lookup, capability metadata), the generic Tool execution
seam (definition + capability + execute with read-only CancellationSignal),
round execution with the one-call-one-result pairing invariant, invalid-call
failed-result pairing, round cancellation with deterministic skipped-call
results, tool-round budget normalization (default 8, hard max 32,
clamp/floor, the over-budget round never executes, exact cap message),
single-flight via a typed Idle/Responding state without concurrency
primitives, terminal response/completion/cancellation/failure propagation,
per-call Host authorization recheck, the narrow approved-tool-surface
projection seam (no R7.3 implementation), the closure of the generic event
surface (response_* + text_delta + tool_* only), and the classification of
every TypeScript co-located branch (prepared mutation/command → R11; Godot
probe/diagnostic/LSP → R8/R9; approval/checkpoint/command audit →
their owning effect milestone; command/audit/pending-approval state →
not ported). The next implementation slice is R7.2 — Application Tool Loop
parity — with the frozen `tool-loop` subject (16 required differential
scenarios per §13.18: the original set plus `tool-loop.authorization`,
`tool-loop.display-input`, and `tool-loop.tool-result-statuses`) and the
Core/Adapter unit-test evidence plan recorded in §13.18-13.19. The
independent-review remediation (docs: close R7.2 independent-review
contract gaps) additionally froze the domain-neutral Core capability
representation (§13.7/§13.17/§13.21), the exact `invalid_input` boundary
(§13.9), the exact UTF-16-code-unit `displayInput` truncation and its
200/201/multibyte evidence (§13.10/§13.18), and direct differential
coverage for capability deny, plain-Tool ask, and the Tool-result status →
event mapping (§13.18).
This authorization is limited to R7.2: it does not satisfy R7 overall,
authorize R7.3+ or R8+, or satisfy any Stage-4 entry gate.

Status at the R7.2 entry-review point: R1-R6 Verified; R7 Active; R7A
Complete; R7.1 Complete (implementation, differential parity, cancellation
authority review, measurement, acceptance); R7.2 Authorized (entry review
PASS, contract frozen) — next implementation slice; R7.3+ not authorized;
R8-R12 not due. The current status is recorded after the implementation
sections below; R7 is not marked Verified.

### R7.2 implementation and acceptance

R7.2 is complete against the frozen §13 contract. `siralos-core::tool`
owns the domain-neutral capability identifier (`CapabilityId`, validated
lowercase ASCII letters/digits with `.`/`_`/`-` separators, 64-byte
bound), the generic allow/ask/deny permission evaluator, the immutable
registration-ordered Tool Registry (duplicate construction failure, exact
case-sensitive lookup, fresh detached definitions), the one generic Tool
seam (`definition` + `capability` + `execute(input, CancellationSignal)`),
the approved-visible-surface guard, the Tool Round (pre-seeded transcript,
invalid-call failed pairing, one-call/one-result, cancelled-tail pairing),
and the synchronous pull-based Application loop (typed single-flight,
provider iteration, exact budget normalization, history ownership, the
closed nine-event R7.2 surface). `siralos-adapters::tool` wraps the R4
workspace primitives as `workspace.list`/`workspace.read`/`workspace.search`
Tools with owned input validation, read-only cancellation observation, and
no mutation/process/Godot authority.

The differential corpus gained the `tool-loop` subject with 16 required
scenarios (corpus schema 3, corpus version 13, 120 scenario files). All
required applicable scenarios match, including the frozen authorization
matrix (allow/deny/ask-plain), the displayInput UTF-16 matrix (exactly 200,
201, supplementary Unicode, surrogate-pair split, and multi-key object
source order), and the Tool-result status to event matrix. The Rust
candidate composes the real registry, permission evaluator, approved
surface, round, application loop, R7.1 provider-turn collector, and the
production workspace read adapter; harness-local stub Tools enter through
those real seams. No serde_json feature changed: source-ordered
`displayInput` uses the narrow `ToolCallInput` ordered-JSON sidecar at the
typed provider-event boundary, so repository canonicalization remains
BTreeMap-backed.

Security review (§13.21) is PASS: provider proposals, registry membership,
and visibility grant no authority; authorization is rechecked per call;
unknown/hidden/deny/ask/invalid/cancelled calls execute zero Tools; Tools
receive only read-only cancellation and cannot mutate Host history; no
automatic retry exists; one-call/one-result always holds; Core has no
optional-domain semantic capability variants; and no mutation/process/Godot
authority was pulled forward. No async runtime, threads, Arc, Mutex,
RwLock, atomics, or unsafe code was introduced. Dynamic dispatch is
confined to the heterogeneous registry boundary (`Box<dyn Tool>` entry
storage; the loop receives `&dyn Tool` only at the lookup/call seam).

Measurement (`git diff --numstat 1812409..<final executable SHA>`,
authorization baseline `1812409`): production Rust added 2,287 lines in
`siralos-core`, 683 lines in `siralos-adapters`, and 1,155 lines in
`siralos-cli` (differential path), with 50 lines removed across the three
crates (dedicated `tests.rs` files are excluded; the small inline test
modules in `workspace_tools.rs` and `tool_loop.rs` remain in their file
totals); 41 Core Tool-loop tests, 6 adapter Tool tests, and 2 candidate
tool-loop harness tests. No new direct dependencies and no serde_json
feature/dependency change. Async runtime: no. Threads: no. Arc: no.
Mutex: no. RwLock: no. Atomics: no production use (test counters use
`Cell`/`Rc` in harness-local fixtures only). Unsafe: no. Dynamic dispatch:
`Box<dyn Tool>` is confined to the heterogeneous Tool Registry entry
boundary (and the candidate's instrumentation wrapper); the loop receives
`&dyn Tool` only at the single lookup/call seam. Differential corpus:
16 required `tool-loop` scenarios; 120 total scenario files (corpus
version 13). No performance claim or benchmark is made.

Status at the R7.2 acceptance point: R1-R6 Verified; R7 Active; R7A
Complete; R7.1 Complete; R7.2 Complete (evidence-backed); later R7.3+
slices were not yet implemented at that historical point. The current status
is recorded in the closure sections below; R7 remains Active and is not
marked Verified.

### R7.3 pre-port projection oracle correction closure

The independent R7.3 entry review was interrupted by a defect in the
TypeScript projection oracle before any Rust R7.3 implementation existed.
Commit `4b805d4ac0a9eac6d6de5a2b90b64bc6146aeafc` (`fix(core): preserve
Unicode boundaries in evidence projection`) corrected the reference before
Rust porting resumed. Its valid-Unicode-boundary search prevents UTF-16
surrogate-pair splits in UTF-8-bounded line and total-truncation projection;
the focused projection regressions passed, and the correction changes no
Host authority, raw evidence/history, tool result, capability, or provider
contract.

The corrected TypeScript oracle is the source for a restarted independent
R7.3 entry review. At the time of this historical entry, this closure did
not implement R7.3, add a projection differential subject, promote the corpus,
or authorize R7.3. The latest
verified executable worktree is `4b805d4ac0a9eac6d6de5a2b90b64bc6146aeafc`;
the historical R7.2 verified Rust implementation baseline remains
`73db8e89c8f670454927ca7ed7554e17d33ea606`. Documentation-only reconciliation
after the executable correction does not replace either evidence pointer. The
completed entry review is recorded in R7_BEHAVIOR_EXTRACTION.md §14.

### R7.3 entry review and authorization

The restarted independent R7.3 projection entry review is recorded in
`docs/development/R7_BEHAVIOR_EXTRACTION.md` §14. It audited the corrected
TypeScript oracle, all projection/context/application/security seams and
relevant tests. The review froze the generic
capacity/estimator/pressure/reduction/segment/evidence/Tool contracts, the
corrected Unicode-scalar boundary behavior, R7.2 ApprovedToolSurface and R7.1
provider-request ownership, the differential/security evidence plan, and the
minimal domain-neutral Rust ownership proposal. Its original result was
**PASS — R7.3 Projection contract frozen; implementation authorized**.

The follow-up independent review of the integrated remediation lineage
(`461f290b3d3d778a3bef4d25a895338efcdf315c` and
`ea145a14a89fb5e6b9e2988eddb97d65d2e37793` reconciled in §14.4 and pinned in
`8e5384c6b188cbaf314f9e72daa8b89368bbd1c8`) has now returned **PASS** on that
lineage and the reconciled §14 contract. At that documentation point, R7.3
remained contract-frozen and was authorized as the next implementation slice;
no R7.3 executable Rust code, differential corpus promotion, R7.4/R7.5 work,
or R8-R12 work was included. The current implementation is recorded below.

### R7.3 integrated evidence-line oracle remediation

Independent review of `c474d725f1c66ea78030c382e33fc06382b5728b` found that
`boundLineLength()` enforced its scalar-safe split in isolation, while
`projectForModel()` could discard that split because inserted newlines increase
JavaScript string length and the never-worse guard restored the pre-reduction
text. The remediation classifies line bounding as a mandatory structural
model-view bound and keeps repeated-line collapse as the only optional
never-worse reduction. Security transforms remain non-revertible; the final
order is strip controls, redact secrets, optionally collapse, enforce the
UTF-8 line bound, then truncate total bytes.

Executable commit `461f290b3d3d778a3bef4d25a895338efcdf315c`
(`fix(core): enforce integrated evidence line bounds`) adds integrated
`projectForModel()` regressions for the hard bound, exact and over-limit
boundaries, Unicode scalar behavior, security composition, optional collapse,
and terminal truncation. The full required gate passed on that clean
executable tree; the differential corpus remains unchanged at schema version
3, corpus version 13, 120 scenario files, digest
`6a5be95acb3ff8a714da39aef206770796987ff8910dc9bd8dd58f4b72246490`.

The follow-up executable/test commit is
`ea145a14a89fb5e6b9e2988eddb97d65d2e37793`
(`test(core): pin truncation-marker line-bound precedence`). It pins the
terminal-marker exception without changing production projector behavior: the
marker remains whole even when its LF-delimited line exceeds a deliberately
tiny `maxLineBytes`, while ordinary pre-truncation evidence remains bounded.
The complete required gate passed on that exact clean executable/test tree.

The latest verified executable worktree is now
`ea145a14a89fb5e6b9e2988eddb97d65d2e37793`. The integrated line-bound
production correction remains preserved at
`461f290b3d3d778a3bef4d25a895338efcdf315c`, the earlier Unicode helper
correction remains preserved at
`4b805d4ac0a9eac6d6de5a2b90b64bc6146aeafc`, and the historical R7.2 verified
Rust implementation baseline remains
`73db8e89c8f670454927ca7ed7554e17d33ea606`. The documentation-only
reconciliation after this executable/test commit is not the executable
baseline. Independent review of the `461f290`/`ea145a1`/`8e5384c` lineage and
reconciled §14 contract had returned PASS; R7.3 was authorized as the next
implementation slice and remained not yet implemented at that documentation
point. The current implementation and acceptance are recorded below.

### R7.3 implementation and acceptance

R7.3 Projection parity is complete and evidence-backed. The reviewed
implementation beginning at `8d70f7b2f0dc1b43339dc20b2a986daba6f185d3` was
closed by `51ed40d` (`fix(core): close R7.3 projection evidence`), which
removed the vacuous acceptance fallback and replaced the inaccurate policy
mutation test with a precise projected-surface recheck assertion. The
focused `siralos-core` projection suite passes 13 application/integration
tests; the differential corpus carries 11 required `context-projection`
scenarios. The production path remains disposable projection over
authoritative history, with no execution authority, mutation, async runtime,
threads, locks, or new direct dependencies.

The §14.19 proportional measurement is recorded from the reviewed
implementation range `1a3aceb..8d70f7b`: 3,208 added Rust source-file lines
in `siralos-core`, no added `siralos-adapters` lines, and 745 added
`siralos-cli` differential/harness lines; the range removes 43 Core lines and
3 CLI lines. Those file totals include the reviewed projection test modules
(`projection_tests.rs` and `adversarial.rs`); the production projection
surface is the `projection` module plus the integrated `tool/session.rs`
seam. The range adds no direct Cargo dependency and changes no canonical JSON
or SHA-256 dependency. Production async runtime, threads, `Arc`, `Mutex`,
`RwLock`, atomics, and unsafe code: none. Dynamic dispatch remains confined
to the pre-existing heterogeneous Tool Registry seam; projection itself is
static. Stateful projection/cache ownership is one application-held
`ProjectionService` containing one revision-bound cache and one last-
projection record; provider-turn projections are recomputed from current
authoritative history. No benchmark is claimed because no projection
hotspot was established.

Security and architecture review remains PASS: the projected tool schema and
`ApprovedToolSurface` are derived coherently, per-call authorization is
rechecked, reduced context never mutates history, and projection never
grants capability.

### R7.4 configuration parity closure

R7.4 is complete and evidence-backed. `siralos-adapters::config` owns the canonical user-config path
support, lstat/symlink and non-regular-file policy, bounded complete
EOF-verified reads at the one-MiB bound, UTF-8/JSON parsing, strict nested
validation, and nonfatal semantic reference diagnostics.
`siralos-cli::configuration` owns the explicit path override, registered
`deterministic-fake` review-provider validation, fixed-order diagnostics
composition, and reference failure reporting. `siralos-core` gains no
generic configuration subsystem.

The corpus is version 15 with 133 scenario files and two required
`user-config` scenarios: the matrix covers missing/default and full values,
unknown top-level/nested keys, invalid enums, installation/reference counts,
absolute-path rejection, provider registration, invalid JSON, exact/over
one-MiB boundaries, non-regular files, and nonfatal reference failures; a
POSIX-only required symlink scenario is explicit and skipped on Windows.
The current Windows audit holds at 128/128 applicable required scenarios,
with one accepted informational deviation. R7.5 interactive CLI parity has
now been implemented as a separate CLI candidate; R7.4 remains closed and
its configuration semantics are unchanged.

### R7.5 CLI context and Tool rendering candidate

R7.5 owns the CLI presentation/composition slice described by the frozen
§14.14 and §14.17 contract. The executable candidate is implemented in
`crates/siralos-cli`: the no-argument `siralos` path composes the existing
R7.4 configuration, deterministic fake provider, read-only workspace Tool
adapters, R7.2 `PermissionPolicy`, and R7.3 `ProjectionService`. The
CLI-owned `output` module ports the exact `/context`, `/tools`, and compact
Tool-projection strings from the TypeScript oracle. It reads the current
detached `LastProjection`; it does not duplicate projection policy, create a
second Tool-authority system, or grant capability.

The application loop remains synchronous and Host-owned. Prompt execution
drains the existing `SiralosApplication` until its terminal restoration,
including Tool rounds, so subsequent `/context` and `/tools` commands render
the latest projection rather than a presentation cache. The candidate adds
no mutation, process, persistence, async, R8/R9, or R10 behavior.

Focused evidence includes 16 Rust `siralos-cli` tests covering uncomputed and
normal projection output, pressure, hidden/gated/denied rendering, stable
registration order, empty Tool surfaces, configuration composition, and a
real Tool-round refresh, plus the TypeScript interactive-session oracle test
suite (86 tests). The existing 11 required `context-projection`
differential scenarios remain the typed-value parity evidence; no new
differential subject is required for CLI-owned strings. The implementation
commit is `3f47dcd67f5ff70e286409ca6b60341047cdb7e2`, with focused Rust test
closure in `b867ca7332f7cac4b289e60d4067f6d9eef1a6d2` and focused oracle
coverage in `d07ae112cd38bed7fa7a089613f297520842e48c`.

### R7 Verified

R7 is **Verified** — R7A through R7.5 all complete and evidence-backed on
the same verified worktree (`61fbf997d781377b2501af4057920a2064dd8716`). The Wayfinder map
(`docs/wayfinder/siralos-roadmap.md`) is decision-ready R7.5 → R12 → Stage 4;
its 8 decisions (01-r7-5-review-rubric through 08-stage4-entry-sequence)
closed the R7.5 byte-equal vocabulary gap (advisory
`09-advisory-terminal-sanitizer` closed by
`crates/siralos-cli/src/sanitize.rs` port of
`apps/cli/src/output/sanitize.ts` — stateful `push`+`flush`, now 51
`siralos-cli` tests) and the 7-surface promotion checklist
(`docs/wayfinder/decisions/02-r7-verified-promotion.md` §2). Corpus v15
133 files, 128/128 applicable Windows (4 explicit skips, 1 informational
deviation); Tier-1 matrix is the audit mechanism (local
`check:differential` is `EPERM` on `stdio:'pipe'` inside DSH, rerun on CI).
Verified commit `61fbf997d781377b2501af4057920a2064dd8716` is the new `Last verified commit` and
`Latest verified executable worktree` in `PROJECT_CONTEXT.md`.

### R8 Verified

R8 — Optional Godot Stage-2 parity is **Verified** on worktree
`c075b3cf5e5240dd275a35cdc1a5a30c3bda9195`. All six frozen surfaces
(`docs/wayfinder/decisions/10-r8-entry-review.md` §2) are ported:
discovery/profiling (`siralos-core::godot` models + `siralos-adapters::godot`
discovery/profiler), fail-closed recovery contracts, version-bound API
knowledge, GDScript check-only diagnostics, bounded LSP (framing/JSON-RPC/URI/
port allocation), and read-only scene/resource intelligence. The differential
corpus advanced to **version 16, 155 scenario files** with all five frozen
subjects at required parity (`godot-discovery` 4, `godot-knowledge` 5,
`godot-diagnostics` 4, `godot-lsp` 4, `godot-scene-resolve` 5) — local audit
**150/150 applicable required scenarios** (4 platform skips). The fail-closed
posture is mechanically preserved: no Godot module contains a process-spawn
path. Gates observed PASS on the verified worktree: `cargo fmt`, `clippy -D
warnings`, `cargo test --workspace` (657 tests), `check:architecture`,
`check:rust`, `check:differential`. Promotion evidence and atomic surface list:
`docs/wayfinder/decisions/11-r8-verified-promotion.md`. R9 awaits its own
entry review.

### R9 Verified

R9 — Optional Godot Stage-3 parity (deterministic core) is **Verified** on
worktree `1623e800f8034d07825d7c6582768c27a91a973e`. All three frozen surfaces (`docs/wayfinder/decisions/12-r9-entry-review.md`)
are ported: review context & impact intelligence
(`siralos_core::godot::impact`), prepare-only scene/resource mutation
contracts (`siralos_core::godot::scene_mutation` plus the
`siralos-adapters::godot::scene_mutation` orchestration whose apply is
typed `Unavailable`), and the deterministic unified `/develop` core
(`siralos_core::godot::development`: surface routing, dependency-based
apply ordering with cycle rejection, blocked dispositions). The
differential corpus advanced to **version 17, 167 scenario files** with
all three frozen subjects at required parity (`godot-review-context` 4,
`godot-mutation-prepare` 4, `godot-develop-plan` 4) — local audit
**162/162 applicable required scenarios** (4 platform skips). Gates
observed PASS on the verified worktree: `cargo fmt`, `clippy -D warnings`,
`cargo test --workspace` (691 tests), `check:architecture`, `check:rust`,
`check:differential`. Promotion evidence and atomic surface list:
`docs/wayfinder/decisions/13-r9-verified-promotion.md`. R10 awaited its own
entry review.

### R10 Verified

R10 — H1 content identity, H2 determinism/replay, ICM context, and H3
runtime-readiness parity — is **Verified** as one milestone with three
ordered, entry-reviewed sub-slices on executable worktree
`a456afb71ab64c5504cd19e8eb7988d32d60a9dc`
(`docs/wayfinder/decisions/14-r10-entry-review.md`,
`15-r10b-entry-review.md`, `16-r10c-entry-review.md`). R10a ported H1
content identity and the H2 determinism family (`siralos_core::identity`
extended with typed digests/deltas/staleness plus
`siralos_core::determinism`; corpus **version 18**, 177/177 applicable
required parity). R10b ported the ICM context family
(`siralos_core::context`: phase contracts with narrowing-only authority,
digest-bound dependency manifests, targeted staleness, provenance and
why-diagnostics; corpus **version 19**, 190/190). R10c ported the H3
runtime-readiness family (`siralos_core::runtime`: causal run identity,
manifest-bound budgets with typed admission, the pure supervisor
lifecycle over the 13-kind failure taxonomy, harness-owned fault
injection under the controlled clock, and the fail-closed readiness
doctor; corpus **version 20, 210 scenario files**, 205/205 applicable
required parity, 4 platform skips). No real process is ever launched.
Gates observed PASS on the verified worktree: `npm run check` (exit 0,
211/211 TypeScript test files), `cargo fmt`, `clippy -D warnings`,
`cargo test --workspace`, `check:differential`. Promotion evidence and
atomic surface list: `docs/wayfinder/decisions/17-r10-verified-promotion.md`.
R11 requires its own entry review.

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
