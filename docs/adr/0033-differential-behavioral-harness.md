---
id: ADR-0033
status: accepted
domains: [process, testing]
paths: [tests/differential/**, crates/siralos-cli/src/harness/**]
supersedes: []
---

# ADR 0033 — Differential Behavioral Harness

- Status: accepted (Stage 3R — R2)
- Date: current milestone
- Related: ADR 0032 (Rust migration), ADR 0028 (content identity),
  ADR 0029 (determinism and reproducibility), ADR 0022 (executor
  briefing)

## Problem

The Siralos Rust candidate must preserve the observable behavior of the
TypeScript reference implementation (ADR 0032). Without a verification
mechanism, behavioral parity is an assertion, not a gate: nothing stops a
future port from drifting silently. R2 establishes that mechanism before
any major subsystem is ported.

## Decision

A **differential behavioral harness** is the migration's **audit
remediation gate**: it audits behavioral parity between the two
implementations on every run and blocks acceptance until drift is
remediated.

### Architecture

```text
scenario corpus (checked in, versioned, digest-bound)
        |
        +---> TypeScript oracle runner ---> canonical outcome records
        |
        +---> Rust candidate runner  ---> canonical outcome records
                                            |
                                            v
                          comparator (exact canonical match)
                          exit 0 = parity, 1 = deviation, 2 = harness error
                                            |
                                            v
                          audit report (coverage, per-subject status,
                          deviation inventory) -> remediation items
```

- **Scenario corpus**: `tests/differential/corpus/`, a versioned set of
  typed scenarios (inputs only, never expected outputs). Each scenario
  has a stable id and a SHA-256 digest over its canonical serialization
  (ADR 0028 discipline). Both runners independently enforce the same
  versioned schema, exact field sets, UTF-8/size/count bounds, canonical
  file names, digest binding, uniqueness, environment authority, and
  non-symlink corpus/file requirements before any scenario executes.
  Scenario fields: `id`, `subject`, `platforms` (`windows` / `posix` /
  `*`), `parity` (`required` | `informational`), and subject-specific
  inputs (environment fixtures for state-dir resolution).
- **Canonical outcome records**: a fixed schema, serialized with sorted
  keys and no absolute paths, timestamps, randomness, or environment
  leakage outside the scenario's declared fixtures. Records carry the
  scenario id, the outcome kind (`ok` | `error`), and subject fields.
- **Oracle runner** (TypeScript): executes each applicable scenario
  against the reference implementation and emits one record per
  scenario. Environment-sensitive scenarios run in a **probe
  subprocess** with a scrubbed environment containing exactly the
  scenario's fixtures, so both sides exercise the real environment-
  reading code path.
- **Candidate runner** (Rust): executes the same scenarios against the
  Rust candidate, also via probe subprocesses with scrubbed
  environments, and emits records in the identical format.
- **Comparator**: each runner output must already be exact canonical JSON
  with one trailing newline. The comparator rejects missing, extra,
  duplicate, reordered, malformed, or subject-mismatched records before
  comparing the canonical records. `required` scenarios must match
  exactly; `informational` scenarios are recorded and reported but never
  fail the gate. Deviations are reported per scenario with field-level
  detail. Exit codes: 0 = parity, 1 = deviation, 2 = harness error
  (mirroring the doctor exit-code conventions).
- **Audit report**: every run emits `audit.json` (digest-bound, no
  timestamps): corpus and exact runner-record digests, source commit and
  a bounded direct-byte source-tree digest, per-subject coverage, and
  per-scenario status (`parity` / `deviated` / `skipped-platform` /
  `informational`). CI retains the runner records and audit as build
  artifacts. The audit is the evidence record of migration parity for
  those exact source and corpus inputs.
- **Remediation loop**: a deviation is a remediation item. It is
  resolved either by fixing the candidate so parity is restored, or by
  classifying the divergence as documented contract scope (recorded in
  this ADR and the scenario's `parity` field). The gate stays red
  (exit 1) until the remediation is applied and re-verified; the audit
  records the resolution. Host-observed parity, never a claim, satisfies
  acceptance (ADR 0022 discipline).
- **Determinism**: two consecutive runs of either runner must produce
  byte-identical records; the harness self-tests enforce this.

### R2 subjects

R2 ports no subsystem (ADR 0032 R1 boundary). The harness proves itself
on observables that already exist on both sides:

1. **State-dir resolution**: `os.homedir()` + `.siralos` on the
   TypeScript side versus `siralos-adapters::paths::state_dir` on the
   Rust side, under environment fixtures (`USERPROFILE`, `HOME`,
   `HOMEDRIVE` + `HOMEPATH`; set, empty, and absent). Platform-specific
   scenarios run only on their platform; both runners skip the same set.
2. **Product version identity**: the version declared by
   `package.json` (TypeScript) versus `[workspace.package] version` in
   `Cargo.toml` (Rust). Parity requires the canonical version strings to
   be equal, so the two implementations can never drift apart on the
   product version.

Scenarios whose edge semantics are not part of the shared contract (for
example the POSIX `HOME`-unset fallback through the OS user database,
which Node resolves via `getpwuid` and the Rust standard library does
not) are marked `parity: informational`: they are executed and recorded
but do not gate, and their divergence is documented rather than
normalized away.

### Future ports

Every later ported subsystem adds scenarios to the corpus. A port is
not accepted until its required scenarios compare clean; the audit
records per-subject coverage, and deviations become remediation items
that block the gate until the candidate is fixed or the divergence is
classified as documented contract scope.

## Consequences

- Behavioral parity for the R2 subjects is a machine-enforced audit
  remediation gate (`npm run check:differential`, wired into
  `npm run check` and the GitHub Actions Rust workflow); every run
  emits an audit report recording coverage and per-scenario status.
- The harness is deterministic and offline: no network, no live
  providers, no mutations outside its gitignored output directory.
- The Rust candidate may need small testability seams (injectable
  environment access at probe boundaries); these must not weaken
  production semantics.
- Harness-only JSON/TOML/digest dependencies are optional behind the
  internal `differential-harness` feature. The default `siralos` product
  binary does not include the harness module or binary;
  `siralos-core` remains dependency-free.
- The audit report is the migration evidence trail: at any commit it
  states which subjects are differentially verified, which are clean,
  and which deviations are open or classified.
