# Decision — TypeScript Archive Removal — Oracle Freeze and Complete Tree Removal

**Wayfinder ticket:** [TypeScript Archive Removal — Oracle Freeze and Complete Tree Removal](../tickets/40-typescript-archive-removal.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R12 Disposition Execution](33-r12-disposition-execution.md) (PASS, retired at 4bef901, `PRE-STAGE-4 ASSURANCE: PASSED`) + [Stage 4.1 v32 Reconciliation](../tickets/40-typescript-archive-removal.md) (corpus v32, 234/234, `CORPUS_VERSION` 32)
**Decided:** 2026-08-28 (resolver session; HITL grilling over the 8-clause frozen removal contract and the 4 open auxiliary questions)
**Status:** **PASS — Freeze contract C1–C8 approved; implementation authorized as next slice**
**Self-loop ledger:** 4 criteria, one decision pass (verification below)

> Mirrors `decisions/33-r12-disposition-execution.md` §2.1 correction. No deletion lands here; this freezes what freezes, what is removed, and what stays. The agent never answered for the human (guardrail h).

---

## Summary

The **TypeScript historical oracle is frozen at the current corpus v32 commit** (`5da5cde`, `CORPUS_VERSION` 32, `corpusSha256 c8b70a95…`, audit 234/234 applicable required, 4 skips) and will be **completely removed from the live repository tree** after a pinned-record freeze. The 8-clause removal contract is confirmed; the 4 auxiliary questions are answered; the implementation slice is authorized next as one coherent pass (C1→C8) with atomic doc advancement.

## 1. Frozen removal contract (confirmed)

| #   | Clause                 | Frozen contract                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| --- | ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Oracle freeze first    | No deletion before freeze: run fresh differential audit at `5da5cde` (corpus v32, 234/234), pin canonical oracle outcome records digest-bound under `tests/differential/evidence/` (r11/r13 precedent). Record freeze-commit SHA, corpus digest, oracle + candidate record digests.                                                                                                                                                                                                   |
| C2  | Harness rework         | `run-differential.mjs` compares candidate against **pinned** oracle records. Live TS oracle leg (`run-oracle.mjs`, 44 probes, `ts-remap-loader`) is removed or demoted to explicit opt-in historical replay (worktree at the freeze SHA) that is never part of `npm run check`. Corpus bumps to v33 at reconciliation commit.                                                                                                                                                         |
| C3  | Gate + script rework   | `check` becomes rust gates + reworked differential. `check:typescript` retires. Classify `scripts/*.mjs`: remove TS-coupled checks (`check-architecture.mjs` + test; `check-nondeterminism.mjs` if TS-only; sandbox/Godot conformance runners — port useful parts to Rust per Q1); keep language-neutral hygiene (`check-doc-links`, `check-identity` with exclusion reconciliation, `check-public`, `check-project-context`, `check-rust-architecture`, `build-conformance-guests`). |
| C4  | Tree removal + sweep   | Delete `apps/**`, `packages/**`, `tests/behavior/**` (14 files), TS configs (`tsconfig*`, eslint, vitest; prettier scope per Q3), `workspaces` entries, TS npm scripts, lockfile churn. Completeness sweep: grep `packages/`, `apps/`, `npm run build`, `vitest`, `tsc` across `scripts/`, `.github/`, `crates/`, `docs/`, `schemas/`, `experiments/`, `fuzz/`, `tests/`.                                                                                                             |
| C5  | CI rework              | `rust.yml` and `tier1-evidence.yml` drop TS jobs/steps; differential stays in pinned mode; CodeQL disposition recorded.                                                                                                                                                                                                                                                                                                                                                               |
| C6  | Atomic doc advancement | Seven-surface pattern (`decisions/33` §3): `PROJECT_CONTEXT.md` (head + §2), `RUST_MIGRATION.md` (removal paragraph + table row), `ROADMAP.md`, `AGENTS.md`, `README.md`, `scripts/check-project-context.mjs` expectations, resolution decision doc. Reconcile `ARCHITECTURE.md`, `docs/architecture/README.md`, `GOLDEN_TRACES.md` TS references.                                                                                                                                    |
| C7  | Honest trade-off       | Live differential drift-detection net (ADR 0033) replaced by pinned records + freeze-SHA replay. Future Rust behavior changes require explicit corpus/expectation updates; no silent oracle backstop. Stated in decision doc, not buried.                                                                                                                                                                                                                                             |
| C8  | Posture unchanged      | Nothing flips a typed `unavailable` effect; no new spawn path; no Rust behavior change beyond corpus/expectation pinning.                                                                                                                                                                                                                                                                                                                                                             |

## 2. HITL answers (2026-08-28)

| #   | Frontier question                                                   | Human answer                                                                                                      |
| --- | ------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| 1   | Sandbox/Godot conformance runners (`scripts/sandbox/*`)             | **Port to Rust** — port still-useful live-probe logic to Rust-side conformance; retire TS wrappers                |
| 2   | Opt-in historical replay command                                    | **Pinned + opt-in** — pinned records by default, explicit `worktree at freeze SHA` replay command retained        |
| 3   | Prettier/eslint retention scope                                     | **Retain minimal** — keep for remaining `.mjs`/`.yml`/`.md` surfaces, remove TS-specific configs                  |
| 4   | `experiments/`, `schemas/`, `tests/domain-conformance/` disposition | **Keep schemas+fixtures** — retain `schemas/` + `tests/domain-conformance/fixtures`, drop `experiments/` if empty |
| 5   | Freeze contract approval                                            | **Approve C1–C8** as drafted                                                                                      |

## 3. Acceptance criteria (implementation slice)

| #   | Criterion                                                           | Check                                                                  |
| --- | ------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| A1  | `npm run check` exit 0 on removal tree (rust + pinned differential) | observed full-gate output at removal commit                            |
| A2  | No live references to removed tree                                  | grep sweep (C4) returns only documented historical/decision references |
| A3  | Pinned records equal freeze-commit oracle output                    | digest equality vs C1 audit artifacts                                  |
| A4  | CI references only existing scripts                                 | workflow grep + `npm run check:docs`                                   |
| A5  | Doc surfaces advanced atomically                                    | `git show --stat` one commit                                           |

**Not due in this decision:** No file is deleted here (`git status --porcelain` clean at `5da5cde`), no harness rewired, no corpus bump.

## 4. Authorization

**TypeScript archive removal is authorized as the next implementation slice** against this frozen contract and the 5 HITL answers above. The slice requires: C1 freeze audit + pin, C2 harness pinning + `run-oracle` demotion, C3 `check`/`check:typescript` rework + script classification, C4 tree sweep, C5 CI rework, C6 seven-surface atomic docs, C7 trade-off disclosure, C8 posture verification. Corpus advances to v33 at the reconciliation commit.

---

## Self-loop verification

| Criterion                                   | Direct evidence                                                                                                       | Status |
| ------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- | ------ |
| 8-clause contract frozen and HITL-approved  | §1 C1–C8 table + §2 row 5 `Approve C1–C8` (2026-08-28)                                                                | pass   |
| All 4 auxiliary questions answered by human | §2 Q1–Q4: `Port to Rust` / `Pinned + opt-in` / `Retain minimal` / `Keep schemas+fixtures`                             | pass   |
| No deletion before freeze (C1 guard)        | `git status --porcelain` clean at `5da5cde`; `packages/` + `apps/` still present                                      | pass   |
| Boundaries explicit (what stays, what goes) | §1 C3–C4: port vs retire classified; `experiments/` vs `schemas/`/`domain-conformance` distinct; C8 posture unchanged | pass   |
