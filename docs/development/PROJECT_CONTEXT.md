# Siralos project context

```text
Project: Siralos
Context schema: 1
Status: Active development
Public stages: 6
Migration track: Stage 3R
Current completed milestone: R6
Current active milestone: R7
Next milestone: R7 - Provider, Tool-Loop, Projection, Configuration, and CLI Parity
R7A behavior extraction/protocol remediation: complete (see docs/development/R7_BEHAVIOR_EXTRACTION.md); R7.1 is the next implementation step
Last verified commit: 99ee902c1c61927070f1249ee16aa276eff24b2b
Canonical repository: https://github.com/CrimsX/Siralos
```

The verified-commit field is an evidence pointer, not a wall-clock status. It
names the exact baseline whose worktree content passed the full `npm run check`
gate (verified parent/baseline model): documentation-only metadata
reconciliation commits after that baseline do not alter executable behavior,
and the latest successful required CI run on `main` remains the publication
authority after later documentation-only commits.

## 1. Product definition

Siralos is a minimal, declarative AI coding harness with an inspectable
execution environment.

Expanded product explanation (ADR 0036):

> Profiles define how the model works. Context shows what Siralos gives it.
> The Host controls what it can do.

Technical tagline:

> Probabilistic reasoning. Deterministic execution.

The security-first, context-efficient engineering posture of Stages 1-3
remains the implementation reality beneath this identity; the identity itself
is frozen by [ADR 0036](../adr/0036-lean-product-composition-and-extension-model.md)
and the lean constitution.

```text
probabilistic model reasoning
            |
            v
typed proposal
            |
            v
host validation
            |
            v
policy/capability decision
            |
            v
controlled transactional effect
            |
            v
verification/evidence
            |
            v
authoritative host state
```

Central invariant:

> The model may propose; it never directly owns authoritative Siralos mutation
> or grants itself capability.

## 2. Current implementation reality

### Current TypeScript reference

The npm workspace under `apps/` and `packages/` is the broad behavioral
reference for Stages 1-3. It includes the interactive CLI, deterministic fake
provider, bounded read-only workspace tools, task/context/evidence/planning
contracts, references/research, static Godot intelligence, and extensive
internal mutation/development workflow contracts. Effects that cannot satisfy
the security contract truthfully report `unavailable`; their presence in code
does not make them a shipped capability.

### Current Rust candidate

