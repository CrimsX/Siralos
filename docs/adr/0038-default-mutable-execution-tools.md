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
as a NAMED FOLLOW-UP MILESTONE and is explicitly not built here:
the Rust adapter never resolves or spawns on non-Unix targets, so
`bash` there is typed `sandbox_unavailable` by construction.

Phase C wires the Unix side (corpus v88). The production session
prefers `SrtEnforcingBackend` over the pinned Sandbox Runtime
(`0.0.70`) where its live canary passes and takes the honest host
refusal otherwise: `PATH` resolution, a `package.json` version gate
on the pinned distribution (never `--version`, which the CLI
reports unreliably), one generated per-execution settings file per
run (offline network, deny-root-re-allow grants), argv-mode
invocation, closed stdin, allowlist environment, bounded streams,
timeout/cancel kill of the supervisor child, and exit passthrough
with CLI-marker infrastructure mapping. The canary re-proves
execution, exit passthrough, inside-grant writes, outside-grant
denial, and grandchild reaping on every probe; anything less is
unavailability with its reason. The corpus pins the host refusal
(`bash-unavailable`, unchanged) plus the runner mapping through a
deterministic stub backend (four `bash-executes` subjects).

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
flips the checkpoint-creation surface with it — and the restore
surface is wired as the user `/undo` family (`/undo`, `/undo <id>`,
`/undo --last`): applied `write`/`edit` records with verified
preimages restore byte-exactly behind an explicit confirmation,
while prepared/uncertain/conflicted records, foreign tools,
protected targets, and escaping targets refuse typed (adversarial
suite B4). The model gets no undo tool. Unverifiable or unexpected
content blocks capacity checks (checkpoint creation) and is never
repaired or deleted automatically, before and after this change.
Restore narrowing: only the addressed record's own metadata or
preimage poison blocks its restore; unrelated stray store content
never disables undo of a valid record (the poison is left intact,
never cleaned up).

## 5. `bash` profile mapping

`bash` runs through the pinned SandboxBackend (core-owned port,
adapter-only imports, normalization into `SandboxError` codes) under
the `develop-offline` / `validation-offline` profiles: minimal
environment from an explicit allowlist (`process.env` never forwarded
verbatim, `SRT_DEBUG` never forwarded), network denied
(`allowedDomains: []`, `deniedDomains: ["*"]`, `strictAllowlist`),
closed stdin, per-execution settings file (deny-root-re-allow
filesystem grants around the workspace; the file is private and
removed after the run), process-tree confinement (grandchildren die
with the tree — the supervisor child is killed on timeout/cancel
and the runtime owns the tree, re-proven by the canary),
wall-clock timeout kill, bounded stdout/stderr with explicit
truncation flags, cancellation mid-run,
and ALL output through the terminal sanitizer at the boundary (no
ANSI/OSC injection survives). No `unsafe`: the workspace forbids
`unsafe_code`, so no process-group kill exists — tree confinement
rests on the runtime plus the live rows, never on a claim.

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
file deletion (D5), which stays typed `unavailable`, plus the
previously closed safe undo: auto-checkpoint plus the tested
`apply_undo` primitive is now wired as the user `/undo` family
(B4 adversarial suite; the AGENTS.md deviation is retired by
this flip).

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

Undo evidence row (flipped by the `/undo` family). `apply_undo`
(`crates/siralos-adapters/src/workspace/checkpoint_lifecycle.rs`)
is implemented and tested: hash-gated restore, `Conflict` never
overwrites newer work, undo-of-create stays `DeleteUnavailable`
(D5). The user surface (`/undo` lists applicable checkpoints,
`/undo <id>` and `/undo --last` restore behind an explicit
confirmation) is covered by twelve cli adversarial tests:
byte-exact restore with `Undone` marking, double-restore refusal,
stale refusal without overwriting, unconfirmed refusal without
touching the store, unknown-id refusal, list scope (prepared,
foreign-tool, and create records excluded) with newest-wins
`--last`, undo-of-create refusal without deleting, protected
refusal, tampered-preimage refusal, poisoned-record refusal
without repair, unrelated-poison non-blocking with the poison
left intact, and traversal refusal. Narrowed restore semantics:
a restore is blocked only by its addressed record's own
unverifiable metadata or preimage; stray content elsewhere in
the store never blocks a valid record's restore and is never
repaired, deleted, or quarantined. The corpus pins the same
outcomes (`undo.*` subjects, v87).

`bash` adversarial inventory (2026-10-14, working tree at
`9a9f47e`; Phase C wired 2026-10-16). The core port (`siralos-core::runtime::sandbox`:
backend trait, typed reasons, availability report, bounded
command/limits/outcome types) launches nothing; the honest probe
adapter (`process::sandbox_backend`: honest probe, allowlist builder,
truncation flags) never spawns; the enforcing adapter
(`process::enforcing_backend` over the pinned `srt` CLI) spawns
only after its live canary proves enforcement on that machine
right now; the tool (`tool::bash_tool`) probes before running and
reports typed `SandboxUnavailable` wherever enforcement is
unproven. Phase C rows, each green on the proving host:
malformed and oversize inputs invalid before any probe; approval
preview binds the exact command bytes; approval for one command
never authorizes another (end-to-end mismatch denial, recorded
approval unconsumed); an approved call still refuses without
enforcement; cancelled calls refuse before any probe; the
credential-contract deny names hold (suffix, prefix, and exact);
the builder keeps allowlist names, drops secrets (and
`SRT_DEBUG`), applies controlled overrides, and lets fixed values
win; truncation flags every cut; `execute` without proven
enforcement spawns nothing (the would-be-spawned marker file stays
absent — asserted on Windows too); the version gate reads
`package.json`, never `--version`; settings deny all network and
deny-root-re-allow the workdir, and refuse relative workdirs;
CLI-marker failures map to infrastructure faults while child
statuses pass through; stub-supervisor rows prove argv shaping,
exit passthrough, settings cleanup, timeout kill, and cancel kill
mechanically; outcome mapping (timeout wins,
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

Phase C live rows (2026-10-16, `SrtEnforcingBackend`, opt-in Unix
tests behind `SIRALOS_PROVE_SRT=1`, corpus v88/350 scenarios).
Seven rows run only where the pinned `srt` is installed and the
variable is set, and FAIL when proof is requested but the probe
refuses: grandchild reaping (background sleeper marker must never
land), timeout kill, cancel kill, loopback denial against a live
listener (tooling-gated: UNKNOWN without node in the sandbox,
never a settings-only pass), allowlist environment (`TERM=dumb`
wins live), closed stdin (`wc -c` reads zero), and workdir
confinement (inside-grant write lands, outside-grant write fails
with no host effects). The `bash-executes` corpus subjects pin the
same runner mapping deterministically through the stub backend
(success, nonzero exit, timeout, cancellation). Platform rows,
stated as unknown where unobserved: this working tree is Windows,
so every live row is `unknown` here (the Windows rows assert the
refusal path and are green); the rows turn green only on a Unix
host with the pinned runtime installed. Wiring CI to install
`srt@0.0.70` and set `SIRALOS_PROVE_SRT=1` on the Linux gate and
macOS platform jobs is a pending owner decision with
supply-chain review — until then the live claim rests on the
mechanism plus the stub rows, and the probe's `Available` report
is falsifiable by the first failing live row (never silently
kept).
