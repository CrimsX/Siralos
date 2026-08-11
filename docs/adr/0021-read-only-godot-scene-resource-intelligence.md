---
id: ADR-0021
status: accepted
domains: [godot, static-inspection]
paths: [packages/core/src/godot/**, packages/adapters/src/godot/**]
supersedes: []
---
# ADR 0021 — Read-Only Godot Scene and Resource Intelligence

- Status: accepted (Stage 3 milestone 8)
- Date: current milestone
- Related: ADR 0008 (Godot discovery and static project profiling),
  ADR 0015 (context/tool/evidence projection), ADR 0016 (workspace
  revision and structural reads), ADR 0020 (host-controlled planning)

## Context

Reliable Godot-native reasoning needs more than raw text reads. A `.tscn`
is not a plain-text file to Solaris: it carries node trees, owners,
instances, inherited scenes, signal connections, groups, scripts, and
UID identities whose meaning lives in relationships, not in individual
lines. Reading raw `.tscn` bodies into model context is token-expensive,
forces the model to re-derive structure every time, and invites
misreading — `parent` and `owner` are different relationships, `instance`
of the root node is inheritance, and `SubResource("1")` is file-local.

At the same time, mutation must not be introduced before semantics are
understood. Editing scene/resource text blindly corrupts UID identity,
ownership, and instance bookkeeping; the current change-set boundary
already refuses `.tscn`/`.tres` paths (ADR 0005). The fail-closed posture
(AGENTS.md) requires that Solaris never claims native scene/resource
editing before the mutation milestone exists, and never bypasses that
limitation through generic text edits.

The Stage 3 foundation already provides exact revision handles (ADR
0016), projection boundaries (ADR 0015), static project profiling (ADR
0008), and host-controlled planning (ADR 0020). Scene/resource
intelligence can now be introduced as derived, read-only, revision-bound
semantic state.

## Decision

Introduce a provider-neutral, static, read-only Godot scene/resource
intelligence layer:

1. **Bounded `.tscn`/`.tres` parser** — a small hand-written tokenizer +
   conservative Variant parser (no regex-only fake parser, no heavyweight
   dependency). Recognizes scene/resource headers (`format`,
   `load_steps`, `uid`), `ext_resource`, `sub_resource`, `node`,
   `connection`, `[editable]`, and ordinary property assignments;
   multiline arrays/dictionaries, comments, and escaped strings are
   handled by real scanning with explicit bounds (nodes, resources,
   connections, properties, nesting depth, raw length, diagnostics).

2. **Revision-aware semantic model** — every parsed document
   (`GodotTextDocument`) binds to the exact workspace revision it was
   parsed from (`rev_` handle). A changed file makes the old model
   historical evidence, never current project truth; caches (if any) key
   on workspace identity + path + exact revision + parser version.

3. **Honest malformed-input handling** — malformed Godot text produces
   structured diagnostics and `complete | partial | invalid` statuses,
   never fabricated structure. Unknown value syntax is preserved as
   bounded opaque/raw data. UID/path identity is retained where declared
   and never invented when absent.

4. **Distinct relationships** — node `parent` vs `owner`; scene
   inheritance (root-node `instance`) vs child PackedScene instances;
   `ext_resource` vs document-local `sub_resource`; serialized signal
   connections (existence) vs verified runtime behavior (never claimed).

5. **Small application-owned relationship index** — derived
   scene→script/scene→scene/resource→resource/project→main
   scene/project→autoload entries, each recording its source revision;
   stale entries are flagged, never presented as current. One subsystem
   (the scene intelligence service) owns current parsed state; the CLI,
   ContextProjector, planning, and review consume it.

6. **Read-only inspection tools** — `godot.inspect_scene`,
   `godot.inspect_resource`, `godot.dependencies` under the existing
   read-only `godot.inspect` capability, available to planner, developer,
   and reviewer per ToolProjector policy. No `godot.write_scene`,
   `godot.edit_resource`, or `godot.add_node` exists.

7. **Static-first, process-free inspection** — normal inspection requires
   no Godot process, no `@tool` execution, no plugin activation, no
   project loading, no import side effects. Recovery-mode/mirror rules
   remain authoritative for anything requiring Godot execution.

8. **Project relationship intelligence** — reuse of the static
   `project.godot` scanner for main scene (contained `res://` resolution,
   existence, revision), autoloads (target kind script/scene/resource,
   enabled state, never executed), and bounded input-action structure
   (action names, deadzone, event count/types).

9. **Planning integration** — verified scene/resource touchpoints carry
   the exact revision and evidence references (`scene:`/`resource:`
   kinds); scene/resource involvement is a deterministic complexity
   signal, never an automatic full-plan trigger. `/develop` may inspect
   scenes to support GDScript-only work and refuses required
   scene/resource mutation at the change-set boundary (no raw-text
   backdoor).

## Alternatives rejected

- **Let the model reason directly over raw `.tscn` text** — token-heavy,
  error-prone, and unrepeatable; structure must be derived
  deterministically host-side.
- **Regex-only parsing** — cannot correctly handle quoted/escaped
  strings, multiline structures, nested arrays/dictionaries, comments,
  or resource references.
- **Load every project in Godot to inspect scenes** — violates the
  static-first, fail-closed posture; imports, `@tool` execution, and
  plugin activation are unacceptable side effects of mere inspection.
- **Execute `@tool` scripts for semantic discovery** — untrusted project
  code must never run during inspection.
- **Immediately implement text-based scene mutation** — mutation needs
  the semantics first; the fail-closed posture forbids claiming native
  editing before the mutation milestone.
- **Generic graph database** — a small application-owned index covers the
  bounded relationship set; a graph store adds infrastructure without
  authority.
- **Treat the derived semantic model as authoritative source** — source
  files, workspace revisions, and Godot itself remain the truth; models
  are disposable derived projections.
- **Eagerly parse the entire repository** — parsing is lazy/on-demand with
  bounded dependency traversal; a bounded project-wide scan is available
  explicitly.

## Consequences

Benefits:

- Godot-native reasoning: node trees, inheritance, instancing, ownership,
  signals, groups, scripts, UIDs become structured model facts.
- Lower token usage: models project compact structural context instead of
  raw scene text.
- Safer future mutation: the semantic layer is the foundation the
  mutation milestone builds on.
- Better planning: verified scene/resource touchpoints with revisions and
  evidence; scene work routes to appropriate depth.
- Future impact analysis: the relationship index supports who-references-X
  queries.
- Better review/QA context: reviewer can inspect Godot-native structure
  read-only.

Costs:

- Parser maintenance and version-format compatibility as Godot text
  formats evolve; unsupported syntax degrades to bounded opaque/raw data,
  never crashes.
- Derived-state invalidation discipline: every model is revision-bound and
  staleness is explicit.
- Additional fixtures/tests: adversarial parsing fixtures and
  final-boundary no-mutation/no-process behavior tests.
