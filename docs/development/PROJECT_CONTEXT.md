# Siralos project context

```text
Project: Siralos
Context schema: 1
Status: Active development
Public stages: 6
Migration track: Stage 3R
Current completed milestone: R2
Next milestone: R3 - Domain-Neutral Core
Last verified commit: 22041693932f8e7e785e714b8d34258d6cd25959
Canonical repository: https://github.com/CrimsX/Siralos
```

The verified-commit field is an evidence pointer, not a wall-clock status. It
names the exact rewritten baseline that passed `npm run check` before this
context consolidation. The latest successful required CI run on `main` remains
the publication authority after later documentation-only commits.

## 1. Product definition

Siralos is a deterministic, security-first, context-efficient
software-development and QA harness with a domain-neutral core and explicitly
installed optional domain intelligence.

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
subjects. No major Stage 1-3 subsystem has been ported yet.

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
track:

1. Harness Foundation - broad TypeScript reference established.
2. Godot Script-Development MVP - broad TypeScript reference established, with
   unsafe effects truthfully unavailable.
3. Godot-Native Development MVP - broad TypeScript reference established.
4. Runtime and Visual QA - not begun.
5. Extensibility and Optional Agents - not begun.
6. Controlled Evolution and Stable Release - not begun.

Current Stage 3R position:

```text
R1      COMPLETE
R2      COMPLETE
R3      NEXT
R4-R12  NOT DUE
```

Status changes require executable evidence and an update to
[ROADMAP.md](../../ROADMAP.md) and the
[migration register](RUST_MIGRATION.md). Stage 3R is not a seventh public stage.

## 4. Stage 3R migration track

| Milestone | Scope                                                          |
| --------- | -------------------------------------------------------------- |
| R1        | Siralos rename + Rust engineering/domain-neutral foundation    |
| R2        | Differential Behavioral Harness                                |
| R3        | Domain-Neutral Core                                            |
| R4        | Generic Workspace / Project Foundation                         |
| R5        | Language Intelligence                                          |
| R6        | Domain Capability Architecture                                 |
| R7        | Providers / Tools / CLI                                        |
| R8        | Godot Stage-2 parity                                           |
| R9        | Godot Stage-3 parity                                           |
| R10       | H1 / H2 / ICM / H3 parity                                      |
| R11       | Full differential/effect parity                                |
| R12       | TypeScript reference retirement or explicit retention decision |

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

The conceptual ownership layers are:

```text
Foundation
    |
    v
Deterministic Runtime
    |
    v
Extensions
    |
    v
Orchestration
    |
    v
Experience
```

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
[ARCHITECTURE.md](../../ARCHITECTURE.md) and the
[architecture index](../architecture/README.md).

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

## 10. Provider model

Siralos is provider-neutral. Provider/model code cannot decide host tool
authority, approval, security policy, or authoritative task completion. Model
and provider identity plus relevant generation configuration are recorded where
reproducibility requires them. Real provider integrations are not implemented
in the current milestone.

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
evidence. Domain code receives only structurally granted imports; package
identity and capability requests remain host-controlled. Reopening the decision
requires new evidence and a superseding ADR.

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

## 16. Stage 5 / 6 guardrails

Stage 5 may later own runtime protocol/SDK/ACP work, skills/plugins/hooks,
optional workers or subagents, TaskGraph, worktree isolation, and durable work.
Stage 6 may later own `/evolve`, benchmarks/ablations, release provenance,
controlled improvement, and stable-release work. These are planned/not due and
must not be pulled into R3.

## 17. Harness-derived design lessons

Adopted principles, independent of any conversational source:

- typed control operations are shared across surfaces;
- lifecycle generation/identity fences stale work exactly;
- read-only observation never manufactures observed state;
- unknown/unavailable reasons are typed;
- operation receipts are bounded and redacted;
- durable sequenced runtime events arrive only in their owning stage;
- worker/model identity becomes explicit when workers arrive.

The last two are **PLANNED / NOT DUE**, not current defects.

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
