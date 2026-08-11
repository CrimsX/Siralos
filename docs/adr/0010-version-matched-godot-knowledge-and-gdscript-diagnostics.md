---
id: ADR-0010
status: accepted
domains: [godot, knowledge, diagnostics]
paths: [packages/core/src/godot/**, packages/adapters/src/godot/**]
supersedes: []
---

# ADR 0010: Version-matched Godot API knowledge and GDScript check-only diagnostics (fail-closed at this stage)

Status: accepted

## Context

Stage 2 (Godot script-development MVP) needs two programming-intelligence
capabilities before any project execution:

1. **Exact-engine API knowledge.** The selected Godot executable is the
   highest authority for API availability. Solaris must be able to answer
   "what does `Node.owner` do in THIS engine?" without consulting a
   possibly-mismatched internet manual. Godot can emit its own API
   documentation: `--dump-extension-api-with-docs` writes an
   `extension_api.json` containing classes, methods, properties, signals,
   constants, enums, utility functions, built-in classes, operators, and
   their descriptions.
2. **Authoritative read-only GDScript diagnostics.** The same engine that
   will run the project is the only truthful parser for it. Godot's
   `--check-only` mode parses a script and reports diagnostics without
   executing gameplay, scenes, or scripts.

Three knowledge sources must never be conflated: the ENGINE API (exact
selected executable; highest authority for API availability), the PROJECT
(actual source, configuration, plugins, diagnostics; highest authority for
project-specific behavior), and the MANUAL DOCUMENTATION (explanatory,
version-matched where possible, not yet synchronized by Solaris). This
milestone implements engine API + project diagnostics only.

Engine console output is not a formally versioned machine protocol, so
diagnostic normalization is conservative by design: known prefixes
(`ERROR:`/`SCRIPT ERROR:`/`WARNING:`/`SCRIPT WARNING:`) and inline
`res://path:line:column` locations are parsed, unmatched error-like lines
are preserved as generic diagnostics, and line/column values are never
fabricated.

## Decision

### Exact-engine API knowledge

- **The engine-generated with-docs dump is the API authority.** Solaris
  uses `--dump-extension-api-with-docs`, never an ordinary
  `--dump-extension-api` result (which lacks documentation), and never an
  internet manual. `latest` docs must never silently guide a stable
  project: stable engines map to their exact `<major>.<minor>` manual
  channel, and prerelease/custom builds map to `unverified`.
- **The knowledge profile is bound to three identities**: the exact
  executable SHA-256, the exact API dump SHA-256, and the Solaris
  knowledge schema version. Any mismatch invalidates the profile; a
  profile must never silently survive an executable fingerprint change.
  The profile is immutable after creation, provider-neutral, and contains
  no provider data, project source, or credentials.
- **Symbol identities are deterministic** (`class:Node`,
  `class:Node/method:add_child`, `class:Node/property:owner`,
  `class:Node/signal:ready`, `class:Vector2/method:length`,
  `global:enum:Error`, `utility:lerp`), contain no filesystem paths and no
  provider-specific ids, and never assume method names alone are globally
  unique (overloads get deterministic `#N` ordinals). Engine-native names
  are preserved exactly.
- **Search is literal/token only**: exact name matches rank first, then
  prefix, then token, then document matches, with deterministic tie
  breaking and an immutable result bound. No embeddings, no internet, no
  fuzzy dependency.
- **Generation is project-independent**: the fixed
  `--dump-extension-api-with-docs` runner runs outside any project
  context, in a Solaris-private probe directory, with the workspace
  excluded from readable roots, network denied, credentials absent, stdin
  closed, output bounded, and the generated file required to be exactly
  `extension_api.json`. The dump is parsed only after successful
  generation and never written into the workspace. An ordinary
  `--dump-extension-api` result is never substituted.
- **The knowledge cache is treated as local generated data, not trust
  data.** Cached documentation is never executed, never interpreted as
  Solaris instructions, cannot affect permissions, cannot register tools,
  and is delivered to the provider only as bounded tool-result data.
  Provider input can never request raw index files or the raw dump and can
  never change the engine profile. The provider receives bounded search
  and lookup results only.

### GDScript check-only diagnostics

- **`--check-only` is the security-relevant invariant.** The only
  legitimate `--script` invocation in Solaris is
  `--headless --path <disposable-mirror> --script <mirror-script>
--check-only`, structurally enforced by the architecture check: `--path`
  may only reference the disposable mirror, `--scene`/`--editor`/
  `--import`/LSP/DAP/recovery options never appear, the source workspace
  never becomes the diagnostic `--path`, and the script value always
  operates on mirror paths. If the selected engine does not advertise
  `--check-only`, the check refuses as unsupported and the script is never
  run normally.
- **Diagnostics use the disposable mirror only.** The established
  disposable-mirror architecture (ADR 0009) is reused — no second copy
  mechanism. The source workspace is only ever read (static preparation)
  and never opened by the engine.
- **A script parse failure is a valid diagnostic result**, never an
  infrastructure failure: `checked` with `valid: false` and normalized
  diagnostics. Infrastructure failures (`denied`, `conflict`, `cancelled`,
  `timed_out`, `unsupported`, `unavailable`, `sandbox_failed`, `failed`)
  are distinct statuses.
- **Project-wide diagnostics run strictly sequentially** in the first
  implementation: simpler process/resource management, more deterministic
  results, easier cancellation, less pressure on mirror cache state. One
  disposable mirror per run; one `--check-only` invocation per script;
  deterministic aggregation with explicit truncation; full cancellation
  stops new checks, terminates the active process, and cleans the mirror.
- **Diagnostics remain one-time approved** (`godot.diagnose` is `ask` in
  every user-facing profile; there is no public unconditional `allow`).
  Approval binds to the exact script hashes, the project risk-manifest
  digest, the fixed command digest, the sandbox profile, and the check
  limits; a changed script, project, or engine is a conflict that requires
  a new approval. Prepared checks are single-use, bounded, expiring, and
  disposable.
- **Approval is only ever requested when execution is available.** The
  CLI checks support first and refuses before requesting approval while
  execution is unavailable.

### Fail-closed at this stage

Both pipelines execute Godot, and every execution path inherits the same
platform blocker as ADR 0009: Node and the pinned sandbox runtime offer no
identity-bound launch primitive (exec-by-handle — the backend re-opens the
staged copy's pathname at spawn time and a same-user process can
substitute bytes between final verification and launch), no
directory-relative create primitive, and no delete-by-handle primitive.
Therefore:

- the API documentation runner and the check-only runner **never spawn the
  executable** and report typed `unavailable` outcomes;
- the knowledge cache is an **explicitly unavailable no-op** (never
  initialized, created, read, or written: `load()` is always a miss,
  `store()` returns a typed unavailable outcome, `count()` is 0);
- no approval for knowledge generation or diagnostics is ever requested
  while execution is unavailable, and nothing is created or deleted;
- `/godot-knowledge-refresh`, `/gdscript-check`, and
  `/gdscript-diagnostics` refuse before requesting approval, and
  `/godot-knowledge`/`/status` report the truthful state;
- the designed pipeline (generation → parse → index → store →
  serve; prepare → mirror → sequential check-only → normalize →
  aggregate → cleanup) is implemented as tested contracts, adapters, and
  static preparation, and becomes operational only after identity-bound
  execution primitives exist.

### Immutable limits

API dump with docs 256 MiB; classes 20 000; symbols 500 000; single
description 256 KiB; search results 25; lookup result 512 KiB. GDScript
file 4 MiB; scripts per project check 10 000; total GDScript bytes 256
MiB; diagnostics per script 500; diagnostics per run 10 000; single check
timeout 30 s; project diagnostic budget 10 min. Providers cannot increase
any limit; truncation is always explicit.

### Why LSP is deferred

The next architectural extension is a bounded Godot GDScript LSP client
over a dedicated local-only channel: a disposable project mirror, a
recovery-mode editor instance, a dynamically allocated LSP port, TCP
JSON-RPC (`initialize`/`initialized`/`textDocument/didOpen`/
`publishDiagnostics`, then completion/hover/definition), and denied
external network. LSP needs its own local-network security design (the
LSP port must never be reachable outside the machine, the editor must run
in the disposable mirror, and the same identity-bound launch prerequisite
applies). No LSP abstractions are introduced in this milestone.

## Consequences

- The provider can search and look up the exact engine's API and receive
  normalized GDScript diagnostics with exact engine-version metadata —
  bounded, deterministic, and never raw dumps or raw index files.
- Diagnostics are honest about their source: engine-generated API docs
  are distinguished from official manual/tutorial documentation, which
  Solaris does not yet synchronize.
- Every execution surface fails closed with typed `unavailable` outcomes;
  nothing is created, deleted, or launched at this stage, and no approval
  is requested for capabilities that cannot execute.
- The architecture check now enforces: the exact with-docs tuple in the
  knowledge runner, the `--check-only` pairing in the check-only runner
  (mirror-only `--path`/`--script`, no scene/editor/import/LSP/DAP), and
  the approved-user boundaries for both runners and the disposable mirror.
- A later milestone must rebuild the cache against an identity-bound
  storage primitive from scratch; the no-op component claims no retained
  capability.
