---
id: ADR-0036
status: accepted
domains: [architecture, product, configuration, context, extensions, evolution]
paths:
  - README.md
  - ROADMAP.md
  - ARCHITECTURE.md
  - docs/development/PROJECT_CONTEXT.md
  - docs/development/RUST_MIGRATION.md
supersedes: []
---

# ADR 0036 — Lean Product, Composition, and Extension Model

- Status: accepted (pre-R3 architectural vision freeze)
- Date: current milestone
- Related: ADR 0032 (Rust migration), ADR 0033 (differential harness),
  ADR 0034 (Godot domain host boundary), ADR 0035 (controlled runtime
  boundary)

## Context

Stage 3R R3 is the first subsystem port under the differential gate.
Before that port begins, the product model must be frozen so the Rust
migration starts from the final intended product shape rather than
mechanically reproducing exploratory TypeScript-era architecture.

This ADR is a **semantic freeze, not an implementation milestone**. It
changes no shipped behavior, ports no subsystem, and adds no code. It
records the lean, durable product vision that constrains future
architecture and migration decisions.

## Decision

Freeze Siralos as a minimal, declarative AI coding harness with an
inspectable execution environment, composed as:

```text
small privileged Host
+ declarative Profile
+ inspectable Context
+ declarative Skill
+ capability-scoped Plugin
+ bounded Run
+ measured Evolve workflow
```

Complexity grows primarily through user configuration, Skills, and
explicitly installed Plugins — not by expanding Siralos Core.

The final repository must answer clearly: what belongs in core, what
belongs in configuration, what belongs in a Skill, what belongs in a
Plugin, what belongs outside Siralos, and what is deliberately not being
built.

### Scope discipline

This ADR does **not**:

- redesign implemented Stage 1–3 behavior;
- break R2 differential parity;
- implement speculative APIs;
- create placeholder Rust types for future architecture;
- introduce new crates;
- create a plugin framework or skill system;
- create `siralos.toml` as an implemented artifact.

It does not supersede earlier ADRs wholesale. Where it narrows an earlier
future-facing decision, the narrowing is stated explicitly and the earlier
ADR remains the historical decision record.

## 1. Product identity

Siralos is a minimal, declarative AI coding harness with an inspectable
execution environment.

Expanded product explanation:

> Profiles define how the model works. Context shows what Siralos gives
> it. The Host controls what it can do.

Technical tagline (retained):

> Probabilistic reasoning. Deterministic execution.

User-oriented shorthand (optional, never a substitute for the technical
definition):

> Configure it. Inspect it. Control it. Evolve it.

## 2. Core product thesis

Siralos stays small by moving sophistication upward into declarative
configuration, Skills, and explicitly installed Plugins whenever doing so
does not weaken correctness, authority, determinism, or inspectability.

Before adding a foundational abstraction, ask:

```text
Does the Host need this to guarantee:
  state · revision · authority · effects · evidence · plugin containment?

YES  -> potentially core
NO   -> Can it be expressed as Profile or Context configuration?
         YES -> configuration
         NO  -> Is it reusable model-facing guidance?
                YES -> Skill
                NO  -> Does it require optional executable functionality?
                       YES -> Plugin
                       NO  -> Do not build it yet.
```

## 3. Architectural budget

Adopt an explicit simplicity bias. Initial target:

- 1 CLI
- 1 optional project manifest (`siralos.toml`)
- 1 lockfile (`siralos.lock`)
- 1 local state directory (`.siralos/`)
- 3 Rust crates initially (`siralos-cli`, `siralos-adapters`, `siralos-core`)
- 1 primary model/tool loop
- 2 extension concepts: Skill, Plugin
- 0 generic workflow engines
- 0 built-in multi-agent frameworks
- 0 plugin marketplaces
- 0 executable configuration languages
- 0 hidden auto-acquisition

These are design budgets, not hard numeric invariants. Exceeding one
requires evidence, not convenience.

## 4. Small privileged Host

The Siralos Host is the small, non-replaceable kernel. It owns only
foundational guarantees:

- authoritative state
- revision/staleness semantics
- capability enforcement
- effect execution
- plugin containment
- evidence/run identity
- profile resolution invariants
- context provenance/integrity rules

The Host is deliberately **not a plugin**. Users cannot replace these
guarantees through configuration. Siralos does not adopt an "everything
is a plugin" architecture.

## 5. Canonical user-facing primitives

The primary product concepts are:

```text
Profile   Context   Skill   Plugin   Run
```

