# Siralos Stage-4 entry gate

Status: authoritative (pre-Stage-4 assurance, contract Part 27).
Stage 4 (Runtime and Visual QA) begins when and only when every
criterion below holds. Status is host-observed evidence, never claims.
This document is the written gate; it is re-evaluated at each
milestone.

| #   | Criterion                                                                   | Status (R2.5)              | Owner / note                                                                                                                                                                                        |
| --- | --------------------------------------------------------------------------- | -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Stage 1–3 Rust migration audit passes                                       | **NOT MET**                | Migration is at skeleton stage (R1 foundation + R2 harness; subsystem ports R3+ pending). The differential audit (ADR 0033) is the audit mechanism and runs green on the current surface.           |
| 2   | Required Rust style/Clippy/rustfmt gates pass                               | **PASS**                   | `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` green; enforced in CI.                                                                   |
| 3   | H1 (content identity/digests) verified in Rust                              | **NOT MET**                | Port pending (R3+).                                                                                                                                                                                 |
| 4   | H2 (determinism/reproducibility) verified in Rust                           | **NOT MET**                | Port pending; determinism discipline enforced on the current surface (replay suite green).                                                                                                          |
| 5   | ICM (interpretable context) adaptation verified in Rust                     | **NOT MET**                | Port pending.                                                                                                                                                                                       |
| 6   | H3 (runtime readiness/resilience) verified in Rust                          | **NOT MET**                | Port pending.                                                                                                                                                                                       |
| 7   | Supply-chain gate passes                                                    | **PASS**                   | `cargo deny check` green (advisories, licenses, wildcards, sources); locked resolution in CI; cargo-vet decision documented (deferred, Stage 6).                                                    |
| 8   | Critical structured inputs have fuzz/property coverage proportional to risk | **PARTIAL**                | Fuzz targets (version parse, CLI args, corpus decode) + proptest properties green on the current Rust surface; provider/model/scene boundaries receive coverage when ported (R3+).                  |
| 9   | Relevant Miri/sanitizer checks pass or are documented not applicable        | **PASS (scoped)**          | All-safe Rust: Miri scoped to core (scheduled ubuntu job); ASan scheduled; TSan documented not applicable (zero shared-state concurrency); host limits recorded.                                    |
| 10  | Relevant concurrency model tests pass or are documented not applicable      | **PASS (N/A)**             | `LOOM: NOT REQUIRED` with scan evidence (zero concurrency primitives); re-evaluated at the first concurrency-bearing port.                                                                          |
| 11  | Supported platform conformance passes                                       | **PARTIAL**                | Tier-1 matrix (Linux/Windows/macOS) enforced in CI; platform-sensitive tests on the current surface green locally (Windows); POSIX scenarios execute on first CI run.                               |
| 12  | Determinism replay passes                                                   | **PASS**                   | Replay stress suite (env-order/temp-root/out-path perturbations) green on both runners; repeated-run stability test-enforced.                                                                       |
| 13  | Performance baseline exists                                                 | **PASS**                   | Criterion benchmarks + recorded baseline (`docs/development/performance-baseline.md`); budgets defined, scheduled enforcement only.                                                                 |
| 14  | No known blocking Stage 1–3 performance regression                          | **PASS**                   | No material regressions on the current surface; no optimization without measurement.                                                                                                                |
| 15  | Domain host ABI ADR is accepted                                             | **PASS**                   | ADR 0034 accepted: WIT/Component Model boundary, measured against IPC; registered in the runtime index.                                                                                             |
| 16  | CI security audit passes                                                    | **PASS**                   | Actions SHA-pinned (GitHub-owned only), least-privilege permissions, no secrets in ordinary CI, no `pull_request_target`, dependabot + CodeQL configured, publication authority isolated by design. |
| 17  | No blocking correctness/security issue remains                              | **PASS (current surface)** | Full gate green; the outstanding blocking items are the migration gaps themselves (criteria 1, 3–6).                                                                                                |

## Clean architecture reconfirmation (contract Part 26)

Reconfirmed by the architecture ratchets and this milestone's evidence:

- Siralos Core: domain-neutral (forbidden-symbol scan green; core has
  zero dependencies).
- Godot Domain: optional only; nothing installed/enabled/auto-detected;
  no marketplace, no placeholder domains.
- Godot Engine: separate dependency, never inside the domain component
  (ADR 0034).
- Godot absent: core builds and tests (proven in CI and locally).
- Host authority remains in Siralos: the domain boundary mediates every
  effect (both prototypes demonstrate this).

## Current verdict

```text
PRE-STAGE-4 ASSURANCE: NOT PASSED (deferred)
```

The gate's own stop conditions 1 and 3–6 cannot be satisfied until the
Stage 1–3 migration audit and the H1/H2/ICM/H3 ports complete (R3+).
Every criterion that is executable on the current surface has been
executed and passes. No additional general hardening milestone is
planned; the remaining criteria are owned by the porting milestones,
after which this gate is re-evaluated.
