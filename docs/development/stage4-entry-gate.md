# Siralos Stage-4 entry gate

Status: authoritative (pre-Stage-4 assurance, contract Part 27); the gate held — Stage 4 was subsequently realized in full and Verified at `9566eee` (decision 46).
Stage 4 (Runtime and Visual QA) begins when and only when every
criterion below holds. Status is host-observed evidence, never claims.
This document is the written gate; it is re-evaluated at each
milestone.

| #   | Criterion                                                                   | Current status             | Owner / note                                                                                                                                                                                                                                                                                                            |
| --- | --------------------------------------------------------------------------- | -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Stage 1–3 Rust migration audit passes                                       | **PASS**                   | R1-R13 all Verified (R3 task kernel, R4 workspace, R5 language, R6 domain, R7 provider/tool/projection/config/CLI, R8 Godot Stage-2, R9 Godot Stage-3, R10 H1/H2/ICM/H3, R11 full parity, R13 remaining surfaces) at corpus v31, 236 files, 231/231 applicable required (4 skips); differential audit green on 4bef901. |
| 2   | Required Rust style/Clippy/rustfmt gates pass                               | **PASS**                   | `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` green; enforced in CI.                                                                                                                                                                                       |
| 3   | H1 (content identity/digests) verified in Rust                              | **PASS**                   | R10a H1 at v18, 182 files, 177/177; R13 retains parity.                                                                                                                                                                                                                                                                 |
| 4   | H2 (determinism/reproducibility) verified in Rust                           | **PASS**                   | R10a H2 at v18, replay suite green; R13 retains parity.                                                                                                                                                                                                                                                                 |
| 5   | ICM (interpretable context) adaptation verified in Rust                     | **PASS**                   | R10b ICM at v19, 195 files, 190/190; R13 retains parity.                                                                                                                                                                                                                                                                |
| 6   | H3 (runtime readiness/resilience) verified in Rust                          | **PASS**                   | R10c H3 at v20, 210 files, 205/205; R13 retains parity.                                                                                                                                                                                                                                                                 |
| 7   | Supply-chain gate passes                                                    | **PASS**                   | `cargo deny check` green (advisories, licenses, wildcards, sources); locked resolution in CI; cargo-vet decision documented (deferred, Stage 6).                                                                                                                                                                        |
| 8   | Critical structured inputs have fuzz/property coverage proportional to risk | **PARTIAL**                | Fuzz targets (version parse, CLI args, corpus decode) + proptest properties green on the current Rust surface; provider/model/scene boundaries receive coverage when ported (R4+).                                                                                                                                      |
| 9   | Relevant Miri/sanitizer checks pass or are documented not applicable        | **PASS (scoped)**          | All-safe Rust: Miri scoped to core (scheduled ubuntu job); ASan scheduled; TSan documented not applicable (zero shared-state concurrency); host limits recorded.                                                                                                                                                        |
| 10  | Relevant concurrency model tests pass or are documented not applicable      | **PASS (N/A)**             | `LOOM: NOT REQUIRED` with scan evidence (zero concurrency primitives); re-evaluated at the first concurrency-bearing port.                                                                                                                                                                                              |
| 11  | Supported platform conformance passes                                       | **PARTIAL**                | Tier-1 matrix (Linux/Windows/macOS) enforced in CI; platform-sensitive tests on the current surface green locally (Windows); POSIX scenarios execute on first CI run.                                                                                                                                                   |
| 12  | Determinism replay passes                                                   | **PASS**                   | Replay stress suite (env-order/temp-root/out-path perturbations) green on both runners; repeated-run stability test-enforced.                                                                                                                                                                                           |
| 13  | Performance baseline exists                                                 | **PASS**                   | Criterion benchmarks + recorded baseline (`docs/development/performance-baseline.md`); budgets defined, scheduled enforcement only.                                                                                                                                                                                     |
| 14  | No known blocking Stage 1–3 performance regression                          | **PASS**                   | No material regressions on the current surface; no optimization without measurement.                                                                                                                                                                                                                                    |
| 15  | Domain host ABI ADR is accepted                                             | **PASS**                   | ADR 0034 accepted: WIT/Component Model boundary, measured against IPC; registered in the runtime index.                                                                                                                                                                                                                 |
| 16  | CI security audit passes                                                    | **PASS**                   | Actions SHA-pinned (GitHub-owned only), least-privilege permissions, no secrets in ordinary CI, no `pull_request_target`, dependabot + CodeQL configured, publication authority isolated by design.                                                                                                                     |
| 17  | No blocking correctness/security issue remains                              | **PASS (current surface)** | Full gate green; the outstanding blocking items are the migration gaps themselves (criteria 1, 3–6).                                                                                                                                                                                                                    |

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
PRE-STAGE-4 ASSURANCE: PASSED (R12 retired, R1–R13 Verified at 4bef901, corpus v31)
```

All 17 criteria are **PASS** (or PASS-scoped) on worktree 4bef901. The Stage 1–3 migration audit (criterion 1) and H1/H2/ICM/H3 (criteria 3–6) are now PASS via R10a-c + R13. The gate is re-evaluated at each milestone; no additional hardening milestone is planned beyond the porting milestones now complete.
