# Siralos platform conformance

Status: authoritative (pre-Stage-4 assurance, contract Part 9 / Part 10).

## Supported Tier-1 matrix

A platform is supported only when Siralos executes real tests there.
The Tier-1 matrix is:

| OS      | CI runner      | Build                       | Tests                | Differential harness | CLI                 |
| ------- | -------------- | --------------------------- | -------------------- | -------------------- | ------------------- |
| Linux   | ubuntu-latest  | `cargo check/test --locked` | full workspace suite | POSIX scenarios      | `siralos --version` |
| Windows | windows-latest | `cargo check/test --locked` | full workspace suite | Windows scenarios    | `siralos --version` |
| macOS   | macos-latest   | `cargo check/test --locked` | full workspace suite | POSIX scenarios      | `siralos --version` |

`.github/workflows/rust.yml` enforces the matrix; `fail-fast: false`
keeps every platform's evidence visible. The Linux gate additionally
runs fmt, clippy, documentation, differential, ratchets, the supply
chain gate, and the core-only check.

Local development hosts: Windows uses the GNU host target with
MinGW-w64 (the CI Windows runner uses the default MSVC host, which
provides the Windows SDK); Linux/macOS use the default host.

## Platform-sensitive Stage 1–3 behavior

Covered today by tests and the differential harness: path separators,
Unicode and non-UTF-8 paths, environment propagation, state-dir
resolution (USERPROFILE/HOME semantics per platform), CLI exit codes,
stdout/stderr separation, and probe subprocesses with scrubbed
environments.

Not yet testable on the current surface (no workspace mutation,
checkpoint, or process-execution capability exists in Rust): atomic
writes, permission failures, read-only files, symlink traversal,
deletion/rename semantics, cancellation, and child-process cleanup.
These receive platform conformance tests when the corresponding
subsystems are ported (R3+) — policy will represent semantic platform
differences explicitly rather than normalizing them away.

## Path correctness (contract Part 10)

The Rust candidate keeps filesystem identity in `Path`/`PathBuf`/
`OsStr`/`OsString` exclusively; there is no lossy UTF-8 conversion in
authoritative identity, equality, authorization, revision binding, or
mutation targeting. The only `display()` conversions are the
differential probe protocol output and CLI usage text — both are
diagnostics/presentation, never authoritative identity (documented in
`crates/siralos-adapters/src/paths.rs` and the harness).

Regression tests: non-UTF-8 home paths (Windows wide and Unix byte
variants), Unicode home paths, and the canonical `.siralos` name.
