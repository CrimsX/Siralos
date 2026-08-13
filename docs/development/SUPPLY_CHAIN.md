# Siralos supply-chain and toolchain policy

Status: authoritative (pre-Stage-4 assurance, contract Part 1 / Part 25).

## Dependency policy

- The committed `Cargo.lock` is authoritative for the application.
  Authoritative CI/build verification uses `--locked` resolution;
  `cargo update` never runs inside ordinary validation CI.
- `cargo-deny` is the repository-owned supply-chain gate
  (`deny.toml`, pinned `cargo-deny` version in CI):
  - RustSec `vulnerability` findings fail the gate;
  - license allowlist covers the current graph (MIT, Apache-2.0,
    Apache-2.0 WITH LLVM-exception, BSD-3-Clause, MPL-2.0, Unicode-3.0,
    Unlicense, Zlib); BSD-3-Clause and Zlib are required by the retained
    WIT prototype graph;
  - wildcard requirements are denied; unknown registries and Git
    sources are denied (crates.io only);
  - duplicate transitive versions are a **warn-level review signal**,
    not a hard failure (the repository treats duplicates as a signal
    unless a specific duplication is known harmful).
- The private, unpublished workspace crates carry no license field; the
  licensing decision is deferred to Stage 6 release planning (recorded
  as a `cargo-deny` exception).
- `cargo-vet` is **not adopted at this stage**. The product workspace and
  each retained standalone domain-ABI prototype are locked and checked by
  `cargo-deny`; all resolved third-party packages come from crates.io and
  use permitted licenses. Duplicate transitive versions are visible as
  warn-level review signals. `cargo-vet` audit records are re-evaluated as
  the migrated product graph grows and again at Stage 6 alongside SBOM and
  provenance tooling.

## Toolchain policy

- Edition: 2024 (all crates).
- Pinned CI toolchain: `rust-toolchain.toml` (currently stable 1.97.1,
  components `rustfmt`, `clippy`). CI installs the pinned toolchain via
  rustup; no independent version is hardcoded in workflows.
- MSRV: `[workspace.package] rust-version` (currently 1.85, the edition
  2024 floor). CI checks the locked all-target/all-feature workspace on
  exact Rust 1.85.0; bumping either requires passing the full Rust gate.
- Stable Rust is the production-build requirement. Nightly is used only
  for separate quality jobs (fuzzing, Miri, sanitizers, coverage) and
  never enters the ordinary stable quality gate.
- Windows: the GNU host target with MinGW-w64 is used for local
  development; CI Windows runners use the default MSVC host (the
  runners provide the Windows SDK).

## CI integration

`cargo deny --locked check` runs in the Rust gate job
(`.github/workflows/rust.yml`), after the locked test suite, with
cargo-deny installed at its pinned version. The scheduled assurance
workflow applies the same policy to the three standalone IPC/WIT
prototype lockfiles.