**Evolve** is a workflow operating over them, not another foundational
runtime primitive. Avoid adding additional overlapping nouns unless
concrete functionality cannot fit these meanings.

## 6. Profile

A **Profile** is a named declarative AI working configuration.

A Profile may eventually select/configure: Provider, Model, Context
policy, Skills, Plugins, requested Permissions, and budgets.

A Profile does not itself grant authority:

```text
Profile request
    -> resolution
    -> Host policy
    -> Effective Run Configuration
```

Lower-authority configuration may narrow behavior. It may never broaden
Host authority.

## 7. Zero-configuration UX

Zero project configuration must remain valid. A user should eventually
be able to run `siralos` using an appropriate user/default
configuration. A repository adopts project-local configuration only when
it wants a shareable, reproducible Siralos setup. Configuration is never
mandatory merely because Profiles exist.

## 8. Portable profile configuration

Freeze the intended portable configuration shape:

| Artifact       | Meaning                                        |
| -------------- | ---------------------------------------------- |
| `siralos.toml` | Human-authored portable declaration            |
| `siralos.lock` | Machine-generated resolved portable identities |
| `.siralos/`    | Local runtime state                            |

`.siralos/` normally remains local/ignored unless a particular artifact
is deliberately exported.

## 9. Profile portability

Portability is a first-class design requirement:

> A committed Siralos Profile should recreate its portable AI execution
> configuration on another supported machine, subject only to explicitly
> reported local requirements.

Portable configuration may include: provider type/reference, model
request, context policy, Skill references, Plugin references, requested
Permissions, budgets, and resolved package/digest identities.

Local requirements (credentials, machine-specific executable locations,
local provider endpoints, local caches, runtime artifacts) are reported
explicitly — never hidden.

## 10. No secrets in portable config

Never store secrets directly in `siralos.toml` or `siralos.lock`.
Use references, conceptually:

```toml
credential = "env:OPENAI_API_KEY"
```

or an equivalent future credential-reference mechanism. The lockfile
must never contain credential values.

## 11. Lockfile semantics

`siralos.lock` has one job: record the exact externally resolvable
identities Siralos can truthfully establish for the selected
configuration. Potential locked identities:

- provider adapter version/digest
- model identifier/resolution level
- Skill version/digest
- Plugin version/digest
- Domain package digest where applicable
- Profile/schema version
- Context policy identity

The lockfile never stores: chat history, workspace contents,
ContextSnapshots, credentials, run outputs, or temporary state.

## 12. Explicit lock mutation

Normal execution must not silently modify `siralos.lock`. If
`siralos.toml` and `siralos.lock` disagree, execution should
eventually report an explicit stale/out-of-date lock condition.

Conceptual explicit operations (not implemented now; only the semantics
are frozen):

```text
siralos profile lock
siralos profile update <input>
```

## 13. Truthful model reproducibility

Do not claim that a hosted model is cryptographically immutable unless
the provider exposes such an identity. Future model resolution should
distinguish the quality of the resolved identity, conceptually:

```text
Exact > ProviderSnapshot > ProviderDeclared > MutableAlias > Unknown
```

Exact naming may change. The permanent rule:

> Siralos records the strongest model identity actually available and
> never claims stronger reproducibility than the provider permits.

## 14. Profile stability during a Run

A Run uses one resolved/effective Profile identity. Provider, model,
tool, and context composition must not silently change mid-Run. Changes
requiring a different resolved Profile produce an explicit transition, a
new Run, or otherwise visible state change. This supports
reproducibility, explainability, cache stability, and tool-surface
stability — without making cache optimization more important than
correctness.

## 15. Context

**Context** is the exact model-visible material Siralos assembles and
sends through its provider boundary.

Siralos may guarantee visibility into `HostCompiledContext` and
`ProviderRequest`. It must not claim visibility into undisclosed
server-side provider behavior.

## 16. Context as first-class state

Context must eventually be: inspectable, explainable, bounded,
provenance-bearing, revision-aware, diffable, pinnable, freezeable, and
reconstructable. The user-facing intent stays simple.

Initial conceptual commands (not implemented in this goal):

```text
siralos context
siralos context show --items
siralos context explain <item>
siralos context pin <item>
siralos context unpin <item>
siralos context freeze
siralos context thaw
siralos context diff
```

## 17. Context modes

Freeze only three initial context concepts:

- **Live** — normal mode. Siralos recompiles context from current
  authoritative state according to policy.
