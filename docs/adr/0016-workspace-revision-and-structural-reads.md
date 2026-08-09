# ADR 0016 — Workspace Revision and Structural Read Foundation

- Status: accepted (Stage 3 milestone 3)
- Date: current milestone
- Related: ADR 0005 (approved workspace mutations), ADR 0014 (task runtime),
  ADR 0015 (projection boundaries)

## Context

Exposing only raw 64-hex SHA-256 digests to the model is awkward and
error-prone: the model must copy long digests, the same digest appears for
every read of the same state, and there is no session-scoped notion of "the
file state I read". At the same time, token-efficient exploration needs
cheaper views than full source: the model should be able to scan the
structure of a file (declarations, signatures, dependencies) or get a
bounded summary before deciding which file deserves an exact read — without
any of those views ever becoming the basis for a mutation. Finally, the
mutation path already revalidates whole-file SHA-256 at prepare and apply
time, but nothing binds _reads, summaries, structural views, or validation
results_ to the specific file state they concern, which later milestones
(re-review, agent teams, stale-read warnings) will need.

## Decision

- **Opaque model-facing revision handles.** Every exact/structural/summary
  read issues a `rev_<32 hex>` handle bound to
  `(workspaceFingerprint, path, file-SHA-256)`. The handle is an ergonomic
  reference, never authority: possession grants no read/write/approval/path
  access, and the host always resolves the handle back to the trusted
  SHA-256 before any mutation. Handles are workspace-scoped (the same
  relative path in a different workspace never resolves) and live in a
  session-scoped, bounded, in-memory registry (no durable storage).
- **Explicit read modes.** `workspace.read` gains
  `exact | structural | summary`:
  - `exact` — authoritative source access, returns the revision handle,
    the SHA-256, and the bounded text/range. Only exact source plus a valid
    revision is the basis for a text mutation.
  - `structural` — deterministic GDScript declaration structure
    (extends/class_name, file and declaration annotations, signals, enums,
    constants, typed/untyped properties, function signatures with
    parameters/return/static, dependencies, source lines) from a
    lightweight scanner with string/comment awareness; keywords in comments
    or strings never produce declarations; syntactically invalid files
    yield a structured `partial` result with parser errors, never
    fabricated structure and never an infrastructure exception; declaration
    output is deterministically capped with an explicit truncated flag.
  - `summary` — a bounded, structure-first advisory overview that always
    states the exact revision it summarizes and always carries an advisory
    footer ("not authoritative source"); the footer is never truncated
    away, and the summary never exceeds its byte budget.
- **Revision-aware mutations.** The change-set operation accepts either the
  existing raw `expectedSha256` (compatibility boundary) or an opaque
  `expectedRevision`. The host resolves the handle to its SHA-256 and the
  existing prepare-time and apply-time revalidation runs unchanged. A
  mismatch produces a precise structured `stale_revision` result
  (path, expected revision, current revision) with user-facing guidance —
  never a fuzzy merge, never silent retry. Successful mutations
  automatically issue the new post-edit revision and invalidate the
  previous current binding; externally changed files fail the next
  revision-bound operation.
- **Revision-aware evidence.** Workspace-derived evidence can carry the
  revision handle (read/structural/summary results expose it; task
  `workspace_read` and `mutation` evidence records support it; the
  EvidenceProjector preserves it in model views; the development bridge
  attaches the post-edit revision to mutation evidence, binding parser/LSP
  validation to the resulting revision where the architecture naturally
  supports it). Old revisions remain resolvable as historical evidence but
  are never promoted to current.
- **Groundwork only.** A small session-local observed-reads record
  (`path, revision, mode`) is kept for future multi-agent stale-read
  detection. No worktrees, write leases, broadcasts, or Agent Hub
  notifications are implemented.

## Alternatives rejected

- **Short/non-cryptographic line hashes** — revision identity stays backed
  by whole-file SHA-256; handles are derived from the cryptographic
  identity tuple and the registry is authoritative for resolution.
- **Fuzzy automatic stale-edit application** — a stale pre-state identity
  hard-fails the prepared change set; no best-effort merging.
- **Path-only identity** — the same path in a different workspace never
  resolves.
- **Summary-based edits** — summary and structural views are advisory; a
  change set still requires an exact pre-state identity, and a summary
  without one is rejected as invalid input.
- **Structural mode pretending to be authoritative source** — structural
  output is declaration metadata with explicit bounds and status; exact
  mode remains the only authoritative source access.
- **No revision tracking for summaries** — every summary states the exact
  revision it summarizes so staleness is detectable.
- **One giant revision system** — the registry is a small session-scoped
  in-memory map; durable/global version control remains future work.

## Consequences

Benefits: safer repeated edits (a second edit requires a fresh post-edit
revision, verified end to end), smaller model context (structural/summary
exploration before exact reads), better exploration ergonomics (opaque
handles instead of raw digests), multi-agent groundwork (observed-read
records, revision-tagged evidence), and clearer evidence lineage
(validation/review evidence can state which file state it concerns).

Costs: a revision registry (bounded, in-memory), a lightweight GDScript
scanner (syntax-derived only; not a compiler), and additional behavior
tests at the final mutation and read boundaries.