The Cargo workspace contains `siralos-core`, `siralos-adapters`, and
`siralos-cli`. R1 established the domain-neutral workspace and engineering
rules. R2 added the differential behavioral harness and its first parity
subjects. R3 added the domain-neutral host-owned task kernel to
`siralos-core` (contracts, revisions, materialized state, transitions,
evidence, acceptance, completion gate, activity, progress) with 17
differential scenarios held at byte parity against the TypeScript reference
(ADR 0033 gate). R4 added the generic workspace/project foundation:
`siralos-core` owns the validated workspace-relative path type (NUL,
absolute, drive, and traversal rejection; protected-path classification),
the workspace bounds, the deterministic revision-handle/registry semantics,
the typed prepared create/edit/delete effect models, the checkpoint model
with operation-state invariants, undo planning, and reconciliation
classification, and the read-only Git error/disposition contract;
`siralos-adapters` owns canonical root resolution, containment-safe path
resolution (symlink/junction escapes rejected), bounded complete exact
reads with EOF-verified whole-file SHA-256 identity, deterministic
bounded listing and search,
the fail-closed mutation-preparation boundary (prepare/apply report
unavailable; nothing is written, approved, or checkpointed), checkpoint
storage inspection and startup reconciliation over the reference metadata
layout, and the typed unavailable Git inspection boundary. The differential
corpus gained 23 scenarios across the `workspace-read`, `workspace-list`,
`workspace-search`, `workspace-revision`, `workspace-prepare`,
`checkpoint`, and `git-inspection` subjects; all required applicable
scenarios match (ADR 0033 gate, corpus version 7, 47 scenario files). R4
hardening corrected the bounded exact reads on both implementations so a
short read is never treated as EOF, a partial prefix can never become
authoritative source identity, file-size boundaries are explicit, and
checkpoint source-path inspection fails closed on any escape. Structural/summary
read modes remain explicit typed unsupported dispositions at R4
(GDScript structure extraction is Godot-domain language intelligence,
R8/R9). R5 added the generic language-intelligence foundation to
`siralos-core` (`siralos-core::language`): one-based source
positions/ranges with typed validation, the bounded sanitized diagnostic
model with deterministic dedup/ordering and explicit truncation, generic
symbol/reference/definition query models with deterministic ordering and
bounds, the language-neutral structural-document representation with the
deterministic advisory summary formatter (byte-bounded, revision-stating,
never authoritative), the typed validation result semantics
(source-invalid never conflated with infrastructure failure), the generic
language-service URI mapping in `siralos-adapters::language::uri`, and
R4 revision binding throughout. The TypeScript reference gained the
corresponding generic language modules (position/sanitize/truncate/
diagnostic/definition/structure) that the Godot adapters now consume,
preserving all existing behavior. The generic structural representation
is explicitly language-neutral: it owns only cross-language kinds
(type/function/method/field/variable/constant/enum/event/module/other),
opaque attributes/modifiers, and generic summary wording, with no
GDScript/Godot semantics (SignalInfo, `extends`/`class_name`,
annotations, `export` interpretation) anywhere in
`siralos-core::language`; the GDScript scanner and summary remain the
TypeScript reference for R8/R9. The differential corpus gained 16
scenarios across the `language-diagnostics`, `language-structure`, and
`language-definition` subjects (corpus version 9, 63 scenario files)
with language-neutral structure fixtures; all required applicable
scenarios match on both implementations. R5 ports no Godot/GDScript
parsing (the GDScript scanner remains the TypeScript reference for
R8/R9), no LSP transport, no process execution, no provider tool
surface, and no Domain architecture.
R6 added the minimal Domain capability architecture and synthetic
conformance Domain: `siralos-core::domain` owns the domain-neutral
lifecycle/capability semantics (validated package identity: stable id,
exact SHA-256 package digest, versioned ABI; the explicit state machine
absent/installed/enabled/active with typed transitions; declared
capability requests with the Host-authoritative grant decision; exact
activation binding; typed recovery-ready failure outcomes; and the
explicit absence of implicit acquisition — workspace contents are
opaque to the lifecycle). `siralos-adapters::domain` owns the production
Component Model / WIT boundary (ADR 0034): the versioned
`siralos:domain-abi@1.0.0` world
(`crates/siralos-adapters/wit/domain-abi.wit`), component
loading/instantiation with the versioned export-identity check
(unknown/incompatible ABI fails closed), exact-byte digest
verification at install and activation (stale/wrong bytes rejected
before any semantic work), fuel/memory/input/output/host-call bounds,
trap containment with typed fault outcomes and session stop, and the
host-mediated effect boundary (grant-checked bounded workspace reads;
process execution denied). The synthetic conformance Domain
(`tests/domain-conformance/`) is a deterministic, product-neutral
component fixture proving the production boundary, including
pathological behaviors (trap, unbounded loop) for containment and
bounded-execution evidence. The differential corpus gained 23 scenarios
across the `domain-lifecycle` and `domain-capability` subjects (corpus
version 11, 86 scenario files); all required applicable scenarios match
on both implementations, and the Rust Component conformance suite
passes against the checked-in component bytes. R6 remediation also
bound every prepared activation to the lifecycle generation validated
at preparation: every successful material lifecycle transition
(install, uninstall, enable, disable, activation commit, deactivate)
advances the generation, and a stale commit fails typed with
`STALE_ACTIVATION` before any mutation, session-id allocation, or
HostSession publication. Activation identity is exact in all three
dimensions: the request ABI must identify the installed package ABI
(a Host-compatible request can never substitute for a differently
declared package ABI) and must also satisfy Host compatibility, so
every successful activation satisfies
`ActivationBinding::matches(installed_package)` by construction.
Prepared activations never carry authority across Host policy
contexts: the final capability grant is recomputed at commit from the
commit-time Host authority, so a narrower final authority fails typed
with zero mutation and zero session consumption, and a wider final
authority can never widen the activation request. R6
implements no Plugin
system, no marketplace, no provider/tool integration, and no Godot
Domain.

