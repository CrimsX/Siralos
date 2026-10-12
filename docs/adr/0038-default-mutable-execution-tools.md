---
id: ADR-0038
status: accepted
domains: [architecture, product, security, extensions]
paths:
  - README.md
  - ROADMAP.md
  - ARCHITECTURE.md
  - SECURITY.md
  - docs/development/PROJECT_CONTEXT.md
  - docs/development/ADVERSARIAL_TESTING.md
  - docs/requirements/REQUIREMENTS.md
supersedes: []
---

# ADR 0038 — Default Mutable and Execution Tools

- Status: accepted (owner directive 2026-10-10)
- Date: 2026-10-10
- Related: ADR 0036 (lean product, composition, extension model),
  ADR 0037 (module composition model), ADR 0033 (differential harness),
  ADR 0032 (Rust migration), SECURITY.md (security contract)

This ADR authorizes four DEFAULT tools — `read`, `write`, `edit`, `bash`
— making the pinned-unavailable mutation and execution surfaces
genuinely available behind the security contract's own gates. It changes
no frozen record by itself; every observable behavior change lands
through the digest-bound corpus amendment mechanics with printed
disclosure. The bar is absolute: a capability becomes available only
when its security property is mechanically enforceable AND covered by
adversarial tests.

## 1. The four tools

`read` presents the existing bounded workspace read surface as a
default tool. `write` and `edit` are host-side workspace mutation.
`bash` is general shell execution (arbitrary commands) under sandbox
enforcement. The existing read-only tools (workspace list/search,
context, and siblings) remain; the four above are the default
mutable/execution tools projected to the provider. There is no fifth
default tool: file deletion stays typed `unavailable` (D5).

Tool definitions are detached, canonical JSON owned by the host. A tool
definition must never alias caller-owned mutable data: the model
proposes structured inputs, the host validates, gates, executes, and
records. `ToolProjector` remains authoritative for model-visible tools;
the permission gate is unchanged.

## 2. Enforcement matrix per platform

Every tool is available ONLY where its enforcement is mechanically
present and a LIVE probe passes. A skipped probe is never a pass.

| Tool             | Gate that proves availability                                                                                                               | Windows today                                                                                                                    |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `read`           | In-process bounded read: containment, protected paths, byte bounds, whole-file identity                                                     | Works everywhere these gates hold                                                                                                |
| `write` / `edit` | In-process gates: containment, protected paths, atomic temp+verify+rename writers, stale-state checks, auto-checkpoint, per-action approval | Works everywhere these gates hold                                                                                                |
| `bash`           | SandboxBackend enforcement present AND live conformance probe passes (per-execution filesystem grants, host-read capability true)           | Typed `unavailable`: the pinned sandbox runtime cannot express per-execution filesystem grants and host-read capability is false |

Windows `bash` enforcement (restricted tokens/job objects or a newer
runtime, plus live probes and its own adversarial suite) is recorded
as a NAMED FOLLOW-UP MILESTONE and is explicitly not built here.

## 3. Approval model (D4)

Approvals are one-time and digest-bound PER ACTION, through a gate
INDEPENDENT of plan approval, sandbox enforcement, checkpointing, and
stale-state checks. Success at one gate never implies another. Plan
approval continues to grant none of this.

- `write`: bound to the exact bytes plus the task revision.
- `edit`: bound to the exact old→new strings plus the task revision.
- `bash`: bound to the exact command digest plus the task revision.

Mutating the bytes after approval, an ambiguous edit (0 or >1
matches), or a stale revision refuses before any effect.

## 4. Checkpoint and undo lifecycle (D3)

`write` and `edit` automatically checkpoint the target BEFORE
mutation, which makes "safe undo" real and therefore flips the
checkpoint-creation surface with it. The checkpoint lifecycle —
creation, byte-bound accounting, retention/pruning policy, byte-exact
restore wired from the pure undo planner — is implemented and tested
as part of this change. Unverifiable or unexpected content blocks
capacity checks and is never repaired or deleted automatically, before
and after this change.

## 5. `bash` profile mapping

`bash` runs through the pinned SandboxBackend (core-owned port,
adapter-only imports, normalization into `SandboxError` codes) under
the `develop-offline` / `validation-offline` profiles: minimal
environment from an explicit allowlist (`process.env` never forwarded
verbatim), network denied, closed stdin, process-tree confinement
(grandchildren die with the tree), wall-clock timeout kill, bounded
stdout/stderr with explicit truncation flags, cancellation mid-run,
and ALL output through the terminal sanitizer at the boundary (no
ANSI/OSC injection survives).

## 6. Corpus amendment plan (v84 → v85)

The pinned oracle asserts `unavailable` for these surfaces, so every
change is DELIBERATE under the established discipline:

- Supersession entries (reason + decision "owner directive 2026-10-10,
  ADR 0038" + both digests) where the scenario id keeps existing and
  its single supersession is unconsumed.
- Where a subject's single supersession is already consumed
  (once-only wall: `user-config-matrix`, `command-catalog-snapshot`,
  `cli-session-set`), RETIRE the old scenario id with a digest-bound
  `retirements.json` entry and ADD a new scenario id pinning the new
  behavior. Retired ids are never reused.
- New subjects pinning the new behavior in detail (`write`/`edit`/`bash`
  outcomes and their typed refusals, checkpoint/undo records) with
  expectations; manifest regenerated; `CORPUS_VERSION` and the
  strict-loader count assert moved together; negative-guard fixtures
  extended. Every superseded and retired record is printed in the
  audit, never silently edited. `oracle.json` stays byte-identical
  (`sha256 9f5f7861903b…`).

## 7. AGENTS.md posture flips

These fail-closed bullets flip, each backed by its adversarial suite:
workspace create/edit/delete application and safe undo; new checkpoint
creation and automatic checkpoint pruning. These stay exactly as
strict: `node-script` and `npm-script` command execution, Git
inspection, private run-directory creation or cleanup, Godot surfaces,
executable caches, and repository-reference materialization — plus
file deletion (D5), which stays typed `unavailable`.

## 8. Lean guardrails (unchanged)

Declarative composition only. This ADR adds no marketplace, no
automatic acquisition, no hooks, no task-graph machinery, and no
multi-agent foundation. The four tools are host-executed operations
with typed schemas, not agents. Core works with zero modules; the
tools work with zero modules installed.