- **Pinned** — specific context items/revisions are retained/preferred
  while the rest remains live. Pinning does not change authority.
- **Frozen** — the exact compiled Siralos-visible context becomes an
  immutable, content-addressed snapshot. No silent substitution of
  updated workspace content is allowed; staleness is reported explicitly.

Do not introduce multiple freeze-depth taxonomies now.

## 18. Context snapshot

A future frozen context may include conceptually: ContextSnapshotId,
digest, ResolvedProfile identity, model/tokenizer identity where known,
Context policy identity, ordered ContextItems, exact item
revisions/digests, rendered representation, tool-schema identities, token
accounting, and provenance.

Ordinary frozen ContextSnapshots are **not** part of `siralos.lock`;
they are Run/debug/evaluation artifacts.

## 19. Context explanation

Every important model-visible context item should eventually be
explainable without asking another model. `context explain` should
answer conceptually:

- why was it included?
- what is its source?
- what authority/provenance class does it have?
- what revision/content identity is it?
- how much context does it consume?
- is it pinned?
- is it stale?

This is a key Siralos product property.

## 20. Cache stability

Add a measured Context Compiler concern: when authoritative inputs and
required semantics are unchanged, Siralos should prefer stable
context/tool prefixes where doing so improves provider cache behavior
without weakening correctness, transparency, or tool semantics.

Cache stability is an optimization. It must never override correctness.
Do not create a dedicated cache architecture solely to satisfy this
principle.

## 21. Memory

Do not create a first-class ambient Memory subsystem. If persistent
learned/user information becomes useful later, represent it as a
provenance-bearing Context source obeying normal Context rules (show,
explain, budget, pin, exclude, freeze, staleness). No hidden automatic
context injection.

## 22. Skill

A **Skill** is reusable declarative guidance for model reasoning.
Initial conceptual contents may include instructions, references,
examples, and context hints.

A Skill has **no authority**. It cannot grant filesystem access, network
access, process execution, credentials, or host mutation.

> Permanent rule: Skill != Capability

## 23. Skill Creator

Keep a future Skill Creator in the vision. Possible future UX:

```text
siralos skill create
siralos skill inspect <skill>
siralos skill test <skill>
siralos skill refine <skill>
```

It may derive candidate Skills from user intent, successful Runs,
repeated workflows, and /evolve findings. Candidate Skills require
review/evaluation according to their owning stage. Do not implement a
Skill registry, marketplace, dependency resolver, or package server now.

## 24. Plugin

A **Plugin** is the only optional executable extension package type in
Siralos. Plugins are: explicitly installed, optional, versioned,
digestable, capability-scoped, and sandboxed/contained by the Host
boundary.

Core must work with zero Plugins installed.

## 25. Plugin contributions

Keep the contribution model deliberately small. A Plugin may eventually
contribute:

```text
Tools
Views
Domain
```

No other public contribution categories are frozen now.

## 26. Tool

**Tool** is the one callable operation abstraction. Do not create a
parallel Plugin Action concept.

A Tool has: stable identity, typed input, typed output, side-effect
classification, and capability requirements.

The same Tool may eventually be invoked by the model, the CLI, a View,
or another explicitly authorized Host surface. Host semantics remain
shared.

## 27. Remove Action as a product abstraction

Do not introduce `PluginAction`, `UIAction`, or `AgentAction` for
ordinary callable operations. Use **Tool**. "Action" may remain an
ordinary English word where appropriate, but never a competing
architecture primitive.

## 28. View

A **View** is a Plugin-contributed visual surface. Views are
future-facing and not implemented now.

A View renders/queries public Host or plugin state, requests typed
Tools, has no direct authority, and does not own runtime semantics. The
Host/UI shell controls placement and layout. Do not freeze a frontend
technology, component DSL, or webview implementation now.

## 29. Plugin UI authority

A View pressing "Move card to Done" must conceptually request
`kanban.move_card(...)` **through the Host**. The UI must not mutate
authoritative external or Siralos state directly. Future GUI/TUI/web
surfaces remain presentation over the same Host semantics.

## 30. Domain

Retain **Domain** as a semantic specialization, not a separate
executable package ecosystem.

> A Domain is specialized development intelligence contributed by a
> Plugin.

Examples:

- Kanban Plugin: Tools, View, no Domain.
- Godot Plugin: Tools, future Views, Godot Domain.

User-facing documentation may say "Install the Godot Plugin to add Godot
domain support." Users are never required to understand a separate Domain
package mechanism.

## 31. Godot