### Target architecture

Rust is the successor implementation, but TypeScript remains the behavioral
migration oracle until R12 records an evidence-backed retirement or retention
disposition. Target Rust behavior is not described as shipped merely because an
ADR or TypeScript contract exists.

Useful language:

- **Optional Domain**: explicitly installed specialization that contributes
  intelligence without acquiring host authority. Godot is the first and only
  current Optional Domain.
- **Behavioral Reference**: implementation whose observable behavior constrains
  migration parity. TypeScript currently owns this role.
- **Rust Successor**: candidate implementation promoted only through migration
  gates; it is not a mechanical rewrite.
- **Controlled Runtime Execution**: future generic host-authorized, bounded
  runtime operation producing structured evidence.
- **Godot Runtime Adapter**: optional Godot specialization of the generic
  runtime boundary, not runtime authority itself.

## 3. Current roadmap position

The six public product stages are separate from the internal Stage 3R migration
track. Stages 1-3 are historical/current product milestones; stages 4-6 are
refined by the lean vision freeze (ADR 0036) and remain staged product
direction subject to evidence, not guaranteed commitments:

1. Harness Foundation - broad TypeScript reference established.
2. Godot Script-Development MVP - broad TypeScript reference established, with
   unsafe effects truthfully unavailable.
3. Godot-Native Development MVP - broad TypeScript reference established.
4. Controlled Execution (Runtime and Visual QA) - not begun.
5. Composition (Profiles, Context controls, Skills, Plugins, Tools, Views,
   optional Domains) - not begun.
6. Evolution & Stabilization (/evolve, evaluation, packaging, release) -
   not begun.

Current Stage 3R position:

```text
R1      COMPLETE
R2      COMPLETE
R3      COMPLETE
R4      COMPLETE
R5      COMPLETE
R6      COMPLETE
R7      NEXT
R8-R12  NOT DUE
```

Status changes require executable evidence and an update to
[ROADMAP.md](../../ROADMAP.md) and the
[migration register](RUST_MIGRATION.md). Stage 3R is not a seventh public stage.

## 4. Stage 3R migration track

| Milestone | Scope                                                                   | Status   |
| --------- | ----------------------------------------------------------------------- | -------- |
| R1        | Siralos rename + Rust engineering/domain-neutral foundation             | Verified |
| R2        | Differential Behavioral Harness                                         | Verified |
| R3        | Domain-Neutral Core                                                     | Verified |
| R4        | Generic Workspace / Project Foundation                                  | Verified |
| R5        | Generic Language Intelligence                                           | Verified |
| R6        | Minimal Domain Capability Architecture and Synthetic Conformance Domain | Verified |
| R7        | Providers / Tools / CLI                                                 |
| R8        | Godot Stage-2 parity                                                    |
| R9        | Godot Stage-3 parity                                                    |
| R10       | H1 / H2 / ICM / H3 parity                                               |
| R11       | Full differential/effect parity                                         |
| R12       | TypeScript reference retirement or explicit retention decision          |

Permanent migration rule:

```text
behavioral parity != structural parity
```

TypeScript constrains observable behavior, authority, outcomes, and effects;
it does not prescribe Rust modules, ownership, traits, or control flow. Each
R3-R11 port follows behavior extraction, idiomatic Rust design, differential
and effect parity, review, measurement, then acceptance.

## 5. Rust engineering direction

The authoritative standard is [RUST_STYLE.md](RUST_STYLE.md). In summary, Rust
work favors idiomatic design, type-driven invariants, clear ownership, narrow
visibility, typed errors, deterministic collections, filesystem-native paths,
and evidence-driven optimization. It avoids mechanical TypeScript translation,
crate-per-class decomposition, speculative traits, unnecessary `Arc<Mutex<_>>`,
and unnecessary async. Do not restate the full guide in milestone prompts.

