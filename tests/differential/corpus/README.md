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
carry pre-structured semantic documents (never raw source), so R5
proves the generic structural representation and the deterministic
advisory summary semantics — not GDScript/Godot parsing. Godot-
specific language parity (engine diagnostics, LSP, the GDScript
scanner) is deliberately NOT established by this corpus; those
surfaces remain the later Godot milestones' oracle.
