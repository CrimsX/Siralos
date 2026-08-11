---
id: ADR-0024
status: accepted
domains: [documentation, context, executor-briefing]
paths: [packages/core/src/executor/**, docs/architecture/**, AGENTS.md]
supersedes: []
---

# ADR 0024 — Scoped Documentation and Progressive Context

- Status: accepted (Stage 3 milestone 3.7B, Solaris documentation
  context optimization)
- Date: current milestone
- Related: ADR 0017 (project instructions and knowledge), ADR 0022
  (executor briefing), ADR 0023 (workspace scope and documentation
  context discipline)

## Problem

Coding executors working on Solaris can accumulate excessive context
from: large root guidance, broad architecture documents, milestone
history mixed into current architecture, duplicated security and
development rules, all ADRs, handoff/current-state documents, and
future roadmap material. This wastes context and makes current
authority harder to identify.

## Decision

Use thin root guidance plus path-scoped guidance plus an architecture
index plus current subsystem documents plus mapped current ADRs, with
deterministic bounded selection (ADR 0023's `DocumentationContextSelector`):

- The root `AGENTS.md` stays a thin universal contract (architecture
  direction, security posture, development discipline, documentation
  discovery rules) and never carries subsystem history or detailed
  requirements.
- Path-scoped `AGENTS.md` files exist only where a directory has
  meaningful domain-specific guidance: `packages/core/AGENTS.md`,
  `packages/adapters/AGENTS.md`, `packages/adapters/src/godot/AGENTS.md`,
  and `apps/cli/AGENTS.md`. They contain owner maps, domain invariants
  not already in root, and navigation (relevant architecture sections,
  applicable ADRs) — they never duplicate root rules and are registered
  in the runtime documentation index for deterministic path-scoped
  selection.
- `docs/architecture/README.md` is the architecture index (domain →
  source paths → current architecture docs → applicable ADRs), and the
  runtime `DOCUMENTATION_INDEX` is its machine-readable counterpart.
- ADRs own historical reasoning, tradeoffs, and decisions; every ADR
  carries machine-selectable frontmatter (`id` / `status` / `domains` /
  `paths` / `supersedes`). Superseded, deprecated, and archived
  documents are excluded from ordinary selection; `docs/archive/` holds
  only obsolete non-authoritative material.
- The roadmap stays high-level (stage / milestone / objective / status);
  detailed requirements live in Milestone Manifests. The four large
  documents (README, ARCHITECTURE.md, ENGINEERING.md, SECURITY.md)
  remain single authoritative documents; the index maps to them instead
  of splitting them or copying their rules elsewhere.
- Selection is deterministic path/domain mapping, bounded by a
  documentation budget that preserves mandatory instructions, direct
  architecture, and critical current ADRs before historical/background
  material. Documentation metadata is navigation, never security policy.

## Rejected

- **Giant root `AGENTS.md`** — universal rules only; history lives in
  ADRs and the roadmap.
- **Recursive docs loading** — normal execution never enumerates or
  reads every Markdown document.
- **One monolithic `ARCHITECTURE.md` as the only mapping** — an index
  maps domains to it instead of requiring full reads.
- **Reading every ADR for every task** — mapped current ADRs only;
  superseded/deprecated/archive excluded by default.
- **Giant handoff/current-state document as required context** — the
  status documents stay navigational (README as status, ROADMAP as
  milestone status); unique history moves to ADRs/archive when needed.
- **Semantic/vector search as the first solution** — deterministic
  path/domain mapping is sufficient and testable.
- **Deleting useful history instead of separating it** — history moves
  to ADRs or the archive; it is never silently dropped.
- **Copying the same rules into many Markdown files** — one canonical
  owner per recurring topic; other documents reference it.

## Consequences

- An executor working on one subsystem normally receives: root
  `AGENTS.md`, the applicable scoped `AGENTS.md` file(s), the mapped
  architecture document, a small set of applicable current ADRs, and the
  task manifest/brief — instead of the documentation tree.
- Path-scoped `AGENTS.md` files participate in the existing
  project-instruction precedence (root → ancestor → most specific), so
  documentation restructuring adds no competing instruction engine.
- The docs-consistency architecture check keeps ADR frontmatter, the
  runtime index, and the scoped-guidance files aligned; broken mappings
  and status drift fail validation.
- Documentation remains derived context: it never grants capability,
  and archived/superseded material is never active authority.
