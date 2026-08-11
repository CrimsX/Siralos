---
id: ADR-0018
status: accepted
domains: [references, research]
paths:
  [
    packages/core/src/reference/**,
    packages/core/src/research/**,
    packages/adapters/src/reference/**,
    packages/adapters/src/research/**,
  ]
supersedes: []
---

# ADR 0018 — External References and Research Sources

- Status: accepted (Stage 3 milestone 5)
- Date: current milestone
- Related: ADR 0014 (task runtime), ADR 0015 (projection boundaries),
  ADR 0016 (workspace revision and structural reads), ADR 0017 (project
  instructions and knowledge)

## Context

A Godot-native development harness needs upstream material: engine source,
addon repositories, and published documentation. Solaris must be able to
consult that material without ever treating it as editable workspace state
or as trusted instructions. ADR 0017 established that instructions,
knowledge, history/evidence, and security policy are distinct authority
classes; external material must not blur those lines either. A repository
README is not policy, a fetched documentation page is not project
knowledge, and neither can grant a capability — but an agent that simply
concatenates everything it found into the prompt would make all of it look
equally binding. Solaris also has no sandboxed Git execution and no
outbound network from sandboxed processes at this stage, so any repository
materialization or remote research must be designed around what can
mechanically fail closed today.

## Decision

Separate three resource classes explicitly:

```text
WORKSPACE   editable project state (the canonical launch directory)
REFERENCE   read-only external material (a local directory outside the
            workspace, or a remote repository pinned to a commit)
RESEARCH    transient external evidence fetched through bounded source
            ports (repository files, Godot documentation)
```

The invariants that govern them:

```text
Reference content is read-only untrusted data.
Research content is transient external evidence.
Neither is instruction authority. Neither grants capability.
Neither becomes project knowledge automatically.
```

- **Read-only reference registry (single owner).** The core
  `ReferenceRegistry` is the SINGLE application-owned owner of reference
  identity. Declarations arrive pre-parsed (strict bounded parsing of the
  untrusted `reference` config section; unknown keys rejected so secrets
  cannot hide inside a declaration; aliases match `^[a-z][a-z0-9._-]{1,63}$`
  and are bounded in count). The registry resolves every declaration at
  creation through a resolver port, records immutable revisions, and
  exposes the ONLY way a revision changes: `refresh`. CLI, provider
  adapters, ContextProjector, and EvidenceProjector never resolve or
  refresh references themselves. Declined or unresolvable references stay
  listed with a precise `failureReason` so the configuration remains
  visible and auditable. Trust classes (`explicit-user` /
  `trusted-project` / `untrusted-project` / `managed`) are metadata for
  policy decisions made elsewhere; possession of a reference never grants
  capability. Local-directory references must resolve outside the
  workspace namespace (registry-level check at resolution and refresh,
  re-verified by the adapter at access time).
- **Pinned repository identity.** A repository reference pins exactly one
  of commit / tag / branch (an absent ref defaults to the mutable branch
  `main`). Mutable refs are refused unless an explicit
  `allowMutableRefs` policy is set, and resolution always records the
  resolved commit — never the branch name — as the revision identity.
  There is no silent branch-following: an active task keeps the revision
  it was bound to (`bindTask` snapshots), and advancing to a new commit
  requires an explicit `refresh`. A failed refresh invalidates the current
  revision (fail closed — a stale identity is never served silently),
  while historical revisions stay reachable through task bindings and
  evidence. Repository origins are normalized to a canonical GitHub form
  (`owner/repo`, https only, no credentials, no query/fragment, no extra
  path segments).
- **Managed cache outside the workspace (fail-closed at this stage).**
  Repository materialization targets Solaris-owned private storage outside
  the workspace (documented layout `~/.solaris/references/<fingerprint>/`
  with a `metadata.json`); the managed-cache root is never model-facing,
  and cache content is never presented as workspace material. At this
  stage real repository materialization is `unavailable`: it requires
  sandboxed Git execution, which does not exist yet, so nothing is ever
  spawned, fetched, or created (the production materializer and cache
  store are fail-closed no-ops with typed `unavailable` outcomes).
  Local-directory references need no copy — the directory IS the
  reference, materialized with zero filesystem operations as a direct
  read-only root.
- **Reference access tools.** `reference.list`, `reference.read`, and
  `reference.search` provide bounded read-only access over a materialized
  reference under the `reference.inspect` capability. Paths are
  reference-root-relative with the same class of containment as workspace
  paths: no absolute paths, no surviving `..`, symlink escapes rejected
  against the canonical root (deepest-existing-ancestor realpath when the
  target is missing), null bytes rejected, backslashes treated as
  separators everywhere. `reference.read` mirrors the workspace read
  modes — `exact` (authoritative source with SHA-256), `structural`
  (deterministic GDScript declarations), `summary` (bounded advisory) —
  and reads are capped (1 MiB files, 64k content characters; non-UTF-8 is
  `unsupported`). Search carries independent global traversal budgets with
  explicit truncation disclosure. Results carry the registry-bound
  revision anchor (commit or fingerprint), never absolute paths; identity
  is never inferred from a model-supplied path (unknown references fail
  closed as `unavailable`).
- **`ResearchSource` port + bounded normalized documents + policy gate.**
  Core defines the `ResearchSourcePort` (fetch one bounded document) and
  the `ResearchTransportPort` (one bounded https GET; no HTTP library in
  core). The application-owned `ResearchService` is the single gate:
  the `research.fetch` capability must evaluate to `allow` BEFORE any
  source port is invoked (there is no approval protocol for research in
  this milestone, so `ask` is refused too), every built-in profile denies
  it by default, and the request is validated against the bounded model
  (query/topic/path/ref/version shapes and caps) and must name a
  configured source (the configured identity, not model-supplied display
  metadata, is passed to the source). Fetches race the caller's abort signal and a timeout
  (10 s, 30 s hard lifetime) and are bounded at every layer: 2 MiB
  download cap, 256 KiB normalized document cap, section/link/heading caps,
  and explicit truncation disclosure. Provenance records what was asked
  for versus what was served (`requestedRef` / `resolvedRevision`,
  `requestedVersion` / `usedVersion`), with any fallback (e.g. Godot docs
  patch → minor → `stable`) explicitly marked — mismatched guidance is
  never served silently. The HTTPS transport applies an exact per-source
  DNS-host allowlist to the initial request and every redirect, rejects URL
  credentials and alternate ports, and uses one deadline for the full chain.
  Asynchronous results are stale-result-bound inside the service: every fetch
  mints a `requestId`, snapshots the exact active task id and TaskContract
  revision, then re-checks both before returning or retaining a document.
  Stale results are discarded before evidence or context, so callers cannot
  omit the check.
- **Evidence integration.** Task evidence gains `reference_read`,
  `reference_search`, and `research` kinds with bounded sources
  (reference id/alias/revision/path/mode/SHA-256; search query and match
  count; research source/requestId/revision/version/fallback/bounded
  excerpt). A `KnowledgeCandidate` may cite `research_evidence`
  provenance only through an explicit `propose` call with host
  verification (`hasResearchEvidence`) — research never becomes knowledge
  automatically, and malformed or unknown evidence ids are rejected.
- **Projection integration.** ContextProjector renders distinct volatile
  `[Reference evidence]` and `[Research evidence]` sections after
  `[Latest evidence]` (bounded: 4 views each, combined budget, explicit
  truncation; never in stable/contextual segments; never absolute cache
  paths). ToolProjector gates visibility as mode ∩ capability policy ∩
  provider capability: `reference.inspect` is `allow` in every built-in
  profile, `research.fetch` is `deny` in every built-in profile, and
  hidden research tools are absent from the provider schema (hidden, not
  "permission denied"). Review mode exposes reference inspection tools by
  exact name but never research tools (network is not in the review-mode
  capability allowlist).
- **Architecture enforcement and fixtures.** Core reference/research
  modules are provider-neutral, Node-free, and mutation-free; the
  adapter's production repository resolver/materializer never contain a
  Git implementation; behavior fixtures cover the reference/research
  surface (fixtures 1–30 and effect tests 51–54, deterministic and
  network-free) with final-boundary assertions: workspace mutation APIs
  reject reference paths before any write or checkpoint, denied research
  never invokes a configured source port, reference/research material
  appears under evidence sections never under instruction authority, and
  task snapshots keep identifying commit A until an explicit refresh.

## Alternatives rejected

- **Clone external repositories into the workspace** — mixes untrusted
  upstream content into editable project state and into the workspace
  tool namespace; nothing would stop workspace tools or mutations from
  reaching it.
- **Workspace tools inspecting arbitrary external directories** — would
  give the model's existing read tools unbounded host-reach and blur the
  workspace/reference namespace boundary; references are instead declared,
  bounded, and identity-tracked.
- **Generic unrestricted web fetch tool** — unbounded, unprovenanced, and
  impossible to gate precisely; research is instead a bounded typed
  surface with provenance and a dedicated capability.
- **Reference content as trusted instruction** — a repository README is
  untrusted data; it is surfaced under evidence sections, never under
  instruction authority (ADR 0017 classes preserved).
- **Automatic branch updates during active tasks** — a task must keep the
  revision it observed; advancing requires an explicit refresh, and a
  failed refresh invalidates rather than silently serving a stale
  identity.
- **Automatic reference execution** — nothing in reference content is
  ever executed; repository materialization itself is unavailable until
  identity-bound sandboxed Git execution exists.
- **Auto-converting web/repository findings to memory** — research
  becomes knowledge only through an explicit host-verified `propose`;
  there is no automatic proposal path.
- **Provider adapter owning research retrieval** — research is
  application-owned state: the service gates, bounds, and records
  evidence; providers only invoke the gated surface.

## Consequences

Benefits:

- Safer upstream inspection: reference content is read-only, contained,
  and identity-tracked; research is gated, bounded, and cancellable;
  neither can grant capability or masquerade as instructions.
- Precise provenance: every reference observation is bound to a revision
  (commit or fingerprint); every research document records requested vs
  resolved identity, version fallbacks, and fetch time.
- Reproducibility: task bindings snapshot reference revisions; task
  evidence carries the exact identity the task observed.
- Smaller context: reference/research material enters only bounded
  volatile evidence sections; nothing is promoted to standing context.
- A clean foundation for `/evolve`-style research and for Godot/addon
  research without opening the workspace namespace to upstream content.
- Fail-closed honesty: repository materialization and the managed cache
  report `unavailable` with precise reasons instead of inventing storage
  that cannot be identity-bound.

Costs:

- Cache and reference management: declared references, registry policy
  (mutable-ref refusal, workspace containment), refresh semantics, and
  (future) cache hygiene and quotas.
- Network policy integration: `research.fetch` as a separate capability,
  denied by every built-in profile, with no approval protocol until one
  is explicitly designed.
- Additional identity models: reference revisions and task bindings,
  research provenance and stale-result binding, and the bounded evidence
  shapes that carry them.
