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
placement = "cell:0,1"             # "tab" | "cell:<0|1>,<0|1>" | "status"
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

`placement = "status"` (owner-directed, 2026-10-20): the third
placement in the same closed grammar. A status-placed spec shares
no occupancy with `tab`/`cell` slots; the registry's
replace-by-id semantics are unchanged, so a manifest that wants
the same feed both as a panel and as a strip line declares two
ids — one per form. Status-placed specs never render in the
panels view: they project instead as strip slot lines composed by
the Host (§6).

## 2. Feeds map to existing authority classes — no new authority

Starter set, kept this small. Each feed reuses an existing
read-only projection; params go through the same bounded,
path-contained parsing as the tools; every rendered string
passes the terminal sanitizer (§6).

| feed               | source (reuse, not new code)                                                                                                                           | capability gate                 |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------- |
| `workspace.list`   | `adapters::workspace::list` (sorted, bounded)                                                                                                          | `workspace-read`                |
| `workspace.search` | existing search adapter                                                                                                                                | `workspace-read`                |
| `context.summary`  | `LastProjection` bytes/pressure/counts only — no raw content                                                                                           | none (already-visible state)    |
| `tools.catalog`    | registry definitions snapshot (name/description/status)                                                                                                | none (already provider-visible) |
| `modules.status`   | install records (`format_modules` input)                                                                                                               | none (local user config)        |
| `checkpoints.list` | `CheckpointStore::list` metadata only: id, path, created-at, state, byte counts — preimage bytes are structurally excluded, pinned by adversarial test | `workspace-read`                |
| `time.window`      | pure clock arithmetic (`adapters::ui_feeds::now_utc_millis`), no host I/O                                                                              | none (no authority needed)      |

**Amendment (owner-directed, 2026-10-19):** `time.window` joins the
closed enum as the first genuinely moment-bearing feed. It renders a
wall-clock window schedule — current time with a `peak`/`off-peak`
verdict, the parsed schedule, and a countdown to the next window
boundary — from declarative params only: `windows` (req, ≤8,
`HH:MM-HH:MM` ranges, no midnight wrap, no overlap), `days` (`all`, a
weekday name, or a wrap range), `holidays` (≤64 civil dates), `label`
(≤256 chars). It computes in UTC with `None` when no clock is
available (honest `now: no wall clock` lines), scans at most 400 days
ahead (`switch: none within 400 days` otherwise), and requires no
capability: pure deterministic arithmetic on injected inputs, so a
module declares it with `capabilities = []`. It was added to serve
the first third-party module (`deepseek-hours`), a pure-data manifest
contributing one panel. Because the enum is closed (§1), the feed is
product code: the module stays a user artifact, never committed.
Panels-view entries now refresh every time (no `is_none` guard) so a
staged snapshot cannot show an aged verdict; the corpus is re-pinned
at v91 with four `time-window.*` render scenarios (peak schedule,
day-horizon countdown, holiday off-peak, no-wall-clock honesty) as
candidate-authored expectation records, with the freezer untouched.

Unknown feed → typed refusal. A module spec naming a feed
outside the module's authority is refused at projection: the
feed's required capability must be in the module's requested set
(ceiling-checked at install) and, when the module is Active, in
its effective grant. Capability ids here are the module-namespace
dash form (`workspace-read`, matching manifest and grant
vocabularies), not the dotted tool capability ids. Feeds with no
gate are open to every spec.

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

**Status strip (owner-directed, 2026-10-20).** The main
transcript view gains an always-on bottom strip row rendering
every enabled `placement = "status"` component. The row is
host-composed from the projection's `status_strip` slots —
worker-staged panel snapshots, not live projection reads — with
three mechanical properties:

- **Composition is injective text.** Each slot renders
  `<title>: <feed lines joined by " · ">`, slots compose in
  registration order, disabled and content-empty slots are
  skipped, and the strip renders nothing at all (zero-height row,
  byte-identical idle frames) when no slot resolves — so panels
  view pins and existing transcript pins do not move.
- **Refreshing and aging are host-owned.** Staging a snapshot
  stamps the staging time; a bounded 60 s tick re-requests the
  panel report only while the strip is visible and no turn is
  busy. Once a staged snapshot is ≥75 s old the strip truncates
  its content to reserve marker room and appends `staged {N}m
ago` in a dimmed accent instead of presenting stale data as
  current. The model never controls strip refresh; there is no
  toggle.

## 7. Corpus amendment plan (v85 → v86)

Executed at v85 (the ui-components subjects), at v91 with the
`time.window` amendment (§2), and at v92 with the status strip
(owner-directed, 2026-10-20): two `time-window.*` strip scenarios
(fresh and stale-marker) and the refreshed `spec-bad-placement`
refusal message joined as candidate-authored expectation records
— new subjects, manifest regenerated, `CORPUS_VERSION` moved
together in the contract and the runner — plus the v92 re-stamp
of both evidence lists and the negative-guard fixtures that carry
version stamps (the deliberate wrong-version and
stale-digest fixtures untouched). The older mechanics are
unchanged: candidate-authored records with printed audit
disclosure, `oracle.json` byte-identical
(`sha256 9f5f7861903b…`), addition-only edits (no pinned record
echoes manifest keys or the slash catalog), and any deviation
retires + adds per the once-only wall, never silently edited.

## 8. Lean guardrails (unchanged)

Declarative composition only. This ADR adds no hooks, no
marketplace, no automatic acquisition, no task-graph machinery,
and no multi-agent foundation. Components are host-rendered
data views, not agents. Core works with zero components; the
empty host is a first-class state.
