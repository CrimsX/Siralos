# Differential corpus contract

The manifest is the only execution inventory. It binds corpus schema version,
corpus version, ordered scenario file names, each scenario's canonical SHA-256
digest, and an overall `corpusSha256` over that identity tuple. The TypeScript
and Rust loaders independently validate the complete manifest and every
scenario digest before any fixture executes. A missing digest, malformed
digest, content mismatch, or unsupported version is a typed corpus-integrity
failure, never a parity result.

Manifest paths must be single, relative JSON file names beneath a corpus
directory that is itself a real directory, not a symlink. Every manifest and
scenario file must also be a regular, non-symlink file. The
repository/workspace path above the supplied corpus root is part of the
host-selected launch namespace; neither runner discovers or widens that
namespace.

`required` means the scenario is fully controlled by its declared inputs and a
difference fails the migration gate. `informational` means the harness records
and reports the observation, but the scenario intentionally includes an input
that the fixture does not control.

The R3 task-contract scenarios (subject `task-contract`) execute the
host-owned task kernel (contracts, phases, evidence, acceptance,
completion, activity, progress) against both implementations; their
`input` carries the controlled contract, steps, operations, and clock.
The `state-dir.unset.*` scenarios are informational. An empty process
environment does not remove the operating system's account database, profile,
or sandbox-selected home directory. `state-dir.fallback.posix` is informational
for the same reason: an empty `HOME` makes both implementations consult the OS
account database. Node and Rust may therefore observe different host-owned
fallback homes even though both probe environments contain the same declared
variables. The deterministic, gating state-directory contract is exercised by
`state-dir.set.*` and `state-dir.fallback.windows`, whose relevant home inputs
are explicit.

Every runner emits protocol schema version 1. Applicable scenarios produce
`COMPLETED`, `PRODUCT_FAILURE`, `UNIMPLEMENTED`, or `UNSUPPORTED`. A scenario
excluded by the current platform remains present as an explicit `UNSUPPORTED`
record with category `PLATFORM_NOT_APPLICABLE`; it is not silently omitted.

The Stage 3R R5 subjects (language-diagnostics, language-structure, and
language-definition) execute generic language-intelligence parity: the
TypeScript side reaches the real generic language modules
(`packages/core/src/language`) and the production Godot wrappers/URI
mapping; the Rust side reaches `siralos-core::language` and
`siralos-adapters::language::uri`. The language-structure fixtures
carry pre-structured, language-neutral semantic documents (never raw
source; paths like `src/example.lang`, generic declaration kinds,
opaque attributes, no GDScript/Godot vocabulary), so R5 proves the
generic structural representation and the deterministic advisory
summary semantics — not GDScript/Godot parsing. Godot-specific
language parity (engine diagnostics, LSP, the GDScript scanner) is
deliberately NOT established by this corpus; those surfaces remain
the later Godot milestones' oracle.

The Stage 3R R6 subjects (domain-lifecycle and domain-capability)
execute generic Domain lifecycle/capability parity: the TypeScript
side reaches the real generic reference modules
(`packages/core/src/domain`); the Rust side reaches
`siralos-core::domain`. The `domain-lifecycle` scenarios drive the
explicit absent/installed/enabled/active state machine (typed
transitions, activation eligibility, exact activation binding, and
the workspace-scan proof that workspace contents never install,
enable, activate, download, or recommend a Domain — including a
repository containing `project.godot` with no installed Domain). The
`domain-capability` scenarios exercise the Host-authoritative grant
decision (grant subset behavior, typed denial with ordered missing
capabilities, and the invariant that denial never widens Host
authority). The actual WIT/Component execution boundary is proven by
the Rust conformance suite
(`crates/siralos-adapters/tests/domain_conformance.rs`) on the
checked-in synthetic conformance component bytes
(`tests/domain-conformance/`); the differential subjects cover the
generic semantics only, exactly like the earlier subjects.
The Stage 3R R7.2 `tool-loop` scenarios execute the generic Application
Tool Loop through the real TypeScript reference (`createSiralosApplication`,
Tool Registry, Tool Round, permission evaluator) and the real Rust
candidate (`siralos-core::tool` registry/round/application loop plus the
workspace read adapter where selected). The 16 required scenarios cover
terminal completion, round-budget normalization and the over-budget
boundary, unknown/hidden-tool denial and provider recovery, invalid and
duplicate call pairing, one-call/one-result under mixed execute/invalid/
cancelled calls, Host cancellation during a round, mixed assistant-text
commit rules, provider failure after a committed round, the five-gate
authorization matrix (allow/deny/ask-plain), the displayInput UTF-16
matrix (200/201/supplementary/surrogate-split/source object key order),
and the Tool-result status to event matrix. Stub Tools are probe-local
and always enter through the real registry/gates/round/loop; no
mutation, process, Git, network, or Godot authority is granted or
exercised.

The Stage 3R R7.4 `user-config` scenarios execute the bounded user-level
configuration boundary through the TypeScript reference loader/diagnostics
and the Rust adapter plus CLI composition path. The matrix covers missing
defaults, a full configuration, strict top-level and nested unknown-key
rejection, sandbox/backend/edition validation, installation and reference
count bounds, absolute installation paths, registered review-provider
validation, invalid JSON, the exact one-MiB boundary and one-byte overflow,
non-regular files, and nonfatal semantic reference failures. Symlink
rejection is represented by a POSIX-only required scenario because the
current Windows host cannot expose that filesystem case portably. The
candidate never creates the default configuration path, reads environment
credentials, selects an engine, launches Godot, or widens sandbox authority.

The Stage 3R R8 `godot-*` subjects execute Godot Stage-2 parity through
the real TypeScript reference services and the real Rust adapters and
core parsers. `godot-scene-resolve` drives the bounded scene/resource
parsers, including the three selection rules that previously diverged
(a null `tres` key is a missing key, unknown input keys are ignored,
and a declared `tres` wins over `tscn`). `godot-discovery` runs the
engine profiler's discovery generation and selected-profile query over
declared config and sanitized host PATH/PATHEXT inputs whose entries
never exist, so outcomes are deterministic with zero filesystem
effects. `godot-knowledge`, `godot-diagnostics`, and `godot-lsp`
exercise the production fail-closed service stacks: truthful support
reasons, preparation refusal before any approval or mirror work,
cancellation propagation, invalid-input ordering before availability,
unknown prepared-check execution failure, and bounded session state.
No scenario launches an engine, creates files, opens ports, or widens
sandbox authority.
