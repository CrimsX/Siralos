---
id: ADR-0023
status: accepted
domains: [executor-briefing, context, workspace-scope, documentation]
paths: [packages/core/src/executor/**]
supersedes: []
---

# ADR 0023 — Workspace Scope and Documentation Context Discipline

- Status: accepted (harness context & repository optimization,
  milestone 3.7A, Part D–T)
- Date: current milestone
- Related: ADR 0014 (task runtime), ADR 0015 (projection), ADR 0017
  (instructions/knowledge), ADR 0020 (planning), ADR 0022 (executor
  briefing foundation)

## Context

ADR 0022 introduced the structured briefing foundation: Execution
Contract, Milestone Manifests, evidence-backed acceptance, the
ExecutorContextPack, and the ExecutorBriefCompiler. What remained
unstructured was the _source_ and _documentation_ side of executor
context. Executors still had no derived task scope (verified vs.
candidate files), no current-step working set, no source-context
budgets, no default exclusions for generated/vendor paths, no new-file
discipline, and no deterministic documentation selection — so a task
could still wander the repository and ingest the docs tree. The
milestone's goal was a small task-specific working set and
documentation set: root `AGENTS.md`, applicable nested `AGENTS.md`, one
relevant architecture doc, 1–3 relevant ADRs, the milestone manifest,
and a compiled brief.

## Decision

Introduce four derived, host-owned layers, all in
`packages/core/src/executor` and all architecture-checked like the
briefing foundation:

1. **WorkspaceScope** (`workspace-scope.ts`) — one task's derived
   execution scope: verified files (with exact revision handle and
   `kind:ref` evidence), candidate files (paths only; contents never
   enter context), allowed create roots, excluded paths, and a
   `WorkspaceContextBudget`. Verified and candidate sets never overlap;
   promotion requires evidence and an exact revision and is recorded as
   a `ScopePromotionRecord` (`candidate -> structural/summary inspection
-> relevance evidence -> verified`). `ActiveWorkingSet` is the
   current plan step's smaller subset, every file carrying an inclusion
   reason (direct task target, dependency, test counterpart,
   architecture owner, validation target, candidate under
   investigation). Budget eviction demotes exact views to summaries in
   a deterministic order (candidate details first, then exact source
   outside the working set) and always retains revision identity and
   evidence references — authoritative evidence is never deleted.
   Default exclusions (`node_modules/`, `dist/`, `build/`, `coverage/`,
   `.git/`, `.godot/`, generated output) suppress noisy paths from
   default discovery; exclusion is context suppression, never security
   denial.

2. **New-file discipline and scope evaluation** (`new-file-discipline.ts`)
   — new production files carry a bounded rationale naming the existing
   owners inspected (extend the existing owner before creating an
   adjacent abstraction). `detectProliferationSignals` is a set of
   deterministic REVIEW signals (many new files, tiny helpers, new
   directories for a narrow change, files outside the planned scope) —
   heuristics that feed review findings, never hard rules. `evaluateScopeDiff`
   classifies the actual diff against the planned scope as expected /
   justified expansion (rationale recorded) / unexplained expansion.

3. **Documentation context selection** (`documentation-context.ts`) —
   deterministic selection in canonical order: root `AGENTS.md` always,
   path-scoped nested `AGENTS.md`, concern-mapped architecture docs,
   accepted ADRs (ordered by concern overlap so the most specific
   survives the budget), development docs last. Superseded/deprecated
   and `docs/archive/` material is excluded. A `DOCUMENTATION_BUDGET`
   bounds the selection and records drops. ADRs carry machine-selectable
   frontmatter (`id` / `status` / `domains` / `paths` / `supersedes` /
   `supersededBy`) parsed by `parseAdrFrontmatter`; `docs/architecture/README.md`
   is the human-readable index/map of the same domains. No semantic or
   vector search — path/domain mapping only, and the docs tree is never
   recursively ingested.

4. **Integration into the pack and brief** — `ExecutorContextPack`
   carries bounded workspace-scope, working-set, documentation,
   scope-signal, and new-file refs, and filters capability guidance by
   area so irrelevant unavailable capabilities are omitted. The brief
   compiler renders verified workspace files, the working set,
   documentation sources, new-file rationales, and scope warnings as
   low-priority sections, and the rendered brief redacts known
   credential-shaped tokens at the projection boundary (sharing the
   `SECRET_PATTERNS` owner in `doctor/safe-report.ts`). Private
   continuation/reasoning content has no pack field and never enters.

## Rejected

- **Making WorkspaceScope a security authority** — it is derived scope;
  capability/path policy stays authoritative.
- **Semantic/vector documentation search** — deterministic
  path/domain mapping is sufficient and testable.
- **Hard file-count rules** — budgets control context, not repository
  access; proliferation heuristics are review signals only.
- **Archiving accepted ADRs by age** — superseded state stays in
  frontmatter; the archive holds only obsolete non-authoritative
  material.
- **Rewriting the giant architecture/security docs** — they remain the
  authoritative single documents; the index maps to them instead of
  duplicating them.
- **Truncating mandatory rules under budget pressure** — budgets drop
  background/historical material before applicable guidance.

## Consequences

### Benefits

- Executor context is bounded on the source side and the documentation
  side: one subsystem task selects a small, relevant set instead of
  wandering the repository or ingesting `docs/`.
- Scope expansion is explicit and observable (promotion records,
  rationale, scope warnings, scope-diff classification).
- ADR metadata and the architecture index make documentation selection
  deterministic and checkable; `npm run check:architecture` keeps
  frontmatter and the runtime index consistent.
- Secrets and private continuation stay out of the provider-visible
  brief boundary.

### Costs

- Index and frontmatter maintenance: the runtime documentation index
  must stay aligned with `docs/adr/` and `docs/architecture/README.md`
  (enforced by the docs-consistency architecture check).
- Additional pack/brief surface: new bounded fields must stay within
  limits and remain derived (architecture checks apply unchanged to the
  new executor modules).

### Security posture

WorkspaceScope, documentation selection, and the context pack carry no
capability/policy surface and cannot grant capability; the new modules
fall under the existing executor-briefing architecture rules (no
security/capability/approval/network/fs imports). Provider adapters
never recreate selection semantics (`BRIEFING_BANNED_IDENTIFIERS`
covers the new surfaces). Archived/superseded documentation is not
active authority and grants nothing.
