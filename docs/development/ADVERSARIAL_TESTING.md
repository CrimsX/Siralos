# Siralos adversarial testing status

Status: authoritative (pre-Stage-4 assurance, contract Parts 4–8, 15).
Companion: `docs/development/STRUCTURED_INPUT_INVENTORY.md` (boundaries
and priorities).

## Fuzzing (contract Part 4)

- Tooling: `cargo-fuzz` 0.13.2 + `libfuzzer-sys` in `fuzz/` — a standalone
  crate **excluded from the workspace** (`exclude = ["fuzz"]`): fuzzing
  requires a nightly toolchain and must never enter the stable quality
  gate.
- Targets (all assert invariants, not merely "did not panic"):
  - `version_parse` — `Version::parse` never panics; decode → encode →
    decode preserves the version; component bounds hold.
  - `cli_args` — argument parsing never panics; non-UTF-8 rejection is
    exercised per platform.
  - `corpus_scenario` — arbitrary JSON never panics the differential
    corpus decoder; invalid parity/unknown subjects never silently
    become valid scenarios.
- The **Windows GNU host cannot build libFuzzer** (requires clang;
  `address sanitizer is not supported for this target`), so local
  fuzzing is documented as unavailable on this host. The scheduled
  assurance workflow (ubuntu) builds and runs the targets with a
  bounded smoke (`-max_total_time`/`-runs`), and minimized crashes, if
  any, become deterministic regression tests in the repository.
- Miri on the Windows GNU host is likewise unavailable (cargo-miri
  requires the MSVC target); the scheduled workflow runs it with pinned
  `nightly-2026-07-15` on ubuntu.

## Property testing (contract Part 5)

- `siralos-core` (proptest): canonical `major.minor.patch` strings
  parse and round-trip through `Display`; ordering is numeric and total
  (matches lexicographic component order); arbitrary digit/dot strings
  never panic and canonicalize (parse → display → reparse is stable).
- Differential harness (deterministic generator): canonical JSON is
  idempotent over parse→canonicalize→parse, and produces sorted keys
  with stable digests regardless of input key order.

## Miri (contract Part 6)

The workspace is fully safe Rust (`unsafe_code = "forbid"`; zero
`unsafe` occurrences; no FFI, pointer manipulation, or custom memory
representation). Miri therefore adds minimal signal for
infrastructure-heavy tests. It is kept scoped: `siralos-core` tests run
under Miri in the scheduled ubuntu workflow; the Windows GNU host cannot
run cargo-miri. Architecture is not contorted for Miri compatibility.

## Sanitizers (contract Part 7)

- AddressSanitizer: runs in the scheduled ubuntu workflow
  (`RUSTFLAGS="-Z sanitizer=address"` + `-Zbuild-std` on `siralos-core`
  tests). Sanitizer success never claims memory correctness.
- ThreadSanitizer: **NOT APPLICABLE** — the workspace contains zero
  shared-state concurrency primitives (no `std::sync`, atomics,
  channels, `Arc`, or `thread::spawn` in any crate; verified by scan).
- Sanitizer jobs are separate from the stable quality gate and use pinned
  `nightly-2026-07-15`.

## Concurrency model testing (contract Part 8)

```text
LOOM: NOT REQUIRED
```

Evidence: no custom concurrency primitives, locks, channels, atomics,
cancellation races, lifecycle races, shared process state, or
concurrent observation normalization exist in the current Rust
workspace (scan of `crates/` for `std::sync`, `thread::spawn`, `Arc<`,
atomics, and channel types returns zero matches). The first
concurrency-bearing subsystem (task runtime / process supervision)
triggers a Loom re-evaluation at its porting milestone.

## Coverage analysis (contract Part 15)

Pinned `cargo-llvm-cov` 0.8.7 over the workspace test suite is used to
locate untested critical paths (security decisions, identity,
validation, state transitions). There is no repository-wide percentage
objective; generated/error-only boilerplate is not artificially tested.
Results are recorded per milestone in the R2.5 report.

## Host limitations (recorded evidence)

- The Windows GNU host cannot build libFuzzer (`address sanitizer is
not supported for this target`; requires clang) and cannot run
  cargo-miri (requires the MSVC target) or `cargo llvm-cov` (the GNU
  rustc distribution lacks the profiler runtime). All three are
  executed in the scheduled ubuntu assurance workflow; local runs on
  this host are documented as unavailable rather than faked.
- Loom and ThreadSanitizer are not required: the workspace contains no
  shared-state concurrency primitives (verified by scan).