Preserve all current Godot invariants. Godot remains the first and
currently only planned Domain. The Godot Plugin/Domain is: not installed
by default, not enabled by default, not auto-installed, not
auto-downloaded, not auto-recommended, and never acquired because
`project.godot` exists. The Godot Engine remains a separate external
dependency.

## 32. Plugin host boundary

Preserve ADR 0034's accepted direction — the WebAssembly Component Model
with versioned WIT — as the primary executable Domain boundary unless
superseded by evidence.

ADR-0036 generalizes the long-term package concept: the same
capability-scoped Plugin mechanism should be preferred for ordinary
executable Plugins and Domain-contributing Plugins if practical. This is
the intended **unification target**, recorded without modifying ADR
0034's measured decision and without implementing generic Plugins now.
Future Plugins are not forced through Wasm before the Plugin milestone
validates that generalization.

## 33. Plugin permissions

User-facing terminology: **Permission**. Internal security terminology:
**Capability**, **CapabilityRequest**, **CapabilityGrant**.

```text
Plugin requested Permission
    -> CapabilityRequest
    -> Host policy
    -> CapabilityGrant
```

Installing or selecting a Plugin never implies authority.

## 34. Plugin secret handling

Prefer future Host-mediated credential use. A Plugin should not receive
raw secrets when the Host can safely perform the credentialed operation
on its behalf. No Plugin credentials belong in the Profile, the lockfile,
Context, or model-visible data.

## 35. Plugin acquisition

Freeze: **explicit install only**. No automatic plugin download,
workspace-triggered installation, automatic recommendations, or implicit
profile-driven acquisition. A Profile may select a Plugin that is
already available; if it is absent, report the missing requirement.

## 36. No marketplace

Do not commit to a Plugin marketplace, ratings, reviews, payments,
recommendation systems, or automatic updating. A registry may be
evaluated later only if real usage requires one. Initial future
distribution remains simple and explicit.

## 37. No plugin dependency graph initially

Do not design Plugin-to-Plugin dependency resolution. A Plugin may depend
on the Siralos Plugin ABI/API version; its ordinary library dependencies
remain encapsulated inside its package/build. Revisit only if real
Plugins demonstrate need.

## 38. No general hook system

Remove generic Hooks from the committed architecture. Do not expose
`before_model`, `after_model`, `before_tool`, `after_tool`,
`before_context`, `after_context`, `on_commit`, or
`on_every_event` as a general public extension mechanism. Hooks create
hidden control flow and weaken explainability.

Route needs through:

```text
model guidance         -> Skill
callable behavior      -> Tool
optional executable    -> Plugin
specialized semantics  -> Domain
UI                     -> View
```

## 39. No built-in multi-agent foundation

Remove from committed core architecture: subagents, agent teams, Fleet,
TaskGraph, generic worker hierarchies, and distributed worker
frameworks.

One Run initially means one selected Profile and one primary
model/tool interaction loop. Future orchestration may be added
externally or as a higher-level consumer if concrete evidence requires
it. Experimentation is not prohibited; designing core around it is.

## 40. Orchestration

Remove Orchestration as a foundational Siralos ownership layer. The
product architecture is:

```text
User Configuration
    -> Siralos Host
    -> Optional Plugins
```

Higher-level schedulers may consume Siralos Runs. They do not define Run
semantics.

## 41. Planning

Do not require a permanent dedicated planning subsystem in the Rust
architecture merely because the TypeScript behavioral reference has
planning contracts. During migration: preserve required observable
planning behavior, but evaluate whether its implementation belongs as
Profile behavior, a Skill, an ordinary model artifact, or a small Host
contract — rather than mechanically porting PlanningDepth,
PlanningPolicy, or a planner/executor framework.

Behavioral parity does not require structural parity. This ADR does not
change R3 scope to implement planning unless porting evidence requires
it.

## 42. Run

A **Run** is one bounded Siralos execution under an explicit Effective
Profile and Host authority state. A Run should eventually make it
possible to answer:

- which Profile was used?
- which provider/model identity was used?
- what Context did the model see?
- which Tools were visible?
- which Permissions/Capabilities were effective?
- what observations occurred?
- what effects occurred?
- what evidence resulted?

Do not create a generic workflow engine around Run.

## 43. Model-visible reconstructability

Freeze this invariant: important model-visible Siralos context must be
reconstructable from Host-owned Run/context state and recorded
identities. A Run should be able to explain what Siralos showed the
model and where it came from. This does not equate to reproducing
undisclosed provider internals.

