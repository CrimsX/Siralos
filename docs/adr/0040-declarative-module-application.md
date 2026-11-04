---
id: ADR-0040
status: accepted
domains: [architecture, product, configuration, security, extensions]
paths:
  - README.md
  - ROADMAP.md
  - SECURITY.md
  - CHANGELOG.md
  - crates/siralos-adapters/src/module/manifest.rs
  - crates/siralos-adapters/src/profile_config.rs
  - crates/siralos-cli/src/interactive/modules.rs
  - crates/siralos-cli/src/interactive/config.rs
  - crates/siralos-cli/src/interactive/session.rs
  - crates/siralos-cli/src/interactive/stdio.rs
  - crates/siralos-cli/src/interactive/tui_loop.rs
  - crates/siralos-cli/src/session_worker.rs
  - crates/siralos-core/src/composition.rs
supersedes: []
---

# ADR 0040 — Declarative Module Application and Rebuild

- Status: accepted (owner-directed task, 2026-10-20)
- Date: 2026-10-20
- Related: ADR 0037 (module composition model), ADR 0036 (lean product,
  composition, extension model), ADR 0038 (default tools and sandbox
  posture), ADR 0033 (differential harness), ADR 0032 (Rust migration)

This ADR replaces the imperative module-registration workflow with a
declarative desired-state workflow. It amends ADR 0037's CLI and profile
selection details; the module package model, the domain-neutral host
boundary, and every ADR 0036 guardrail remain in force. No remote
acquisition, marketplace, automatic lock writing, or module self-granted
authority is introduced.

## 1. Context

The product exposed `/modules-add`, `/modules-enable`, and
`/modules-activate`. The first mutated `siralos.toml`; the second and third
reconciled in-memory lifecycle state and consulted `[profile] modules` as a
narrowing filter. The result had two sources of truth for the desired module
set, an imperative workflow that could drift from the file, and a profile
filter that no longer matched the declarative direction.

The lifecycle primitives already exist and remain correct:
`install`, `enable`, `activate`, `deactivate`, and `uninstall` are Host
operations over exact package identity. What was missing was a single
declarative input and an apply pass that reconciles it safely.

## 2. Decision

### 2.1 One desired-state table

Every `[modules.<id>]` entry in workspace `siralos.toml` is a desired module:

```toml
[modules.godot]
path = "modules/godot"
# digest is optional
```

- `path` is required and is workspace-relative.
- `digest`, when present, is an independent pin spelled `sha256:<64 hex>`.
  It must match the module manifest.
- `digest` may be omitted. The manifest digest is resolved at apply time and
  the manifest and any named component are still verified against it.
- The manifest id must equal the declaration id.
- A declaration is not a trust grant. It names a package; lifecycle,
  capability, protected-path, and sandbox gates still run.

Removing an entry and rebuilding is the removal operation. Deleting a
module's files without removing the declaration is not a supported state and
reports truthfully.

### 2.2 Startup and `/rebuild`

The shared `compose_session` path applies declarations at startup for every
frontend and for headless/evaluation composition. `/rebuild` re-reads the
declarations and reconciles the live session.

- Startup reports failures as existing startup notices and continues.
- `/rebuild` applies module declarations only. It never re-reads or applies
  `[profile]`, never edits `siralos.toml`, and never writes `siralos.lock`.
- `/modules` remains a read-only declaration list. Live lifecycle status in
  the TUI is deferred.

The product command catalog adds `/rebuild` and removes the three imperative
commands. The frozen `siralos-core::commands` replay vocabulary and
differential corpus are untouched.

### 2.3 Apply and failure isolation

Resolution happens before any lifecycle mutation:

1. Read and shape-validate every declaration. A malformed whole document
   aborts the pass without changing any module. An individual invalid entry
   is reported and skipped while valid entries continue.
2. Resolve each valid declaration's manifest and optional component digest.
   A failed resolution blocks that id: its previous working state is retained
   rather than removed.
3. Reconcile removals for ids no longer declared.
4. Apply additions, changes, and unchanged entries in id order.

A changed module is staged before the old slot is touched:

- a component host is installed, enabled, and activated;
- a spec-only module is installed and enabled;
- only after staging succeeds is the old host deactivated and uninstalled;
- if the old teardown fails, the staged replacement is dropped and the old
  state is restored, with a truthful failure.

A new component whose activation fails is not inserted; the failure is
reported and the next rebuild retries. An unchanged active module is not
re-activated.

Kind changes (`hostless` spec-only to `hosts` component and back) use the same
stage-then-swap path. A module that stops resolving does not lose its previous
working state merely because an intermediate edit broke the declaration.

### 2.4 Profile approval

`profileApproval` now binds the canonical `[profile]` semantics, not the
whole-file bytes. Adding, removing, or editing `[modules.<id>]` entries must
not invalidate an approved profile.

- The digest covers the parsed profile: name, overlay, context, skills,
  provider/model/credential/endpoint/protocol, replay flags, and
  context-system enablement.
- It excludes `[modules.<id>]` because those are applied by `/rebuild`, not
  by profile composition.
- It excludes the deprecated `[profile] modules` field, which is parsed for
  compatibility but ignored by the product.
- The legacy whole-file digest continues to be accepted while the file bytes
  remain identical, so an existing approval keeps working until the first
  module edit.
- `siralos --profile-digest <dir>` prints the canonical digest for a
  workspace. It is a read-only parser call: no session, provider, module
  apply, or mutation.

`/reload` still revalidates the trusted configuration and refuses a changed
dangerous profile. Editing a module declaration does not revoke live
authority because it is not a profile-authority change.

### 2.5 Deprecated `[profile] modules`

The field is accepted so old files do not become syntax errors, but it has no
effect. When an applied profile contains it, startup prints one notice:
`[profile] modules is ignored; declare modules under [modules.<id>]`. It is
also excluded from non-live authority digests: editing it cannot revoke a
session.

## 3. Security boundary

Declarative application adds no authority.

- Startup activation executes a declared component's guest `bind` with an
  empty `HostAuthority`. A module that requests a capability cannot obtain it
  from a declaration; activation fails and is reported.
- The guest remains bounded by the existing fuel, input/output, host-call,
  and component-byte limits, and by the protected-path and workspace
  containment checks.
- Module manifests and component bytes are verified against the resolved
  digest before install and again at activation.
- `siralos.toml` and `siralos.lock` remain model-protected. Declarations are
  Host-controlled input.
- Existing lock files are verified only. This ADR does not create or update
  `siralos.lock`.
- No new network, subprocess, or filesystem capability is granted by the
  apply pass. `/rebuild` mutates only in-memory module lifecycle state.

## 4. Consequences

- The desired set is inspectable in one file and the workflow is
  edit/rebuild, matching the product's declarative direction.
- A module failure is local to that module; the session still starts and
  other modules apply.
- Existing approvals need one-time regeneration after the first module edit
  if they used the legacy whole-file digest.
- The in-memory registry is not persisted separately. It is reconstructed on
  every session and rebuilt on demand.
- Live module lifecycle status in `/modules` remains future work; the view
  is intentionally honest about declarations rather than guessing runtime
  state.

## 5. Non-goals

- Remote module acquisition, marketplaces, or automatic downloads.
- Automatic `siralos.lock` generation or mutation.
- Module-provided authority, hooks, task graphs, or multi-agent machinery.
- Applying profile changes through `/rebuild`.
- Changing the frozen core command catalog or differential corpus.

## 6. Verification

The implementation is covered by:

- adapters tests for omitted digests, per-entry failure isolation, manifest
  resolution, declared digest mismatch, and component verification;
- CLI tests for startup application, `/rebuild`, removal, broken-declaration
  continuation, component activation, spec-only enablement, kind changes,
  failed-update retention, and command removal;
- profile tests for approval survival across module edits, canonical-digest
  equality with the approval gate, legacy raw-digest acceptance, and the
  deprecated `[profile] modules` notice;
- headless tests proving startup application runs through `compose_session`
  and a broken declaration does not block the turn;
- `cargo fmt --all --check`, clippy with warnings denied, workspace tests,
  `npm run check:differential` in pinned mode, and `npm run check`.