## 6. Architecture

The conceptual ownership model (ADR 0036) is:

```text
USER CONFIGURATION
Profile · Context · Skills
            |
            v
SIRALOS HOST
State · Revision · Capability
Tools · Effects · Evidence
            |
            v
OPTIONAL PLUGINS
Tools · Views · Domains
```

Siralos is a small privileged Host: authoritative state, revision/staleness
semantics, capability enforcement, effect execution, plugin containment,
evidence/run identity, profile resolution invariants, and context
provenance/integrity rules are non-replaceable Host responsibilities.
Orchestration is not a foundational ownership layer; higher-level schedulers
may consume Siralos Runs without defining Run semantics. The previous
five-layer model (Foundation / Deterministic Runtime / Extensions /
Orchestration / Experience) is superseded by this lean model.

The Host stays small by moving sophistication upward into declarative
configuration, Skills, and explicitly installed Plugins whenever doing so does
not weaken correctness, authority, determinism, or inspectability (ADR 0036
core thesis). Canonical product primitives:

- **Profile** — a named declarative AI working configuration; it requests
  authority through resolution and Host policy, it never grants it.
- **Context** — the exact model-visible material Siralos compiles and sends
  through its provider boundary; inspectable, explainable, bounded, and
  controllable (Live / Pinned / Frozen).
- **Skill** — reusable declarative guidance for model reasoning; it has no
  authority (Skill != Capability).
- **Plugin** — the only optional executable extension package; explicitly
  installed, versioned, digestable, capability-scoped, and contained by the
  Host boundary; core works with zero Plugins.
- **Tool** — the one callable typed operation abstraction (stable identity,
  typed input/output, side-effect classification, capability requirements).
- **View** — a Plugin-contributed presentation surface; it renders public
  state and requests typed Tools through the Host and has no direct
  authority.
- **Domain** — specialized development intelligence contributed by a Plugin
  (Godot is the first and currently only planned Domain).
- **Run** — one bounded Siralos execution under an explicit Effective Profile
  and Host authority state; one Run uses one stable Profile identity.
- **Evolve** — a bounded evaluation workflow that proposes measured
  improvements, escalating Profile -> Context -> Skill -> Plugin -> Host.
- **Permission vs Capability** — Permission is the user-facing authorization
  terminology; Capability (CapabilityRequest/CapabilityGrant) is the
  host-enforced internal security terminology. Installing or selecting a
  Plugin never implies authority.

Dependencies point toward lower-level ownership. The current Rust bias is a
modular monolith:

```text
siralos-cli -> siralos-adapters -> siralos-core
```

Core never depends on infrastructure or a domain. Additional crates require a
real dependency, distribution, security, or ABI reason. Current TypeScript
dependency direction is `apps/cli -> packages/adapters -> packages/core`; the
TypeScript core still carries historical Godot contracts because it is the
behavioral reference, not the target Rust structure. See
[ARCHITECTURE.md](../../ARCHITECTURE.md), [ADR 0036](../adr/0036-lean-product-composition-and-extension-model.md),
and the [architecture index](../architecture/README.md).

## 7. Permanent security model

Authority precedence is:

```text
hard security
> managed
> user
> trusted project
> workflow/task
```

Lower layers may narrow authority; they may never broaden it.

```text
instructions != security
knowledge != security
skills != capability
domain installation != authority
hash != trust
approval != general capability
model completion != host acceptance
```

Missing required enforcement means fail closed. Approval, capability policy,
sandboxing, checkpoints, revision checks, and verification are independent
gates. Repository text, providers, references, research, and tool output are
untrusted data, never policy. [SECURITY.md](../../SECURITY.md) is the complete
security contract.

### Deterministic Host guarantees are non-configurable

The lean composition model does not weaken Siralos determinism. The following
Host guarantees cannot be configured away by Profiles, Skills, Plugins, or
project configuration:

