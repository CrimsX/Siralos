---
id: ADR-0013
status: accepted
domains: [godot, quality, review]
paths: [packages/core/src/godot/**]
supersedes: []
---
# ADR 0013: GDScript development quality gates and independent review

Status: accepted

## Context

The GDScript development workflow (ADR 0012) can turn a request into an
approved, checkpointed, validated source change and repair validation
errors. But it reports clean completion when the GDScript parses and the
LSP reports no errors. A change that is syntactically valid can still be
incorrectly scoped, inconsistent with project conventions, affected by
blocking Godot diagnostics that are not parser errors, missing applicable
validation, or regressive in ways no parser can see.

This milestone closes the initial GDScript-development stage by adding a
quality stage between validation and completion: deterministic quality
gates (measurable conditions the application itself computes) plus one
dedicated model-based independent reviewer (an additional reasoning
signal, never a replacement for any deterministic gate). A development
task can no longer report clean completion merely because it parses.

## Decision

### Deterministic gates and model review are separate layers

Deterministic checks are authoritative for measurable conditions: the
approved change was applied exactly, a checkpoint was recorded, no
unexpected workspace change occurred, changed GDScript parses, no
error-severity LSP diagnostics exist in changed files, required configured
validation completed, and the reviewer found no evidence-backed
Critical/High issues. The reviewer is a bounded reasoning signal over the
final diff that can never replace parser checks, LSP diagnostics, hash
verification, source-integrity checks, sandbox enforcement, or test exit
codes — and the deterministic gates can never detect every behavioral or
architectural regression. Both layers are required.

Gates are classified hard (failure prevents clean completion), soft
(generate advisories), or informational (evidence only). The
classification is fixed by the application; it is not provider-configurable.

The hard gates are: `approved-change-applied`, `checkpoint-recorded`,
`scope-verified`, `parser`, `lsp-errors`, `required-validation`, and
`independent-review`. The soft gates are `warnings` and `conventions`;
`diff-metrics` is informational. A required gate that could not run
because infrastructure was unavailable — or a validation command the user
denied — produces `validation_incomplete`, never `passed`; required gates
are never silently skipped.

Final quality states map into the workflow vocabulary: `passed` →
completed (ready), `passed_with_advisories` → completed with warnings
(ready with advisories), `blocking_findings` →
`completed_with_blocking_findings`, `validation_incomplete` and
`quality_gate_failed` → the truthful terminal states, `cancelled` →
cancelled.

### Warning policy respects actual Godot/project behavior

Solaris does not invent "all warnings are errors". A warning that Godot or
the project surfaces as an error is a hard error. A normal Godot warning
is advisory unless project configuration promotes it to error or
repository guidance explicitly requires it to block. Before applying a
change, bounded diagnostics for the relevant files are captured from the
pre-edit language session where practical; afterwards the before/after
diagnostics are compared with stable normalized identities (never raw
message ordering), with conservative line-movement tolerance. Entries are
classified `introduced`, `resolved`, `unchanged`, or `uncertain`; a newly
introduced warning is advisory by default, a newly introduced error
blocks, and an unavailable baseline makes attribution uncertain rather
than falsely attributed. Solaris never inserts `@warning_ignore`
annotations and never modifies project warning configuration.

### Project conventions take precedence over fallback style guidance

The convention analyzer is a small read-only analyzer for high-confidence
issues in new or modified lines only (trailing whitespace, mixed
indentation in newly added blocks, very long newly introduced lines,
multiple statements on one line, obvious naming-convention drift, obvious
indentation-width mismatch with the file, typed-file signature drift). It
is not a full GDScript linter and does no complex semantic parsing.
Priority: explicit repository guidance (only deterministic rules marked
repository-mandatory may block), then the existing local file/module
convention inferred from the file itself, then project-wide conventions,
then Godot style recommendations as fallback. Findings are advisory unless
explicitly mandatory; existing unrelated project style is never rewritten.

Static typing is never required globally: a predominantly typed file's new
public APIs should normally preserve that style (advisory), a
predominantly dynamic file is never forced to add annotations, and
mixed/uncertain files produce no style finding. Project-wide
dynamic-to-static refactors are out of scope.

### Validation-plan selection

The mandatory validation steps are always included and controlled by the
application: changed GDScript `--check-only`, changed-file LSP
diagnostics, workspace/source integrity, Git/change-set scope
verification, and the independent review. The provider never has to
remember to request them. Project-defined commands are discovered only
from existing npm scripts named `check`, `test`, `lint`, or `typecheck`
(never invented names; `install`/`ci`/`npx`/`exec` are never candidates),
preferring `check` when it clearly aggregates validation and avoiding
redundant scripts when `check` already includes them. No packages are
installed and no general build graph exists. Absence of a test runner is
`not_applicable`, never an infrastructure failure.

### Project-defined commands still require one-time process approval

A project-defined validation command is untrusted executable project
content. The existing sandboxed process rules remain authoritative: the
exact command is shown with the exact repository script body and requires
its own exact one-time process approval; the workspace stays read-only,
network stays denied, and stdin stays closed. A development-workflow
approval is never a process approval. If the user denies an applicable
command, the result is `validation_incomplete` — never `passed` — the
source changes remain, and no automatic rollback occurs. The command
runners fail closed as `unavailable` at this stage exactly like every
other process surface, so the executor refuses before any approval and
the gate honestly reports `validation_incomplete`.

### One dedicated independent reviewer

One dedicated reviewer reviews the final diff with a FRESH provider
context: a new provider instance and a brand-new conversation — the
primary implementer's conversational history, hidden reasoning, and
continuation data are never reused or forwarded. The reviewer is strictly
read-only: its tool registry contains only `workspace.list`/`read`/`search`,
`git.status`/`git.diff`, `godot.inspect_project`, `godot.api_search`/
`godot.api_lookup`, and the read-only LSP query tools (`hover`,
`definition`, `lsp_diagnostics`; never `complete`) — no write tools, no
`process.run`, no approval controls, no checkpoint mutation, no undo. The
reviewer receives the user's original request, the complete final diff,
the changed-file list, validation evidence, engine version, and any
discovered repository guidance; never credentials, approval internals,
mirror host paths, or hidden primary-provider reasoning.

The review provider defaults to the active development provider profile
with a fresh context. A trusted user-level `quality.reviewProvider` may
reference an existing configured provider profile; there is no new
credential system, a missing profile fails clearly, and there is no silent
fallback to an unrelated provider. Reviewer output is a strict JSON
contract validated at runtime: findings are bounded (50), every field is
length-bounded, paths are normalized to safe workspace-relative form,
malformed output rejects the whole review, and reviewer output can never
register tools or execute actions. Review is cancellable (cancellation
stops the provider request, leaves source changes and evidence intact,
and yields `validation_incomplete`). Reviewer infrastructure failure
(timeout, missing provider, auth failure, malformed output) is
`validation_incomplete`, never a pass.

### Blocking versus advisory findings

Critical/High findings block clean completion only when backed by
sufficient concrete evidence (confidence high or medium); a low-confidence
Critical/High finding is normalized conservatively to advisory. Medium and
Low findings are always advisory. The reviewer cannot change this policy.
A disputed blocking finding stays in the final report with the
disagreement explained; no debate system is built. Findings get
deterministic local ids from safe normalized fields (category, path,
line, normalized title) for tracking and re-review comparison only — never
as security identifiers — and duplicates are deduplicated conservatively.

### Review repair cycle

For a confirmed blocking finding the primary provider may propose a
focused repair. Every repair is a normal change set: exact diff approval,
checkpoints, parser validation, fresh LSP restart, applicable test checks,
and then a FRESH holistic review of the complete final diff (previous
finding ids are passed only for traceability, never as instructions to
ignore other problems). The reviewer itself never edits. Review-repair
rounds are bounded separately (at most 2 beyond the existing iteration
budget); the provider cannot raise any budget. If blocking findings
remain, completion is `completed_with_blocking_findings`, never clean.
Denial of a repair preserves the existing approved change.

### Fail-closed at this stage

The quality stage lives inside the development workflow, whose
change-set applier fails closed as unavailable on every platform (no
directory-relative commit primitive). The stage, the reviewer plumbing,
the validation executor, and the report machinery are tested internal
code exercised through injected fakes; in the shipped product the
workflow refuses before any approval, no quality stage or review runs,
no validation command executes, and nothing is created or deleted. The
opt-in `npm run test:godot-quality` conformance verifies this
fail-closed truthfulness against a real enforcing sandbox when one is
available and always reports the live quality-stage isolation probe as
skipped, never passed.

### Limits

Maximum review findings 50 · evidence/impact/recommendation 4096 chars ·
title 256 chars · review-context diff 1 MiB (chunked by complete file
when larger; every changed file is covered and the aggregator is
read-only) · review rounds 3 total · review-repair rounds 2 · review
timeout 120 s · warning line tolerance 30 · long-line advisory 160 chars ·
convention findings 100 · warning-delta entries 200. The provider cannot
raise any of them.

## Consequences

- A `/develop` workflow automatically enters the quality stage before
  completion; the user never has to remember `/quality`.
- `/quality` shows the current/final quality report; `/review-change`
  runs a fresh read-only independent review of the current tracked change
  with no approval and no modification.
- Clean completion now requires: approved edits applied, checkpoints
  durable, changed scripts parse, LSP errors absent, project-required
  warning-as-error policy passes, required validation commands completed,
  scope verification passes, and an independent review with no blocking
  findings.
- Cancellation is truthful end to end: the quality stage is cancellable
  through the in-flight apply signal (the reviewer and the validation
  executor honor it), a cancelled review yields `cancelled`, and a
  terminal state is never resurrected by a racing stage.
- Scope verification is cumulative: a later change set (including review
  repairs) is never flagged as unrelated because of the workflow's own
  earlier approved change sets.
- Repository-guidance discovery is deferred: there is no instruction
  mechanism in Solaris yet, so `repositoryGuidance` is always null and
  no convention rule is repository-mandatory by default. The plumbing
  (the reviewer prompt injection and the analyzer's mandatory-rule
  promotion) is real and tested; wiring a bounded instruction mechanism
  is future work and is never presented as shipped.
- This is not the general multi-agent framework: exactly one reviewer,
  no voting, no consensus, no debate agents, no provider-to-provider
  conversations, no autonomous approval, no persistent review history.
- The next narrow milestone is Stage 3: read-only Godot scene and
  resource intelligence for `.tscn`/`.tres`, resource UIDs, scene
  inheritance, node relationships, and project settings, using structured
  semantic inspection before any scene/resource mutation is permitted.
