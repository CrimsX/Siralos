---
id: ADR-0012
status: accepted
domains: [godot, development]
paths: [packages/core/src/godot/**, packages/adapters/src/godot/**]
supersedes: []
---

# ADR 0012: Bounded GDScript development and repair loop (fail-closed at this stage)

Status: accepted

## Context

Solaris can inspect a Godot project, look up exact-engine API knowledge
(ADR 0010), run `--check-only` parser validation (ADR 0010), and host a
bounded recovery-mode GDScript language session (ADR 0011) — but it cannot
yet turn a development request into an approved, checkpointed, validated
source change. This milestone integrates those primitives into one bounded
development workflow: the provider investigates with read-only tools,
proposes an exact text change set, the user approves the exact change
once, Solaris checkpoints, applies, parses with `--check-only`, recreates
a fresh language session, collects diagnostics, records validation
evidence, and — when validation reports errors — runs a bounded repair
loop where every repair requires its own approval.

The security posture of every earlier Godot surface applies here too:
execution that cannot be mechanically identity-bound fails closed. The
change-set applier requires a directory-relative (openat/renameat-style)
commit primitive that Node does not offer, so on every platform the
workflow refuses with a typed `unavailable` outcome before any approval
for a mutation, no checkpoint is created, and nothing executes — exactly
like engine probing (ADR 0008), recovery probing (ADR 0009), knowledge
generation and check-only diagnostics (ADR 0010), and LSP session startup
(ADR 0011) at their stages.

## Decision

### A focused workflow, not a workflow engine

One explicit discriminated-union state machine: `investigating →
proposal_ready → awaiting_approval → applying → parser_validation →
language_validation → reviewing` with the terminal statuses `completed`,
`completed_with_warnings`, `completed_with_errors`, `denied`, `conflict`,
`cancelled`, `apply_failed`, `validation_failed`, and `unavailable`. One
active workflow per Solaris session, in-memory only, provider-neutral,
containing no credentials, no mirror host paths, and no raw LSP transport
data. No generic workflow engine, no event bus.

### The workflow composes existing primitives; it never bypasses them

- **Every source mutation is an exact approved change set.** The provider
  proposes bounded create/edit/delete operations on UTF-8 text files with
  exact current SHA-256 preconditions; preparation is read-only and freezes
  an immutable digest; the one-time approval binds to exactly that digest;
  application consumes the prepared change set (never reusable) and runs
  the existing mutation lock, hash-gated preconditions, and per-file
  checkpoints before any file is written.
- **Checkpoints remain mandatory.** Every affected existing file gets a
  checkpoint (with its exact pre-change bytes) and every create gets an
  absence state before anything is applied; a checkpoint failure applies
  nothing.
- **The language session is restarted, never live-synced.** After an
  approved edit the active session is suspended (`closing_for_edit`), new
  LSP queries are rejected while it closes, and the edit never proceeds if
  the suspension fails. After the apply, a fresh disposable mirror is
  prepared and a fresh session starts. Restarting gives fresh mirror
  hashes, fresh project state, fresh parser state, simpler correctness and
  rollback semantics, and no hidden stale document state. Live
  source↔mirror synchronization is deferred.
- **`--check-only` runs before LSP validation.** Changed `.gd` files are
  parsed sequentially with the fixed `--headless --path
<disposable-mirror> --script <mirror-script> --check-only` invocation
  (ADR 0010 discipline, architecture-enforced); a parser-invalid result is
  validation evidence, never an infrastructure failure, and never an
  automatic rollback.
- **Validation errors do not automatically roll back accepted edits.** The
  provider receives the exact diagnostics as evidence and proposes a
  focused repair — a new change set requiring a new approval. Denial of a
  repair preserves already approved changes and the workflow ends
  truthfully (`completed_with_errors`). Cancellation likewise preserves
  approved changes and reports an incomplete validation.
- **Repair iterations are bounded and immutable.** At most 3 repair
  proposals and 4 total iterations; the provider cannot raise these
  limits. Budget exhaustion ends the workflow truthfully.

### Approval semantics

The workflow start is one one-time approval binding the request text, the
project authored-file fingerprint, the engine fingerprint, and the
immutable limits. Its authorization covers only the read-only validation
context: LSP recreation after approved edits (the fresh session plan's
digest is accepted under this authorization only when the engine
fingerprint is unchanged and the project delta equals exactly the
approved change sets, with capabilities, sandbox profile, and network
policy unchanged), `--check-only` parsing, API lookup, workspace
inspection, and Git inspection. It never covers source mutations: each
change set (including every repair) still requires its own exact one-time
approval. While a workflow is active, `godot.lsp_session` and
`/gdscript-lsp` defer to the workflow's session ownership. No
"approve all repairs" option exists; the provider cannot approve its own
actions.

### Multi-file change sets and partial-application recovery

A change set is limited to 16 files, 32 replacements per file, a complete
diff of 512 KiB, and 4 MiB of resulting bytes; a truncated preview cannot
be approved (`changeset_too_large` requires splitting). Preparation
validates every path and protected path, reads and hashes every current
file, checks exact expected hashes, computes every resulting file in
memory, generates complete deterministic diffs, and freezes the digest —
all before any lock, approval, or checkpoint. Application is serialized
under the mutation lock: revalidate all preconditions, checkpoint every
file, verify durability, then apply sequentially with post-state hash
verification. A partial infrastructure failure triggers internal recovery
of exactly the files Solaris just changed, each gated on its current hash
still matching the partially applied result, restored from the
just-created checkpoint preimages; external changes are preserved and
reported. Outcomes are `apply_failed_recovered`, `apply_failed_partial_recovery`,
or `apply_failed_uncertain` — success is never reported after partial
application.

### Infrastructure failure vs invalid source

A script parse failure or an LSP error diagnostic is invalid source:
repairable evidence. A parser invocation that cannot run, an LSP session
that cannot start, or a validation budget that expires is an
infrastructure failure: the approved source changes stay, the workflow
ends `validation_failed`, and the source is never blamed for an
infrastructure problem (nor is clean success claimed when a gate was
skipped).

### Evidence

Per iteration, the workflow records bounded validation evidence: the
exact change set id, per-file before/after hashes, parser results, LSP
diagnostics, Git status when available, and a workspace-integrity check
against the workflow-start baseline — any file that changed outside the
approved change sets is an unexpected change, reported truthfully,
never reverted, and it invalidates the workflow (`conflict`). Evidence
never contains mirror host paths, credentials, or raw JSON-RPC data.
LSP diagnostics settle deterministically (initial receipt, bounded quiet
period, hard timeout) rather than assuming an instant empty result.

### Fail-closed at this stage

The change-set applier fails closed on every platform: Node offers no
directory-relative (openat/renameat) primitive, so a same-user process can
swap a parent or target at any instruction boundary. `isAvailable()` is
false, `workspace.apply_text_changeset` preparation refuses with a typed
`unavailable` result, the workflow refuses before any approval for a
mutation, no checkpoint is created, and nothing is written or deleted —
mirroring every other execution surface at this stage. The full
orchestration below the gate is tested internal code exercised through
injected in-memory file primitives, scripted language/parser services,
and the real filesystem checkpoint store; the opt-in `npm run
test:godot-development` conformance verifies the fail-closed truthfulness
against a real enforcing sandbox when one is available and always reports
the live development-loop isolation probe as skipped, never passed.

### Limits

Concurrent workflows 1 · files per change set 16 · complete diff 512 KiB
· resulting bytes 4 MiB · replacements per file 32 · repair proposals 3 ·
total iterations 4 · parser timeout 30 s per script · LSP startup 30 s ·
validation budget 2 min per iteration · total workflow budget 15 min ·
diagnostics per evidence bounded by the existing per-run limits. The
provider cannot raise any of them.

## Consequences

- Solaris's first complete GDScript programming cycle exists: inspect →
  propose → approve → checkpoint → apply → parser gate → fresh LSP →
  diagnostics → evidence → repair, with every mutation still explicitly
  approved and checkpointed.
- The workflow is deliberately not a black-box `implement_feature` tool:
  the provider reads, researches, proposes, and evaluates evidence through
  the normal transparent tool loop.
- The next narrow milestone is GDScript development quality gates and
  independent review: style/convention checks, targeted testing,
  architecture review, changed-file regression analysis, and optional
  independent reviewer subagents before development tasks are declared
  complete.
- Runtime/game execution, scene/resource editing, DAP, visual QA, and
  editor integration remain explicitly deferred; the architecture check
  now enforces the workflow orchestrator's Node-infrastructure isolation
  (no fs, sockets, processes, or path handling in the orchestrator or the
  change-set executor).