## 44. Evolve

Keep `siralos evolve` as a committed future product feature.

**Evolve** is a bounded evaluation workflow that proposes measured
improvements to Siralos and its configuration. It is not: continuous
autonomous self-modification, background rewriting, automatic production
deployment, or an independent agent runtime.

## 45. Evolve escalation order

Freeze the default improvement hierarchy:

```text
Profile -> Context -> Skill -> Plugin -> Host
```

When a lower-cost configurable layer can solve a measured problem
adequately, prefer it over adding Host code. This is a core mechanism
for resisting architectural bloat.

## 46. Evolve evidence

Permanent rule: **no evaluation -> no demonstrated improvement**.

Evolve operates conceptually as:

```text
baseline -> candidate -> evaluation -> comparison -> reject or propose
```

Host/self-modifying code changes must never be silently promoted.

## 47. Evolve complexity bias

If two candidates perform materially equivalently, prefer the one with:
less Host code, fewer dependencies, fewer abstractions, fewer
permissions, less model-visible Context, and less persistent state.
Evolution optimizes fitness, not feature count. It may recommend
deletion.

## 48. Skill creation from Evolve

Evolve may eventually detect repeated successful behavior and propose
"create Skill", "refine Skill", or "delete redundant Skill" before
proposing Host implementation. Not implemented now.

## 49. External orchestration example

Integrations such as Kanban/project management need not become core
workflow machinery:

```text
Kanban Plugin Tools
+ external/high-level scheduler
+ ordinary Siralos Runs
```

may coordinate work. Siralos Core does not need Kanban semantics.

## 50. Public stage model

Retain six public product stages. Stages 1–3 are historical/current
milestones and are not rewritten for branding. Keep Stage 3R as the
internal Rust migration. Refine future stages to the lean vision:

- **Stage 4 — Controlled Execution**: generic host-authorized runtime,
  runtime evidence, Godot runtime specialization, visual/runtime QA
  where domain-owned.
- **Stage 5 — Composition**: Profiles, portable locking, Context
  controls, Skills, Skill Creator, Plugins, Tools, Views where
  justified, optional Domains.
- **Stage 6 — Evolution & Stabilization**: evaluation corpus/baselines,
  /evolve, Profile/Context optimization, Skill creation/refinement,
  Plugin/Host improvement proposals, compatibility, performance,
  packaging, release stabilization.

Listed Stage 5/6 capabilities are staged product direction subject to
evidence — not guaranteed implementation commitments.

## 51. Removed from committed roadmap

Remove or explicitly demote to speculative/future-only: general Hooks,
built-in optional agents/subagents, TaskGraph, generic workflow engines,
Agent Teams, Fleet, distributed workers, remote workers, plugin
marketplaces, plugin dependency graphs, automatic Skill acquisition,
automatic Plugin acquisition, model-router architecture, generic Memory
subsystems, and GUI/TUI runtime ownership. None are upcoming guaranteed
work; each may be reconsidered only from concrete demand/evidence.

## 52. Architecture simplification

The repository's conceptual ownership model is:

```text
+---------------------------------+
|      USER CONFIGURATION         |
| Profile · Context · Skills      |
+----------------+----------------+
                 |
                 v
+---------------------------------+
|         SIRALOS HOST            |
| State · Revision · Capability   |
| Tools · Effects · Evidence      |
+----------------+----------------+
                 |
                 v
+---------------------------------+
|       OPTIONAL PLUGINS          |
| Tools · Views · Domains         |
+---------------------------------+
```

Dependency details remain governed by ARCHITECTURE.md. Do not create
code-layer dependencies purely to mirror this diagram.

## 53. Terminology freeze

| Term       | Meaning                                                                      |
| ---------- | ---------------------------------------------------------------------------- |
| Profile    | Named declarative AI working configuration; requests, never grants           |
| Context    | Exact model-visible material compiled and sent through the provider boundary |
| Skill      | Reusable declarative model guidance; no authority                            |
| Plugin     | The only optional executable extension package, capability-scoped            |
| Tool       | The one callable typed operation abstraction                                 |
| View       | Plugin-contributed presentation surface; no direct authority                 |
| Domain     | Specialized development intelligence contributed by a Plugin                 |
| Permission | User-facing authorization terminology                                        |
| Capability | Host-enforced internal security terminology                                  |
| Provider   | Infrastructure/model service integration selected by a Profile               |
| Model      | The hosted reasoning model behind the provider boundary                      |
| Task       | A unit of host-owned structured work (existing Stage 3 concept)              |
| Run        | One bounded execution under an Effective Profile and Host authority          |
| Evidence   | Host-observed records backing claims and acceptance                          |
| Revision   | Lifecycle identity of authoritative state/artifacts                          |
| Digest     | Exact content identity of an artifact                                        |
| Evolve     | Bounded evaluation workflow proposing measured improvements                  |