```text
revision verification
digest identity
stale-write rejection
capability enforcement
deterministic authoritative ordering
explicit nondeterminism boundaries
transaction verification
evidence identity
replay semantics
```

Configuration may change what environment is requested. It may never disable
the Host mechanisms that make authoritative behavior verifiable.

### Bounded recovery

Recovery is a Host/run behavior and design property, never a foundational
product subsystem. Siralos may automatically recover from a failure only
through bounded actions already permitted by the current Host authority and
Run policy. Recovery uses existing authority and never creates authority: it
never broadens capabilities, never weakens validation, never bypasses
revision/staleness, approval, checkpoint, sandbox, or verification gates,
never alters an authoritative Goal silently, never converts uncertain state
into accepted state, never mutates Frozen state, and never retries
indefinitely. Every recovery attempt is observable and bounded, prefers
deterministic recovery and uses model-assisted recovery only when reasoning
is actually required (receiving no special authority), and is followed by
verification; if verification fails or the recovery budget is exhausted,
Siralos stops and reports the unresolved failure with retained evidence.
Recovery repairs or continues the current Run; long-term learned change
belongs to Evolve (Stage 6). Full recovery parity remains R11-owned; R6
inherits only typed-failure/recovery-readiness constraints.

## 8. Mutation model

The required mutation sequence is:

```text
inspect exact revision
-> prepare complete change
-> preview
-> approval
-> checkpoint
-> revalidate revision
-> apply
-> verify
-> validate
-> review
-> acceptance
```

Approval binds exact prepared content; changing content invalidates approval.
Git is not transactional authority. Current Node-based workspace mutation,
checkpoint creation, undo, private run-directory management, and process launch
remain unavailable where same-user pathname substitution cannot be defeated by
mechanical primitives. Another pathname recheck, hash window, warning, or
private filename is not a fix.

## 9. Context model

> No LLM role reads everything by default.

Preferred discovery is:

```text
search -> structural/summary -> evidence -> exact content only when necessary
```

`WorkspaceScope` identifies verified/candidate files and budgets.
`ActiveWorkingSet` explains the exact current-step files and inclusion reasons.
`PhaseContract` and typed intermediate artifacts constrain work by phase.
Context carries provenance and authority, remains bounded, and is reconstructable
from host-owned state. Conversation history is not authoritative state.

### Deltas are derived; full artifacts are authoritative

Full current artifacts are authoritative. Semantic deltas are derived
descriptions of material change, usable for targeted staleness propagation,
incremental Context compilation, validation/review selection, cache
invalidation, Tool-surface comparison, Profile comparison, and future
`/evolve` experiment comparison. Do not build a delta-only state
reconstruction or event-sourcing architecture.

### Targeted invalidation

The preferred model is:

```text
input identity changed
    -> derive semantic delta
    -> invalidate only known affected derived state
```

Conceptually: a Skill change may change affected Profile/Context identity; a
Plugin change may change the ToolSurface; a Context policy change invalidates
compiled Context; one changed source file invalidates only dependent derived
Context/index artifacts where known. Do not build a universal dependency graph
or generic reactive engine before evidence requires one.

### Cache rules

Caches are derived and reconstructable; caches are never authoritative. A
future derived-cache key should generally combine input content identity,
algorithm/schema version, and relevant configuration identity — for example
`SourceDigest + ParserVersion -> ParsedSource` or
`ContextItemDigest + TokenizerIdentity -> TokenCount`. Do not introduce a
generic CacheManager, distributed cache, or reactive cache framework now.

## 10. Provider model

Siralos is provider-neutral. Provider/model code cannot decide host tool
authority, approval, security policy, or authoritative task completion. Model
and provider identity plus relevant generation configuration are recorded where
reproducibility requires them. Real provider integrations are not implemented
in the current milestone.

### Profile / lock / Context identity

Future Profile semantics remain compatible with H1/H2:

```text
siralos.toml -> resolution -> siralos.lock -> ResolvedProfile
                                              -> ResolvedProfileDigest
```

