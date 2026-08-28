---
title: "TypeScript Archive Removal - Oracle Freeze and Complete Tree Removal"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: "PASS — Freeze contract C1-C8 approved (HITL 2026-08-28: Q1 Port to Rust, Q2 Pinned+opt-in, Q3 Retain minimal, Q4 Keep schemas+fixtures). Implementation slice authorized next; no deletion in this decision."
blockedBy: []
---

## Question

Per the R12 retirement verdict ([decision 33](../decisions/33-r12-disposition-execution.md))
and its §2.1 correction (oracle executed for differential only), decide and
scope the **complete removal** of the retired TypeScript tree
(`packages/**` + `apps/**`) and of every gate, script, and CI surface
that still executes it. Direct human direction opened this ticket:
**completely remove them**. The formal HITL at resolution confirms the
freeze contract below; the agent never answers for the human (guardrail h
pattern from template [07](../decisions/07-r12-disposition.md)).

## Why this is a slice, not a cleanup

- Retirement is an authority disposition, not a deletion. The TS tree is
  still **executed** by `npm run check:differential`
  (`tests/differential/run-oracle.mjs` plus 44 probe files holding 189
  `packages/` imports) and by `check:typescript` inside
  `npm run check`.
- The oracle has **diverged from the retained `4bef901` SHA**: corpus v32
  added post-retirement oracle probes (`runtime-execution`,
  `runtime-evidence`). The freeze must archive at the **freeze commit**,
  never `4bef901`.
- Decision 33 §3 explicitly deferred physical archival to "a separate
  historical archive after verification"; no ticket existed until this one.
- CI executes the TS tree today: `.github/workflows/rust.yml` (format,
  lint, typecheck, architecture, nondeterminism, identity, public, docs,
  context, differential) and `.github/workflows/tier1-evidence.yml`
  (differential + sandbox conformance).

## Frozen removal contract (draft for HITL confirmation)

| #   | Clause                 | Contract                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| --- | ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Oracle freeze first    | No deletion before the freeze: run a fresh differential audit at the freeze commit (currently corpus v32, 234/234 applicable required, 4 skips) and pin the canonical oracle outcome records digest-bound under `tests/differential/evidence/` (r11/r13 precedent). Record freeze-commit SHA, corpus digest, oracle + candidate record digests.                                                                                                                                                               |
| C2  | Harness rework         | `run-differential.mjs` compares the candidate against the **pinned** oracle records. The live TS oracle leg (`run-oracle.mjs`, the 44 probes, `ts-remap-loader`) is removed or demoted to an explicit opt-in historical replay (worktree at the freeze SHA) that is never part of `npm run check`. Corpus bumps to v33 at the reconciliation commit.                                                                                                                                                          |
| C3  | Gate + script rework   | `check` becomes rust gates + reworked differential. `check:typescript` retires. Classify every `scripts/*.mjs`: remove TS-coupled checks (`check-architecture.mjs` + test; `check-nondeterminism.mjs` if TS-only; sandbox/Godot conformance runners — port the useful parts or retire with evidence); keep language-neutral repo hygiene (`check-doc-links`, `check-identity` with exclusion reconciliation, `check-public`, `check-project-context`, `check-rust-architecture`, `build-conformance-guests`). |
| C4  | Tree removal + sweep   | Delete `apps/**`, `packages/**`, `tests/behavior/**` (14 files), TS configs (`tsconfig*`, eslint, vitest; prettier scope per Q3), `workspaces` entries, TS npm scripts, lockfile churn. Completeness sweep: grep `packages/`, `apps/`, `npm run build`, `vitest`, `tsc` across `scripts/`, `.github/`, `crates/`, `docs/`, `schemas/`, `experiments/`, `fuzz/`, `tests/`.                                                                                                                                     |
| C5  | CI rework              | `rust.yml` and `tier1-evidence.yml` drop TS jobs/steps; differential stays in pinned mode; CodeQL disposition recorded.                                                                                                                                                                                                                                                                                                                                                                                       |
| C6  | Atomic doc advancement | Seven-surface pattern ([decision 33](../decisions/33-r12-disposition-execution.md) §3): `PROJECT_CONTEXT.md` (head + §2 "Current TypeScript reference"), `RUST_MIGRATION.md` (removal paragraph + table row), `ROADMAP.md`, `AGENTS.md`, `README.md`, `scripts/check-project-context.mjs` expectations, the resolution decision doc. Reconcile `ARCHITECTURE.md`, `docs/architecture/README.md`, `GOLDEN_TRACES.md` TS references.                                                                            |
| C7  | Honest trade-off       | The live differential drift-detection net (ADR 0033) is replaced by pinned records + freeze-SHA replay. Future Rust behavior changes require explicit corpus/expectation updates; there is no silent oracle backstop. This must be stated in the decision doc, not buried.                                                                                                                                                                                                                                    |
| C8  | Posture unchanged      | Nothing flips a typed `unavailable` effect; no new spawn path; no Rust behavior change beyond corpus/expectation pinning.                                                                                                                                                                                                                                                                                                                                                                                     |

## Acceptance criteria

| #   | Criterion                                                               | Check                                                                  |
| --- | ----------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| A1  | `npm run check` exit 0 on the removal tree (rust + pinned differential) | observed full-gate output at the removal commit                        |
| A2  | No live references to the removed tree                                  | grep sweep (C4) returns only documented historical/decision references |
| A3  | Pinned records equal the freeze-commit oracle output                    | digest equality vs the C1 audit artifacts                              |
| A4  | CI references only existing scripts                                     | workflow grep + `npm run check:docs`                                   |
| A5  | Doc surfaces advanced atomically                                        | `git show --stat` one commit                                           |

## Open questions (resolve at HITL)

1. Sandbox/Godot conformance runners (`scripts/sandbox/*`): port the
   still-useful parts to Rust-side conformance, or retire them with the
   TS tree and keep only the Rust differential + live probes?
2. Keep an opt-in historical replay command (worktree at the freeze SHA)
   or pinned records only?
3. Prettier/eslint retention scope for the remaining `.mjs`/`.yml`/`.md`
   surfaces.
4. `experiments/`, `schemas/`, `tests/domain-conformance/` disposition
   from the C4 sweep.

## Non-goals

- No fail-closed posture change, no new product capability, no Stage 4
  scope change, no Rust refactor beyond what pinning requires.
