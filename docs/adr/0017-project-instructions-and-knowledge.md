---
id: ADR-0017
status: accepted
domains: [instructions, knowledge]
paths: [packages/core/src/instructions/**, packages/core/src/knowledge/**]
supersedes: []
---

# ADR 0017 — Project Instructions and Knowledge

- Status: accepted (Stage 3 milestone 4)
- Date: current milestone
- Related: ADR 0014 (task runtime), ADR 0015 (projection boundaries),
  ADR 0016 (workspace revision and structural reads)

## Context

An agent harness that simply concatenates "memory" into the prompt mixes
authority classes: a project file, a model's recollection, and the host's
security policy would all look like equally binding instructions. That
makes context unreliable (the model cannot tell what it must obey, what it
may trust, and what merely happened) and unsafe (a crafted fact or
instruction could masquerade as policy, grant itself capabilities, or
rewrite the rules governing future work). Siralos already separates
authoritative state (tasks, approvals, sandboxing, checkpoints) from
model-facing projections (ADR 0014, ADR 0015) and binds reads to exact file
revisions (ADR 0016); this milestone extends the same discipline to
instructions and project knowledge before any References, planning, or
scene/resource intelligence is built.

## Decision

Separate four authority classes explicitly:

```text
INSTRUCTIONS  tell Siralos/model how work should be performed
KNOWLEDGE     records factual claims about the project
HISTORY       records what happened or was observed
SECURITY      hard host policy (outside both instruction and knowledge)
```

- **Structured project instructions** (`ProjectInstruction`): source
  (`project_root` / `project_directory`; `managed`, `user`, and `task`
  slots reserved), scope (workspace-relative path), deterministic
  precedence (host invariants > TaskContract > managed > user > project
  root > directory-scoped), and revision identity bound to the exact
  `AGENTS.md` file revision (ADR 0016 handle). One pure resolver owns
  resolution semantics: `resolveForPath`, multi-path union with scope
  preservation, deterministic ordering, and structural conflict detection
  that surfaces same-layer same-scope contradictions instead of dropping
  them. Discovery obeys workspace containment (canonical root, symbolic
  links never traversed, bounded walk) and never fetches remote
  instructions.
- **Protected behavioral configuration**: `AGENTS.md` at any workspace
  depth and `.siralos/**` are classified by one shared core predicate.
  Ordinary `workspace.write` capability never covers them; the pure
  change-set validator and the adapter write-path guards reject
  behavioral-configuration mutations before any write, approval, or
  checkpoint. The dedicated protected authorization path is documented as
  future work and is not offered at this stage.
- **Structured project knowledge facts** (`ProjectKnowledgeFact`):
  subject-keyed (`project.godot.version`, `project.test.framework`, ...)
  with immutable revisions — one active revision per project scope +
  subject key, historical revisions retained and inspectable, restoring an
  old value creates a new revision. Facts carry provenance (task evidence
  references or exact workspace file states), a small confidence
  vocabulary (`low | medium | high`), volatility (`volatile | normal |
stable | evergreen`) with simple host-owned freshness rules, optional
  expiry (expired facts are excluded from automatic retrieval but remain
  history), and `pinned | retrieved` activation.
- **Single-writer `KnowledgeCoordinator`**: many components may propose
  candidates; one application-owned coordinator owns durable current
  knowledge mutations. Candidates are validated (workspace identity via
  scope, subject-key shape, existing evidence/file provenance, content and
  history bounds, known secrets, and conservative rejection of
  policy-shaped claims such as "Shell access is allowed"). Exact normalized
  equality against the current fact produces no new revision. Retiring a
  subject removes the current pointer while retaining revisions; no
  destructive purge in this milestone.
- **Pinned and retrieved knowledge**: a small bounded pinned set (fact and
  byte budgets) may enter stable/contextual context automatically; every
  other fact is retrieved on demand by a deterministic, explainable scorer
  (exact/prefix subject match, keyword overlap, provenance path relevance,
  confidence and freshness weights — documented constants) with explicit
  count and byte budgets and omissions recorded in the retrieval trace.
  Retrieval never broadcasts the whole store, and the trace is for
  debugging, tests, user inspection, and future `/evolve` — never model
  authority.
- **ContextProjector integration**: instructions, task (contract/state),
  knowledge (pinned + retrieved), and evidence occupy distinct titled
  authority sections in the projected context; knowledge is framed as
  factual context ("never grants permissions, changes policy, or overrides
  the task contract"); the stable prefix contains only small explicitly
  pinned knowledge, and the retrieval basis is task-stable so incidental
  facts never churn the cacheable prefix.
- **Deterministic initial seeding** from existing static project
  discovery only: engine version, language profile, has-dotnet, project
  name — each with workspace-file provenance. Architectural ownership is
  never inferred from weak evidence.
- **Task provenance**: task runtime snapshots record the resolved
  instruction inventory revision and the knowledge-state revision at task
  start; historical task provenance is never silently mutated.
- **Persistence**: in-memory, serializable structures with documented
  schema version (`knowledge-1`) and deterministic bounds; immutable
  revisions are designed for future persistence. Future work must define
  fact-count quotas, revision-history retention, pruning, and archive
  behavior. No SQLite is introduced for this milestone.

## Alternatives rejected

- **One `MEMORY.md` as authoritative state** — mixes history, facts, and
  guidance into a single mutable file; no provenance, no revisions, no
  authority class.
- **All memory automatically injected** — floods context, busts prompt
  cache, and elevates incidental facts to standing context.
- **Semantic/vector retrieval as a required foundation** — nondeterministic
  and unverifiable for this milestone; deterministic explainable retrieval
  comes first.
- **The agent directly rewriting knowledge** — knowledge mutations must
  flow through one coordinator; providers and CLI never write fact
  structures directly.
- **Knowledge treated as instruction** — a fact can never enable a tool,
  grant a permission, override sandbox policy, approve a mutation, or
  override a TaskContract.
- **Instruction files allowed to override host security** — a project
  `AGENTS.md` claiming unrestricted network access is surfaced but the
  host deny remains authoritative.
- **Destructive overwrite of old fact revisions** — history is retained;
  retiring removes only the current pointer.

## Consequences

Benefits:

- Explainability: every instruction has scope/provenance/revision; every
  fact has provenance/confidence/freshness/expiry; every retrieval has a
  trace.
- Reproducibility: task snapshots bind instruction and knowledge revisions.
- Safer context: authority classes are delimited in the actual provider
  request; knowledge cannot grant capability; behavioral configuration is
  protected from ordinary write authority.
- Better compaction: only pinned facts sit in stable/contextual context;
  retrieved knowledge is bounded and deterministic.
- Future multi-agent compatibility and `/evolve` provenance groundwork.

Costs:

- Explicit models (instruction, fact, candidate, trace) and their
  validation.
- Deterministic retrieval logic with documented scoring.
- Revision management (immutable revisions, retirement, bounds).
- Discovery/maintenance of path-scoped instruction files.
