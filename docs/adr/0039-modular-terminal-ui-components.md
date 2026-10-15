---
id: ADR-0039
status: accepted
domains: [architecture, product, security, extensions]
paths:
  - README.md
  - ROADMAP.md
  - ARCHITECTURE.md
  - SECURITY.md
  - docs/development/PROJECT_CONTEXT.md
  - docs/development/ADVERSARIAL_TESTING.md
  - docs/development/STRUCTURED_INPUT_INVENTORY.md
  - docs/requirements/REQUIREMENTS.md
  - CHANGELOG.md
  - docs/wayfinder/siralos-roadmap.md
supersedes: []
---

# ADR 0039 — Modular Terminal UI Components

- Status: accepted (owner-directed task, composition-layer plan)
- Date: 2026-10-15
- Related: ADR 0036 (lean product, composition, extension model),
  ADR 0037 (module composition model), ADR 0038 (default tools),
  ADR 0033 (differential harness), ADR 0032 (Rust migration),
  SECURITY.md (security contract)

This ADR authorizes a declarative UI component system for the
terminal frontend: users compose panels from within Siralos, and
installed modules may contribute panel specs. A component is
**data, never code** — specs declare what to show, the Host owns
all rendering. It changes no frozen record by itself; every
observable behavior change lands through the digest-bound corpus
amendment mechanics with printed disclosure. The bar is absolute:
a capability becomes available only when its security property is
mechanically enforceable AND covered by adversarial tests.

It supersedes nothing in ADR 0036 except one addition: UI
components join its composition classes as the concrete form of
Views (user-authored specs plus module-declared specs, rendered
by the Host).

## 1. Component spec schema

One shape in two homes: workspace `.siralos/ui.toml` (user specs)
and `module-manifest.toml` `[ui]` entries (module specs). TOML
array-of-tables in both:

```toml
[[components]]
id = "recent-files"                # 1..=64, [a-z0-9_-]
title = "Recent files"             # 1..=128 chars, sanitized at render
feed = "workspace.list"            # closed enum, §2
enabled = true                     # default true (§5: the enable switch)
placement = "cell:0,1"             # "tab" | "cell:<0|1>,<0|1>"
max_rows = 20                      # 1..=99 size hints
max_cols = 60                      # 1..=99

[components.params]                # ≤8 entries; keys ≤64 chars
path = "src"                       # string values ≤1KiB each
```

Bounds (shared by both homes): file ≤64KiB; ≤32 user components;
≤8 module specs per manifest. Parsing is all-or-nothing: one
malformed spec refuses the whole document with a typed error,
never a partial application. Specs detach from caller-owned data
at parse (owned `String`s, no borrows into input buffers).

`enabled` is the user enable switch for module-declared
components: writing a same-id user spec (the §5 override) with
`enabled = true` enables it, `enabled = false` disables it.
User-authored specs default to enabled.

## 2. Feeds map to existing authority classes — no new authority

Starter set, kept this small. Each feed reuses an existing
read-only projection; params go through the same bounded,
path-contained parsing as the tools; every rendered string
passes the terminal sanitizer (§6).

| feed               | source (reuse, not new code)                                                                                                                           | capability gate                 |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------- |
| `workspace.list`   | `adapters::workspace::list` (sorted, bounded)                                                                                                          | `workspace.read`                |
| `workspace.search` | existing search adapter                                                                                                                                | `workspace.read`                |
| `context.summary`  | `LastProjection` bytes/pressure/counts only — no raw content                                                                                           | none (already-visible state)    |
| `tools.catalog`    | registry definitions snapshot (name/description/status)                                                                                                | none (already provider-visible) |
| `modules.status`   | install records (`format_modules` input)                                                                                                               | none (local user config)        |
| `checkpoints.list` | `CheckpointStore::list` metadata only: id, path, created-at, state, byte counts — preimage bytes are structurally excluded, pinned by adversarial test | `workspace.read`                |

Unknown feed → typed refusal. A module spec naming a feed
outside the module's authority is refused at projection: the
feed's required capability must be in the module's requested set
(ceiling-checked at install) and, when the module is Active, in
its effective grant. Feeds with no gate are open to every spec.

## 3. Storage and protection

