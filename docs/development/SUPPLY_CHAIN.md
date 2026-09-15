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
- **Licensing: decided (2026-09-12, decision 175).** Every workspace
  member sets `publish = false`, and that is not a convention — the
  architecture gate fails a crate that does not
  (`scripts/check-rust-architecture.mjs`: "private crates must set
  publish = false"). The workspace is therefore private and unpublished
  **by policy, mechanically enforced**, and a private crate needs no
  license field: there is no distribution for a license to govern. The
  decision that was deferred to Stage 6 ("name a license") is replaced by
  a decision it can actually act on — the LICENSE DECISION IS
  PRECONDITIONED ON PUBLISHING, and publishing is preconditioned on
  removing that ratchet.

  Read plainly, because it has a real consequence: this repository is
  public but **grants no license for reuse** (no license file, no license
  field — all rights reserved by default). That is the current state, not
  an oversight.

  The trigger for naming a license is any of: the first publish attempt,
  a shipped distribution artifact, or an explicit owner intent to let
  someone else reuse the code. When it fires, the default to reach for is
  `MIT OR Apache-2.0` (the Rust ecosystem norm) applied to all three
  members with the license text committed — one small change, not a
  redesign. Until then this file is the record.

- `cargo-vet` is **not adopted**, with an explicit re-evaluation trigger
  (2026-09-12, decision 175 — the previous "re-evaluated as the graph grows
  and again at Stage 6" had no trigger and Stage 6 is Verified): re-evaluate
  when **any** of these happens — the distinct third-party direct
  dependency count grows past 20 (it is 12 today, across 19 direct entries
  in the three crates, pinned and locked), a workspace crate is published, a
  supply-chain incident touches the graph, or SBOM/provenance tooling is
  adopted. The product workspace and each retained standalone domain-ABI
  prototype stay locked and checked by `cargo-deny` (crates.io only,
  permitted licenses, wildcard and Git sources denied); duplicate transitive
  versions remain warn-level review signals.

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