A Run eventually records the exact ResolvedProfile identity. ContextSnapshot
identity remains separate from `siralos.lock`: a lockfile records resolvable
portable inputs; a ContextSnapshot records exact model-visible compiled
material. Never merge them.

### Profile stability during a Run

One Run uses one resolved/effective Profile identity. Provider, model, tool,
and context composition must not silently change mid-Run; a material
composition change requires an explicit transition, a new Run, or otherwise
visible identity change. This supports reproducibility, explainability, cache
stability, and Tool-surface stability.

### Tool-surface identity

If a future Plugin update changes tool presence, tool schema, side-effect
classification, or capability requirements, the model-visible ToolSurface
identity must be able to change explicitly. Plugin updates must never silently
mutate the model-visible callable environment inside an existing Run.

### External nondeterminism

External nondeterministic observations — LLM responses, network responses,
process/compiler output, future Plugin service responses — are recorded or
explicitly classified when strict replay requires them. Strict replay consumes
recorded observations rather than silently calling live external systems
again (H2).

## 11. Godot domain policy

Godot is currently the only Optional Domain.

```text
NOT INSTALLED by default
NOT ENABLED by default
NOT AUTO-INSTALLED
NOT AUTO-DOWNLOADED
NOT AUTO-RECOMMENDED
NO workspace-driven acquisition
```

A `project.godot` file never triggers package state transitions. The Godot
Domain package and Godot Engine installation are separate. Core must work with
the package absent. A disabled Godot domain contributes no tools, project-domain
discovery, Godot context, Godot validation, or Godot runtime. The TypeScript
reference's static Godot surfaces do not imply that the future Rust package is
installed or enabled.

## 12. Domain host boundary

[ADR 0034](../adr/0034-godot-domain-host-boundary.md) accepts the WebAssembly
Component Model with versioned WIT as the primary host/domain boundary. A
versioned out-of-process IPC boundary is retained as fallback/reference
evidence. R6 promotes the experiment to the minimum production boundary:
the versioned world `siralos:domain-abi@1.0.0` lives in
`crates/siralos-adapters/wit/domain-abi.wit` and is implemented by
`siralos-adapters::domain` on top of the domain-neutral lifecycle in
`siralos-core::domain`; the deterministic synthetic conformance Domain
(`tests/domain-conformance/`) proves the boundary on the real component
bytes. Domain code receives only structurally granted imports (the world's
`host-effects` interface plus the minimal wasm32-wasip2 std plumbing; no
filesystem, network, process, or ambient WASI surface); package identity
(computed from the exact accepted component bytes) and capability requests
remain host-controlled. Reopening the decision requires new evidence and a
superseding ADR. ADR 0036 records the long-term unification target — the
same capability-scoped Plugin mechanism preferred for ordinary executable
Plugins and Domain-contributing Plugins if practical — without changing
ADR 0034's measured decision.

## 13. R2 differential contract

Every later migration milestone uses R2 to protect observable behavior:

```text
fixture
   |
   +-> TypeScript reference --+
   |                          +-> normalized semantic comparison -> verdict
   +-> Rust candidate --------+
```

The corpus and each scenario are digest-bound. Both runners have symmetric
bounded lifecycle supervision. Outcomes are typed canonical records, compared
semantically rather than as prose. Required parity is distinct from explicit
platform skips and accepted informational differences. CI enforces the
applicable matrix and retains exact evidence. See
[ADR 0033](../adr/0033-differential-behavioral-harness.md).

## 14. H1 / H2 / ICM / H3

These accepted targets are ported at R10; existing TypeScript contracts do not
make Rust parity complete.

- **H1**: `hash = exact identity`, `delta = what changed`,
  `revision = lifecycle identity`. A hash does not imply trust, authority, or
  provenance.
- **H2**: same authoritative inputs produce the same host decision. Fresh LLM
  prose need not be byte-identical; nondeterministic observations are recorded
  or explicitly unreplayable.

### Identity model preservation