Workspace specs live in workspace `.siralos/ui.toml` — inside
the protected `.siralos/**`, so the model cannot mutate them
through workspace capability (existing classifier; a dedicated
refusal test pins `.siralos/ui.toml` exactly). The HOST writes
the file via the safe observed-byte + atomic-writer path (the
`profile.rs` write pattern: fresh token, consume-once, bounded
re-read with identity match, `stage_atomic` with the mutation
temp prefix, staged-byte verification through the spec parser,
`commit_if_digest`/`commit_if_absent`). Round-trip is
byte-stable: parse → serialize → parse is a fixed point
(`toml_edit` merge, never a `toml::Table` rewrite).

No persistence beyond `.siralos/ui.toml` plus the manifest
field: module-declared specs are re-parsed from manifests each
session, never cached. `schemas/user-config.schema.json` and the
`siralos.toml` section set are untouched (ADR 0037 §4).

## 4. Authoring surface

`/ui-list`, `/ui-add`, `/ui-edit`, `/ui-remove`, `/ui-preview`
join the interactive vocabulary (`commands.rs`; catalog pins
updated). Add/edit drive `UiSpecForm` (`tui/forms.rs`
machinery: bounded fields, closed feed/placement enums,
per-field validators, sanitized error text). `/ui-preview`
renders one spec to sanitized lines without saving. Edit
addresses specs by id; ambiguous or missing ids refuse typed.
Mutating commands cross the worker boundary like
`/modules-add` (session-owned state); list/preview render
locally. All output through the terminal sanitizer.

## 5. Module contributions and lifecycle

`module-manifest.toml` gains an optional `[ui]` section: an
array of component specs in the §1 shape, parsed with the same
validators and bounds as user specs. Module input is hostile by
the repo's rules: oversized, malformed, or unknown-key `[ui]`
sections are typed-refused; unknown top-level keys stay ignored
(existing tolerance test keeps passing).

Lifecycle discipline carries over verbatim:

- Specs arrive DECLARED at install (parsed from the manifest,
  bounded, validated).
- A module's specs render only while the module is Enabled or
  Active — install alone contributes nothing
  (`install != enable != activate`).
- User specs override module specs on id conflict (fields,
  placement, and `enabled`).
- Uninstall removes the manifest, so the declared set shrinks
  with no separate cleanup step.

## 6. Rendering seam and sanitizer rules

`UiComponentProjector` mirrors `ToolProjector`: a registry plus
one projection code path, owned by `siralos-core` (`ui`
module; ownership index entry). It projects registered specs +
resolved sanitized feed content into ordered slots; adapters
resolve feeds; the cli TUI host draws. Rendering is entirely
host-owned — modules contribute specs only, never render hooks
(ADR 0036 lean guardrails stand: no hooks machinery, no
marketplace, no automatic acquisition).

The panel host is a separate explicit TUI view: a tab bar (one
tab per `tab`-placed component in id order, plus a fixed
`grid` tab) over a content area (selected tab full-bleed, or
the 2×2 split with `cell`-placed components in row/col order).
Empty cells and the zero-component state render sane
placeholders. The transcript `draw` path is untouched while the
panels view is inactive, so existing `tui-render.*` pins do not
move. Panel rendering is deterministic: byte-equal buffers for
identical state at the fixed harness viewport.

Sanitizer rules: every feed string, title, id echo, and error
text passes `sanitize_for_display` before it reaches a buffer;
a component cannot render content outside the sanitizer
(adversarial-pinned); truncation markers are host text, never
feed text.

## 7. Corpus amendment plan (v85 → v86)

New `ui-components.*` subjects pinning the new behavior in
detail (spec parse/validate, authoring round-trip, render pins
for the starter components incl. the empty host, module
precedence, refusal cases) with candidate-authored expectations;
manifest regenerated; `CORPUS_VERSION` and the strict-loader
count assert moved together; negative-guard fixtures extended
or re-stamped; every amendment printed in the audit, never
silently edited. `oracle.json` stays byte-identical
(`sha256 9f5f7861903b…`). Addition-only is expected (no pinned
record echoes manifest keys or the slash catalog); any
deviation retires + adds per the once-only wall, never edits.

## 8. Lean guardrails (unchanged)

Declarative composition only. This ADR adds no hooks, no
marketplace, no automatic acquisition, no task-graph machinery,
and no multi-agent foundation. Components are host-rendered
data views, not agents. Core works with zero components; the
empty host is a first-class state.