Avoid these as distinct subsystem types (descriptive use is fine):
Action, Extension, Hook, Module, Integration, Connector, Agent Package,
Workflow Package.

## 54. Action vs Tool

**Tool** = callable typed operation. Do not establish **Action** as a
competing callable abstraction.

## 55. Integration

Integration remains ordinary descriptive language. Example: "The Linear
Plugin integrates Siralos with Linear." No Integration package/runtime
category exists.

## 56. Providers

Providers remain infrastructure/model service integrations. They are not
another general Plugin class unless future implementation evidence makes
unification clearly beneficial. A Profile selects a Provider. Provider
implementation never owns Host authority.

## 57. Configuration language

Do not build a Nix-like executable language. Project configuration
remains: declarative, typed, schema-validated, inspectable, diffable,
portable. TOML is the intended initial representation unless an
implementation ADR later demonstrates a concrete reason to change it.
No arbitrary functions, conditionals, recursive imports, general code
evaluation, or deep module algebra.

## 58. Profile composition

Keep initial composition deliberately small. Potentially support: named
Profile, optional simple base/extends, explicit CLI override. Do not
design multiple inheritance, arbitrary merges, recursive expressions, or
dynamic environment-dependent configuration until actual user need
exists.

## 59. Permanent lean constitution

The permanent constitution (authoritative in the canonical context and
architecture documents):

> The Host stays small.
> Profile is the composition unit.
> Context is inspectable and controllable.
> Skill is declarative and has no authority.
> Plugin is the only executable extension package.
> Tool is the one callable operation abstraction.
> Users see Permissions; the Host enforces Capabilities.
> Domain is semantic specialization contributed by a Plugin.
> Run records what environment actually executed.
> Evolve requires evaluation and prefers configuration over Host
> complexity.
> No hidden acquisition or hidden context mutation.
> Complexity must earn its place in the Host.

## Narrowing of earlier future-facing decisions

The following earlier statements are narrowed without superseding their
ADRs:

- **ADR 0014** — the task runtime's future "multi-agent orchestration"
  framing: Siralos Core commits to no built-in multi-agent foundation;
  one Run means one Profile and one primary model/tool loop. The task
  runtime remains the host-owned structured work foundation.
- **ADR 0016 / ADR 0017 / ADR 0020** — "future multi-agent
  compatibility" groundwork notes remain historical; multi-agent
  machinery is not core architecture.
- **ADR 0020** — planning behavior stays host-owned; Rust migration must
  not mechanically port the planning framework (PlanningDepth /
  PlanningPolicy / planner-executor) when the required observable
  behavior can be represented by fewer idiomatic Rust types.
- **ROADMAP Stage 5** — "Extensibility and optional agents" is rescaled
  to lean Composition (Profiles, Context controls, Skills, Skill
  Creator, Plugins, Tools, Views where justified, optional Domains);
  multi-agent functionality is not committed core work.
- **ADR 0031 / ADR 0035** — runtime readiness and the controlled runtime
  boundary remain authoritative for Stage 4; ADR-0036 adds no runtime
  behavior and keeps GUI/TUI surfaces as presentation over Host
  semantics.
- **ADR 0034** — remains authoritative for the current Domain host
  boundary; ADR-0036 records the Plugin/Domain package unification
  target without changing that measured decision.

## Consequences

- The Rust migration (R3 onward) ports behavior, not TypeScript-era
  structure: no mechanical porting of planning frameworks, orchestration
  layers, context-projector proliferation, workflow abstractions, or
  agent hierarchies.
- Future milestones implement Profiles, Skills, Plugins, Context
  controls, Views, and /evolve from this frozen model; nothing in this
  ADR is an implementation commitment before its owning stage.
- No new crates, packages, or product code are introduced by this ADR.
- Documentation (README, ROADMAP, ARCHITECTURE, PROJECT_CONTEXT,
  RUST_MIGRATION, REQUIREMENTS, RFC_INDEX, AGENTS) is reconciled to this
  vision; contradictory future-facing claims are removed or marked
  historical/speculative.
