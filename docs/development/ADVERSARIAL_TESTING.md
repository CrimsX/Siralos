# Siralos adversarial testing status

Status: authoritative (pre-Stage-4 assurance, contract Parts 4–8, 15).
Companion: `docs/development/STRUCTURED_INPUT_INVENTORY.md` (boundaries
and priorities).

## Fuzzing (contract Part 4)

- Tooling: `cargo-fuzz` 0.13.2 + `libfuzzer-sys` in `fuzz/` — a standalone
  crate **excluded from the workspace** (the root manifest lists
  `exclude = ["fuzz", "harness"]`): fuzzing requires a nightly toolchain
  and must never enter the stable quality gate.
- Targets (all assert invariants, not merely "did not panic"):
  - `version_parse` — `Version::parse` never panics; decode → encode →
    decode preserves the version; component bounds hold.
  - `cli_args` — argument parsing never panics; non-UTF-8 rejection is
    exercised per platform.
  - `corpus_scenario` — arbitrary bytes never panic, hang, or
    over-allocate the differential corpus decoder: the target in
    `fuzz/fuzz_targets/corpus_scenario.rs` feeds them to the harness's
    own strict scenario gate in `harness/src/harness/loader.rs` (typed
    JSON parse, strict shape validation, canonical digest; no
    filesystem, no process), so malformed documents, invalid parity,
    and unknown subjects are typed rejections that never silently
    become valid scenarios. Running it needs only a nightly toolchain —
    the harness no longer depends on the external module, so no sibling
    checkout is provisioned; the scheduled assurance workflow runs
    the bounded smoke
    (`cargo fuzz run corpus_scenario -- -runs=10000 -max_total_time=60`).
- Local fuzzing is unavailable from this repository's pin, and the reason
  previously recorded here was not evidence this repository holds: it
  quoted a rustc diagnostic from a target this pin does not build. What is
  locally true is the pin itself — a stable channel with
  `profile = "minimal"`, no `rust-src`, and `fuzz/` Cargo-excluded from the
  workspace — which does not satisfy cargo-fuzz's nightly requirement.
  Nothing here asserts whether the address sanitizer works on
  `x86_64-pc-windows-msvc`; that was not established. The scheduled
  assurance workflow (ubuntu) installs a pinned nightly, builds the targets,
  and runs each with a bounded smoke (`-max_total_time=60`); minimized
  crashes, if any, are added to the repository as deterministic regression
  tests by a follow-up.
- Miri is likewise unavailable locally, and the reason this file used to
  give was backwards: the active toolchain already **is** the MSVC host
  (`1.97.1-x86_64-pc-windows-msvc`), and Miri is unavailable all the same.
  What the tool prints is what is recorded — `cargo miri --version` reports
  the `miri` component as not available for that toolchain, and
  `rustup component list` offers no `miri` line for it — and why it is not
  provided is not established here. The scheduled workflow installs the
  component explicitly with pinned `nightly-2026-07-15` on ubuntu.

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
under Miri in the scheduled ubuntu workflow, which installs the component
on a pinned nightly; this pin cannot run cargo-miri, because the component
is not available for `1.97.1-x86_64-pc-windows-msvc`. Architecture is not
contorted for Miri compatibility.

## Sanitizers (contract Part 7)

- AddressSanitizer: runs in the scheduled ubuntu workflow
  (`RUSTFLAGS="-Z sanitizer=address"` + `-Zbuild-std` on `siralos-core`
  tests). Sanitizer success never claims memory correctness.
- ThreadSanitizer: **NOT APPLICABLE** — the workspace contains zero
  shared-state concurrency primitives (no `std::sync`, atomics,
  channels, `Arc`, or `thread::spawn` in any crate; verified by scan).
- Sanitizer runs are separate from the stable quality gate and use pinned
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

## Bash enforcement adversarial coverage

The `bash` surface ([ADR 0038](../adr/0038-default-mutable-execution-tools.md),
Phase C) is covered in three layers. Unit-pinned on every host:
refusal without enforcement (host and enforcing backends, marker
file absent), the `package.json` version gate (never `--version`),
offline settings shape (deny-all network, deny-root-re-allow
grants, relative workdirs refused), CLI-marker infrastructure
mapping versus child-status passthrough, and stub-supervisor rows
for argv shaping, exit passthrough, settings cleanup, timeout
kill, and cancel kill. Corpus-pinned deterministically (v88):
four `bash-executes` subjects (success, nonzero exit, timeout,
cancellation) through the stub backend, plus the unchanged
`bash-unavailable` host-refusal record. Live rows (Unix-only,
opt-in behind `SIRALOS_PROVE_SRT=1`, failing — never skipping —
when proof is requested but refused): grandchild reaping,
timeout/cancel kill, loopback denial against a live listener,
allowlist environment, closed stdin, and workdir confinement.
Status on this Windows working tree: every live row is `unknown`
(the Windows rows pin the refusal path and are green); CI wiring
to install the pinned runtime and set the variable on the Linux
gate and macOS platform jobs is a pending owner decision, so no
live enforcement is claimed here.

## UI component adversarial coverage

The declarative UI surface ([ADR 0039](../adr/0039-modular-terminal-ui-components.md))
is covered by Rust adversarial suites plus a differential subject, not by
the contract parts above: spec-boundary refusal (unknown feeds, bad
placements, oversize titles/params), projector truncation and precedence,
feed refusals (unknown params, denied containment, checkpoint preimage
exclusion), lifecycle/authority-gated merge (disabled modules contribute
nothing, gated feeds drop without authority, user ids win), authoring
refusals (duplicates, renames, missing ids, concurrent modification), and
the protected-path refusal for model writes to `.siralos/ui.toml`. The
`ui-components` differential subject pins the same outcomes with
candidate-authored expectation records.

## Local tool limitations (recorded evidence)

- The three adversarial tools are unavailable from this repository's pin, and
  the reasons previously recorded here were wrong in shape rather than in host
  name:
  - **libFuzzer**: the pin is a stable channel with `profile = "minimal"`, no
    `rust-src`, and `fuzz/` Cargo-excluded from the workspace, which does not
    satisfy cargo-fuzz's nightly requirement. Nothing here asserts whether the
    address sanitizer works on `x86_64-pc-windows-msvc`.
  - **cargo-miri**: `cargo miri --version` reports the `miri` component as not
    available for `1.97.1-x86_64-pc-windows-msvc`, and `rustup component list`
    offers no `miri` line for it. This is not a GNU-versus-MSVC matter — the
    active toolchain already is MSVC — and why the component is not provided is
    not established here.
  - **`cargo llvm-cov`**: the command is not installed for this toolchain
    (`cargo llvm-cov --version` reports no such command), although
    `llvm-tools-x86_64-pc-windows-msvc` is offered for it. No claim is made
    about any other toolchain or host.
- All three are configured to run in the scheduled ubuntu assurance workflow
  (`.github/workflows/assurance.yml`): a fuzz build plus three bounded
  `-max_total_time=60` runs, Miri on the pinned nightly, and coverage on stable
  after `rustup component add llvm-tools-preview`. No workflow in this
  repository has run yet — `ROADMAP.md` records CI as `unknown` and the release
  workflow as never executed — so these are the steps the workflow performs,
  not observed runs. Local runs are documented as unavailable rather than
  faked, and that policy now rests on the reasons above rather than on a host
  name.
- Loom and ThreadSanitizer are not required: the workspace contains no
  shared-state concurrency primitives (verified by scan).
