# Decision — R12 Disposition Execution — Retirement Verdict

**Wayfinder ticket:** [R12 Disposition Execution](../tickets/20-r12-disposition.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R13 Verified Promotion](32-r13-verified-promotion.md) (PASS, R1–R13 Verified, corpus v31) + [R13 Execution Register](../tickets/22-r13-execution-register.md) (closed) + [R12 Disposition Template](07-r12-disposition.md)
**Decided:** 2026-08-27 (resolver session, HITL grilling on the 8-row shared evidence package and the 5-field retention vs retirement split; inspection of the local 231/231 audit, corpus v31 manifest, and `stage4-entry-gate.md` 17 criteria)
**Status:** **PASS — R12 Verified (retired) at `4bef901` (executable `72e20be`)**
**Self-loop ledger:** 5 criteria, one implementation pass (verification below)

> The human decided **retirement** (guardrail h: the agent never answers for the human). This decision re-presents the evidence package per template 07 §1 and records the verdict per §1.2/§2.

---

## Summary

**Stage 3R R12 — TypeScript reference retirement — is Verified (retired).** The Rust candidate is now the **sole behavioral source of truth** per ADR 0032. Every required surface is at differential parity, every impossible effect still reports typed `unavailable`, and the 17-criteria stage-4 entry gate is all PASS.

The TypeScript source under `packages/**` + `apps/**` is archived as historical oracle (commit `4bef901` retained for SHA-bound audit replay). Fixing a bug now means fixing the Rust code in `crates/**`.

## 1. Shared evidence package (8 rows, all PASS on `72e20be` / `4bef901`)

| Evidence                               | Artifact (SHA-bound)                                                                                                                                                                                                                                                                                                                                                      | Where retained                                                                              | Status   |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | -------- |
| **Differential audit v31**             | `tests/differential/out/audit.json` — `231/231` applicable required, 4 platform skips, 0 informational, schema 3, `corpusSha256 e24f4bb15ee532e9825e23f8f61a0912e13a1d6a8e5b4bffc90ce1fae78af504`, `candidateRecordsSha256 1271343…`, `corpusVersion 31`, `sourceTreeSha256 c5bea8b…` (local Windows; R11 Tier-1 v23 `50c0575f…` remains the cross-platform matrix proof) | `tests/differential/out/audit.json` (local) + `tests/differential/evidence/r13/` (retained) | **PASS** |
| **Corpus promotion manifest**          | `tests/differential/corpus/manifest.json` v31, 236 files, schema 3, digests per file                                                                                                                                                                                                                                                                                      | `tests/differential/corpus/`                                                                | **PASS** |
| **Harness replay stress**              | `tests/differential/replay.test.mjs` + `oracle*.json`/`cand-*.json` — deterministic, no `EPERM` mis-pass                                                                                                                                                                                                                                                                  | `tests/differential/`                                                                       | **PASS** |
| **Stage 1–3 migration audit**          | Every Stage-3 milestone at Rust parity (R3 task kernel, R4 workspace, R5 language, R6 domain, R7 provider/tool/projection/config/CLI, R8 Godot Stage-2, R9 Godot Stage-3, R10 H1/H2/ICM/H3, R11 full parity, R13 remaining surfaces) — zero `unknown`                                                                                                                     | `docs/development/RUST_MIGRATION.md` R12 section                                            | **PASS** |
| **Fail-closed effect-boundary parity** | `workspace-prepare`/`workspace-apply`/`checkpoint`/`godot.*` all `unavailable` without mutation (`harness_cli_session.rs` zero spawn)                                                                                                                                                                                                                                     | `tests/differential/corpus`                                                                 | **PASS** |
| **Typed recovery readiness**           | 7 dimensions (`recovery-taxonomy`, `runtime-readiness.*`) with stable codes                                                                                                                                                                                                                                                                                               | `tests/differential/corpus`                                                                 | **PASS** |
| **Sandbox truthfulness**               | `npm run test:sandbox` loud skip (`setup-required` on Windows)                                                                                                                                                                                                                                                                                                            | `SECURITY.md` live probe                                                                    | **PASS** |
| **Performance baselines**              | `RUST_STYLE.md 568-589` measurement, `performance-baseline.md` no blocking regression, gate 7–10 PASS                                                                                                                                                                                                                                                                     | `stage4-entry-gate.md` rows 7–10                                                            | **PASS** |

All eight were **observed PASS on the same worktree** (`72e20be` executable, `4bef901` promotion). The `e24f4bb` corpus digest and `231/231` audit are the retirement proof.

## 2. Retirement verdict and retained audits

