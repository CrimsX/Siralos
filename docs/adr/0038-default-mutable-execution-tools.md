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
mutation, which makes restore possible in principle and therefore
flips the checkpoint-creation surface with it — but no undo
surface is wired, so "safe undo" as a product capability stays
closed (the `apply_undo` primitive is implemented and tested with
no caller outside its own tests; see the deviation recorded in
AGENTS.md and ROADMAP.md). The checkpoint lifecycle —
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
workspace create/edit application; new checkpoint
creation and automatic checkpoint pruning. These stay exactly as
strict: `node-script` and `npm-script` command execution, Git
inspection, private run-directory creation or cleanup, Godot surfaces,
executable caches, and repository-reference materialization — plus
file deletion (D5), which stays typed `unavailable`, plus safe undo:
auto-checkpoint plus the tested `apply_undo` primitive with no wired
surface (deviation disclosed in AGENTS.md and ROADMAP.md rather than
claimed as a flip).

## 8. Lean guardrails (unchanged)

Declarative composition only. This ADR adds no marketplace, no
automatic acquisition, no hooks, no task-graph machinery, and no
multi-agent foundation. The four tools are host-executed operations
with typed schemas, not agents. Core works with zero modules; the
tools work with zero modules installed.

## 9. Implementation phasing (2026-10-13)

The campaign lands in phases; no phase flips the posture alone
(see ROADMAP.md §10 Remaining for the durable in-flight ledger).
`read` projection, this ADR, and the `write`/`edit` implementation
with their adversarial suites land first — while every live
`write`/`edit` call is still denied for want of the approval UI.
The live interactive approval prompt is in flight (owner-approved);
`bash`, the corpus v84→v85 amendment, and the AGENTS.md/SECURITY.md
posture flips stay closed until the digest-bound amendment lands
with its printed audit.

## 10. Adversarial evidence (2026-10-13, working tree at `9a9f47e`)

Before/after for the parent-substitution race fix. Pre-fix,
`rename_substitution_race_never_misdirects`
(`crates/siralos-adapters/src/tool/mutation_tools.rs`) FAILED with
`left: String("hot-parked/t-8.txt"), right: "hot/t-8.txt"`
(observed in the gate run 2026-10-10, round 2): a parent renamed
inside Windows `canonicalize` resolves the same directory object
under its NEW name, containment still passes, and the write lands
(and truthfully reports) a path the approval never bound. Post-fix
the resolution must still name the request
(`require_requested_parent_name`, the name half) and the commit
re-verifies the retained parent identity (the object half); the
race passes 25/25 runs and the full `npm run check` chain is exit 0. `ParentChanged` covers both halves: a same-object rename and a
same-name object replacement refuse with the same typed conflict.

Platform rows, stated as unknown where unobserved. The Windows
0/1000 substitution observation lives in `atomic.rs` (renaming a
directory that holds an open staged handle is refused with Access
denied, so mid-commit substitution cannot be staged
deterministically there). The symlink, socket, FIFO, and
rename-away deterministic cases are `cfg(unix)`-only; on non-Unix
targets the gates degrade to canonical-path identity, under which
a same-path object replacement is undetected. Unix-only rows are
named, never passed by skip.

Undo evidence row (disclosed, not flipped). `apply_undo`
(`crates/siralos-adapters/src/workspace/checkpoint_lifecycle.rs`)
is implemented and tested: hash-gated restore, `Conflict` never
overwrites newer work, undo-of-create stays `DeleteUnavailable`
(D5). It has no caller outside its own tests — no `/undo` command
exists in `siralos-cli` — so the undo surface stays closed and
§4/§7 claim no flip.

`bash` adversarial inventory (2026-10-14, working tree at
`9a9f47e`). The core port (`siralos-core::runtime::sandbox`:
backend trait, typed reasons, availability report, bounded
command/limits/outcome types) launches nothing; the adapter
(`process::sandbox_backend`: honest probe, allowlist builder,
truncation flags) never spawns; the tool (`tool::bash_tool`)
probes before running and reports typed `SandboxUnavailable`
wherever enforcement is unproven — today every platform, because
no enforcing backend is pinned. Rows, each green: malformed and
oversize inputs invalid before any probe; approval preview binds
the exact command bytes; approval for one command never
authorizes another (end-to-end mismatch denial, recorded approval
unconsumed); an approved call still refuses without enforcement;
cancelled calls refuse before any probe; the credential-contract
deny names hold (suffix, prefix, and exact); the builder keeps
allowlist names, drops secrets, applies controlled overrides, and
lets fixed values win; truncation flags every cut; `execute`
without proven enforcement spawns nothing (the would-be-spawned
marker file stays absent); outcome mapping (timeout wins,
nonzero fails, boundary cancellation wins); hostile child output
survives the terminal sanitizer as inert text only. Platform
rows: the refusal string itself is platform-stable by
construction (the corpus pins it byte-exactly on every host);
the Windows per-execution-grant/host-read gap lives in §2 and
the probe reason, asserted by the backend unit tests. Live
timeout-kill, grandchild reaping, and mid-run cancellation have
no live row anywhere by construction (no spawn exists to
observe); they are contract-level (limits/timeout/timeout-flag/
cancel-flag types and their mappings), and a backend milestone
must add live rows before claiming enforcement.