Keep the accepted distinctions: **digest** = exact content identity,
**delta** = what materially changed, **revision** = lifecycle identity. Never
collapse these concepts. Reuse one typed, domain-separated digest
architecture (the existing ArtifactDigest/domain-separated identity model)
for future exact identities; do not create independent hashing systems such as
`ProfileHash`, `ContextFingerprint`, `PluginChecksum`, or `SkillHash`.

Structured semantic artifacts may use canonical serialization before hashing.
Workspace source identity must continue to reflect the exact relevant bytes
where stale-write protection depends on exact source identity — do not
normalize away meaningful source changes before revision/staleness
verification.

- **ICM**: phase-specific, provenance-aware, reconstructable typed context and
  artifacts.
- **H3**: runtime-readiness identity, budgets, lifecycle, failure taxonomy,
  cancellation, cleanup, reconciliation, and fault injection.

## 15. Stage 4 direction

The future internal Stage 4 sequence is:

1. Controlled Runtime Execution
2. Runtime Evidence
3. Godot Runtime Adapter
4. Visual Evidence
5. Controlled Interaction
6. QA Workflows
7. Profiling

Stage 4.1 is generic runtime execution, never shorthand for "run Godot." It is
not due until the Stage 3R migration and entry gates pass.

## 16. Stage 5 / 6 guardrails (lean vision, ADR 0036)

Stage 5 (Composition) may later own Profiles, portable locking
(`siralos.toml` / `siralos.lock` semantics), Context controls (Live /
Pinned / Frozen, explain/diff), Skills and the Skill Creator, capability-scoped
Plugins, Tools, Views where justified, and optional Domains. Stage 6
(Evolution & Stabilization) may later own `/evolve` with measured evaluation,
evaluation corpora/baselines, Profile/Context optimization, Skill
creation/refinement, Plugin/Host improvement proposals, compatibility,
performance, packaging, and release stabilization.

Deliberately removed from committed core architecture by ADR 0036: general
Hooks, built-in optional agents/subagents, TaskGraph, generic workflow
engines, Agent Teams, Fleet, distributed/remote workers, plugin marketplaces,
plugin dependency graphs, automatic Skill/Plugin acquisition, model-router
architecture, a generic Memory subsystem, and GUI/TUI runtime ownership.
These are speculative/future-only and may be reconsidered only from concrete
demand/evidence. None of the above is due and none may be pulled into R3.

## 17. Harness-derived design lessons

Adopted principles, independent of any conversational source:

- typed control operations are shared across surfaces;
- lifecycle generation/identity fences stale work exactly;
- read-only observation never manufactures observed state;
- unknown/unavailable reasons are typed;
- operation receipts are bounded and redacted;
- durable sequenced runtime events arrive only in their owning stage;
- Run identity and provider/model identity are recorded where
  reproducibility requires them; worker identity is not a core concept
  (ADR 0036) and appears only if an external orchestration consumer
  demonstrates a concrete need.

The runtime-event and identity items are **PLANNED / NOT DUE**, not current
defects; worker identity specifically is **SPECULATIVE / NOT CORE**.

## 18. Anti-patterns

The permanent `AP-001` through `AP-016` register lives in the
[normative requirements document](../requirements/REQUIREMENTS.md). Important
themes are: the model never owns authoritative state; chat is not state;
unrestricted shell is not the authority primitive; permissions are not inferred
from prose; domains do not auto-activate; Git is not rollback authority; retries
are bounded; caches remain derived; and Rust is not a mechanical copy of the
TypeScript structure.

## 19. Verification model

Verification grows with the owning milestone and includes unit, schema,
invariant, differential, effect, security, property, fuzz, transaction,
crash/recovery, determinism, replay, domain conformance, cross-platform,
adversarial, evaluation, and performance suites. The current authoritative
local gate is `npm run check`; R2 parity alone is `npm run
check:differential`. Live sandbox/Godot probes are opt-in and a skip is never a
pass. See [GOLDEN_TRACES.md](GOLDEN_TRACES.md) for scenario status.

### Budget categories

Keep distinct:

- **Deterministic/countable budgets** — context tokens, tool calls, retry
  count, output bytes, artifact count, and future Plugin work/fuel where
  supported.
- **Operational budgets** — wall-clock timeout, deadline, memory ceiling,
  process lifetime.

Wall-clock timing itself is not a deterministic computation boundary.

### Future Plugin resource bounds

Plugins must never implicitly receive unbounded resources. The eventual
Plugin Host should be able to bound memory, execution/work, host-call count,
output size, open resources, and wall-clock lifetime. No Plugin runtime is
designed or implemented in this goal.

### Observability source of truth

Avoid separate competing truths for CLI logs, GUI history, model transcripts,
analytics, and debug output. Authoritative structured Host state/evidence is
recorded once; human, model, CLI, GUI, evaluation, and debugging surfaces
project from that state where practical. No telemetry platform is built now.

### Durable versioning and stable boundaries

Keep separate: product version, schema version, and protocol/ABI version.
Unknown durable schema or ABI versions must fail explicitly; do not guess
future versions. Compatibility discipline applies to durable/external
boundaries — future `siralos.toml`, `siralos.lock`, Run artifacts,
ContextSnapshot, Skill/Plugin manifests, WIT interfaces, and machine-readable
external protocols. Internal Rust module names, structs, functions, and
traits are not stable public contracts merely because they exist during R3;
internal Rust APIs remain free to evolve until a real external consumer
exists. Do not prematurely version internal structs or create speculative
public traits.

## 20. Authoritative documentation index

| Document                                           | Ownership                                 |
| -------------------------------------------------- | ----------------------------------------- |
| [README.md](../../README.md)                       | Public landing page                       |
| [AGENTS.md](../../AGENTS.md)                       | Concise repository execution guidance     |
| [ARCHITECTURE.md](../../ARCHITECTURE.md)           | Current dependency and ownership truth    |
| [ROADMAP.md](../../ROADMAP.md)                     | Public stages and current milestone       |
| [SECURITY.md](../../SECURITY.md)                   | Security contract                         |
| `docs/development/PROJECT_CONTEXT.md`              | Complete development bootstrap context    |
| [RUST_MIGRATION.md](RUST_MIGRATION.md)             | Stage 3R sequencing                       |
| [RUST_STYLE.md](RUST_STYLE.md)                     | Rust engineering standard                 |
| [REQUIREMENTS.md](../requirements/REQUIREMENTS.md) | Normative CORE/HAR/AP requirements        |
| [RFC_INDEX.md](../architecture/RFC_INDEX.md)       | RFC/decision work-item ownership          |
| [GOLDEN_TRACES.md](GOLDEN_TRACES.md)               | End-to-end verification scenario registry |
| [docs/adr/](../adr/)                               | Accepted decision history                 |
| [docs/architecture/](../architecture/)             | Detailed architecture index               |

Accepted ADR frontmatter owns status and scope. `docs/archive/` is explicitly
historical/non-authoritative and excluded from normal context discovery.
[ADR 0036](../adr/0036-lean-product-composition-and-extension-model.md) is the
authoritative lean product, composition, and extension model for future
milestones; earlier ADRs remain the historical decision records they narrowed
without superseding.

## 21. Prompt / goal generation rules

Future implementation goals normally contain:

```text
ROLE
MILESTONE
GOAL
BEFORE IMPLEMENTING
milestone-specific invariants
implementation requirements
tests
effect/final-boundary tests
architecture boundaries
docs/ADR requirements
validation
acceptance criteria
required final response
next milestone - DO NOT IMPLEMENT
```

Do not paste the entire architecture or private conversational history into a
goal. Executors discover permanent rules from this repository and include only
the requested milestone's specific scope and acceptance boundaries.

## 22. New-session bootstrap

> For a new development session, first inspect `AGENTS.md` and
> `docs/development/PROJECT_CONTEXT.md`, then follow the linked authoritative
> documents relevant to the requested milestone. Do not require a separate
> conversational handoff. Verify current repository status before claiming
> completion.