- **Verdict:** `retirement` (no qualifier)
- **Evidence SHA:** `R13 Verified 72e20be` + `R12 promotion 4bef901` + `audit e24f4bb` + `corpus v31 236 files, 231/231`
- **Rationale:** Every required observable is at parity, every impossible effect still fails closed with a typed reason, and no remaining required-but-not-due surface exists (the `R13` continuation closed the last 20 unported fixtures). No blocking correctness, security, or performance issue remains.
- **Retained audits (post-retirement proof):**
  - `tests/differential/out/audit.json` (`231/231` at v31, `candidateRecordsSha256 1271343…`)
  - `tests/differential/corpus/` (236 files, `corpusSha256 e24f4bb…`)
  - `docs/development/stage4-entry-gate.md` 17/17 PASS (re-evaluated at retirement, `PRE-STAGE-4 ASSURANCE: PASSED`)
  - `docs/development/RUST_MIGRATION.md` R12 retirement paragraph + `PROJECT_CONTEXT.md` head

After retirement, the Rust path is the sole source of truth; the TypeScript archive (`packages/**` + `apps/**` at `4bef901`) is historical and not executed.

## 3. Atomic surface advancement (this commit)

Seven surfaces advanced together per template 07 §2 (same 7-surface pattern as R7/R13):

1. `docs/development/PROJECT_CONTEXT.md` — head (`R12 Verified (retired)`), position table (`R12 COMPLETE`), milestone row, `Last verified commit: 4bef901`, `Latest verified executable worktree: 72e20be`, R12 retirement paragraph
2. `docs/development/RUST_MIGRATION.md` — table row `R12 Verified (retired)`, new R12 retirement paragraph (verdict + evidence SHA + retained audits)
3. `ROADMAP.md` — table row `R12 Verified (retired)`, Current tail (`R12 retired — Rust sole source of truth`)
4. `AGENTS.md` — Current line (`R12 Verified (retired) at 4bef901`)
5. `README.md` — status line (`R12 Verified (retired)`)
6. `scripts/check-project-context.mjs` — `R12 COMPLETE` expectation + `Last verified commit` SHA check
7. `docs/development/stage4-entry-gate.md` — criteria 1,3–6 flipped `NOT MET → PASS`, `PRE-STAGE-4 ASSURANCE: PASSED`

No TypeScript source was deleted in this commit; archival is by retention of the `4bef901` oracle SHA and will be performed as a separate historical archive after verification (guardrail e: no removal before disposition).

## 4. Guardrail compliance

All eight guardrails from template 07 §3 are satisfied:

- **a** R11 owner of recovery parity — R11 Verified at `eea0029`, retained
- **b** Every predecessor Verified — R1–R13 all Verified
- **c** Harness audit-bound — schema 3, v31, digests, symmetric supervision, 4 skips explicit
- **d** Four-thing gate — R1–R13 + migration audit + R12 (this) + 17 criteria now PASS → Stage 4 may begin
- **e** No premature removal — `packages/` retained at `4bef901`
- **f** Retention 5-field block — N/A for retirement (retirement has retained-audit split instead)
- **g** Retained audits — §2 lists them by SHA
- **h** HITL grilling — human chose `retirement` via `tickets/20-r12-disposition.md` (agent never answered)

---

## Self-loop verification

| Criterion                                       | Direct evidence                                                                                                                                                        | Status |
| ----------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Shared 8-row evidence all PASS on same worktree | §1 table: audit 231/231 v31, corpus 236, replay green, migration audit all paritied, fail-closed 0 spawn, typed recovery 7 dims, sandbox loud skip, perf baseline PASS | pass   |
| Retirement verdict + retained audits filed      | §2: verdict `retirement`, evidence SHA `4bef901`/`e24f4bb`/`72e20be`, rationale, retained-dir list (`audit.json`/`corpus/`/`stage4-entry-gate.md`)                     | pass   |
| Seven surfaces advance atomically               | §3 enumerates them; each edited in the same commit (`git show --stat`)                                                                                                 | pass   |
| No premature TypeScript deletion                | §3: `packages/` retained at `4bef901`                                                                                                                                  | pass   |
| Stage-4 gate now PASSED, not just R12           | `stage4-entry-gate.md` 1,3–6 flipped PASS, verdict `PASSED`                                                                                                            | pass   |

Evidence ladder: L1 inspected `audit.json` (231/231), `manifest.json` (v31 e24f4bb), `stage4-entry-gate.md` (17/17), and `RUST_MIGRATION.md` R12 paragraph; L2 file:line citations across §3 surfaces; L3 template 07 precedent; L4 this decision.
