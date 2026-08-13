# Differential corpus contract

The manifest is the only execution inventory. Its version and each scenario's
canonical SHA-256 digest are checked independently by the TypeScript and Rust
runners before any scenario executes. Manifest paths must be single, relative
JSON file names beneath a corpus directory that is itself a real directory,
not a symlink. Every manifest and scenario file must also be a regular,
non-symlink file. The repository/workspace path above the supplied corpus root
is part of the host-selected launch namespace; neither runner discovers or
widens that namespace.

`required` means the scenario is fully controlled by its declared inputs and a
difference fails the migration gate. `informational` means the harness records
and reports the observation, but the scenario intentionally includes an input
that the fixture does not control.

The `state-dir.unset.*` scenarios are informational. An empty process
environment does not remove the operating system's account database, profile,
or sandbox-selected home directory. `state-dir.fallback.posix` is informational
for the same reason: an empty `HOME` makes both implementations consult the OS
account database. Node and Rust may therefore observe different host-owned
fallback homes even though both probe environments contain the same declared
variables. The deterministic, gating state-directory contract is exercised by
`state-dir.set.*` and `state-dir.fallback.windows`, whose relevant home inputs
are explicit.
