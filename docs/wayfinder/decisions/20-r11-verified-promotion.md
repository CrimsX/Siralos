# Decision — R11 Verified Promotion — What Closes R11 Active?

**Wayfinder origin:** [R11.3 Tier-1 Repair Register](../tickets/19-r11-tier1-repair-register.md)
Definition of done + [R11 Gate](06-r11-gate.md) §2/§4
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R11 Entry Review](18-r11-entry-review.md) (PASS) + R11.1/R11.2
landed (`workspace-apply`, `recovery-taxonomy`; corpus v23, 222 files,
217/217 applicable required parity) + Tier-1 closure (this session)
**Decided:** 2026-08-24 (resolver session, interactive HITL decisions on the
Tier-1 register, inspection of the three-platform dispatch artefacts, and
this session's full-gate outputs on the promotion tree)
**Status:** **PASS — R11 Verified at executable worktree
`eea0029e70aae7248b3e1022c3be1cb669fd5a09`**
**Self-loop ledger:** 5 criteria, one implementation pass plus one
environmental rerun (verification below)

> Mirrors decisions 11/13/17: one atomic promotion closes the milestone;
> status advances only with executable evidence observed on the promoted
> tree.

---

## Summary

**Stage 3R R11 — full differential, effect-boundary, security, recovery,
and cross-platform parity — is Verified.** R11.1 (`workspace-apply`
refusal-before-binding + protected paths) and R11.2 (`recovery-taxonomy`
over the seven typed failure dimensions) landed at corpus **version 23,
222 scenario files**, and R11.3 closed the Tier-1 matrix: every
TypeScript Stage-3 surface now has a Rust differential subject at parity
on the same verified worktree, the fail-closed boundaries report typed
`unavailable` at parity, sandbox conformance reports its true state on
all three platforms, and the six cross-platform findings are closed
without weakening any deny list or unavailable posture.

## 1. Tier-1 three-platform evidence

The `tier1-evidence.yml` dispatch at
`eea0029e70aae7248b3e1022c3be1cb669fd5a09` returned **three green
platforms**. All six artefacts are retained with matching provenance in
[tests/differential/evidence/r11/EVIDENCE.md](../../../tests/differential/evidence/r11/EVIDENCE.md):

| Platform | Audit                                                  | Sandbox conformance                                               |
| -------- | ------------------------------------------------------ | ----------------------------------------------------------------- |
| Linux    | parity held, 217/217 applicable required, 0 deviations | loud skip (`dependency-missing`: rg/bwrap/socat absent)           |
| macOS    | parity held, 217/217 applicable required, 0 deviations | loud skip (33 probes; private run-directory creation unavailable) |
| Windows  | parity held, 217/217 applicable required, 0 deviations | loud skip (`setup-required`)                                      |

All three audits carry the identical `sourceTreeSha256` (`ad1e4963…`,
1264 source files), corpus digest `50c0575f…`, schema 3, and the
retained Windows TypeScript-gate record shows **211 test files / 3231
tests passed, 8 skipped** at the same commit.

## 2. Gate evidence observed on the promoted tree

This session, on the worktree whose executable content equals `eea0029`
(the commits after it are documentation-only):

- TypeScript chain — format check, lint, typecheck pass; full vitest
  suite first attempt reported **16 failures, every one a Vitest
  default-timeout (5000 ms)** in pre-existing CLI/Git suites under
  sustained background CPU load; the five affected files passed
  **138/138 in isolation** (each sub-second), reproducing the
  environmental-contention pattern recorded in [decision
  17](17-r10-verified-promotion.md) §2 — the failures never reproduced
  in isolation and moved between unrelated spawn-heavy suites.
  `check-project-context.test.mjs` (edited this session) passed in both
  attempts. The remaining chain steps (architecture, nondeterminism,
  identity, public hygiene, doc links, project context) all pass.
- `check:differential` — **parity held: 217/217 applicable required
  scenarios** at schema 3, corpus v23, with **4 explicit platform skips**
  and **0 accepted informational deviations**; the fresh local audit is
  internally byte-stable (reference == candidate record digests) and its
  per-commit binding differs from the dispatch copy only because the
  audit is commit-bound by design (ADR 0033).
- `check:rust-all` — Rust architecture check clean; differential green
  (above); `cargo fmt --check` clean; `cargo clippy --workspace
--all-targets --all-features --locked -- -D warnings` clean;
  `cargo test --workspace` green (489 `siralos-core` unit tests, 207
  adapter tests, 59 CLI tests, 25 domain-conformance tests).
- One tooling exclusion added this session: `.prettierignore` now skips
  generated differential-harness protocol documents
  (`tests/differential/out/`, `tests/differential/evidence/**/*.json`)
  — retained evidence must stay byte-identical to the producing runner
  upload, so formatting it would falsify provenance (same principle as
  the already-ignored `package-lock.json`). No check standard was
  weakened.

## 3. Atomic surface advancement (this commit)

The following surfaces advance together and now record R11 as Verified
with executable worktree `eea0029e70aae7248b3e1022c3be1cb669fd5a09`:

1. `ROADMAP.md` — Stage 3R current-position statement.
2. `docs/development/PROJECT_CONTEXT.md` — status block, position table
   (`R11 COMPLETE`), milestone table row, and verified-worktree fields.
3. `docs/development/RUST_MIGRATION.md` — R11 table row + completion
   paragraph.
4. `AGENTS.md` — current-implementation bullet and Current line.
5. `README.md` — Current status line.
6. `scripts/check-project-context.mjs` + test — "R11 COMPLETE"
   expectation bump.
7. Wayfinder map — Notes frontier (R12 disposition next), Decisions
   index ([R11.3 Tier-1 Repair Register] closed; this decision).

## 4. Finding #5 disposition (HITL option (a))

The macOS `SSH_AUTH_SOCK` finding is resolved as an **accepted
deviation**: GitHub macOS runners inject an ssh-agent socket that the
sandbox wrapper forwards; Siralos's fail-closed refusal of denied
variables is correct behavior. The workflow unsets only the CI-injected
variable (`aa13128`) so the live gates can execute; the deny list is
unchanged and remains unit-covered, and the macOS live suite then skips
loudly on the typed private-run-directory boundary. Recorded here and in
EVIDENCE.md; never a deny-list weakening.

## 5. Downstream authorization

This promotion authorizes **only** the [R12 Disposition] work: the
retirement-vs-retention decision over the TypeScript reference, which
requires its own HITL grilling per the frozen template in [R12
Disposition](07-r12-disposition.md). Stage 4 remains gated behind R12 +
the stage4-entry-gate per [Stage 4 Entry Sequence](08-stage4-entry-sequence.md).
No unavailable effect was made operational to close this gate.

---

## Self-loop verification

| Criterion                                         | Direct evidence                                                                                                                             | Status |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Three-platform Tier-1 matrix green with artefacts | §1: dispatch `eea0029` audits parity-held 217/217 ×3 with identical source-tree digest; artefacts inspected field-by-field before retention | pass   |
| Full local gate on the promoted tree              | §2: TypeScript chain green (with documented environmental isolation rerun), differential 217/217 @ v23, fmt/clippy/tests green              | pass   |
| Seven surfaces advance atomically                 | §3 enumerates them; each edited in the same documentation commit                                                                            | pass   |
| #5 resolved without weakening posture             | §4: option (a); deny list untouched; loud skips retained                                                                                    | pass   |
| No downstream authorization beyond R12            | §5 explicitly bounds what this promotion starts                                                                                             | pass   |

Evidence ladder: L1 inspected dispatch artefacts (§1) and this session's
gate outputs (§2, quoted verbatim); L2 file:line citations across the
surfaces in §3; L3 promotion precedent (decisions 11/13/17); L4 this
decision.
