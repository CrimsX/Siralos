# Decision — R10 Verified Promotion — What Closes R10 Active?

**Wayfinder ticket:** [R10 Verified Promotion — What Closes R10 Active?](../tickets/17-r10-verified-promotion.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R10c Entry Review](../decisions/16-r10c-entry-review.md) (PASS) + R10c implementation landed (differential parity 205/205 @ corpus v20, commits `ef51f87..a456afb`)
**Decided:** 2026-08-23 (resolver session, interactive review of the three sub-slice corpora, this session's full-gate outputs on worktree `a456afb`, and the seven status surfaces)
**Status:** **PASS — R10 Verified at executable worktree `a456afb71ab64c5504cd19e8eb7988d32d60a9dc`**
**Self-loop ledger:** 5 criteria, one implementation pass (verification below)

> Mirrors decisions 11/13: one atomic promotion closes the milestone;
> status advances only with executable evidence observed on the promoted
> tree.

---

## Summary

**Stage 3R R10 — H1 content identity, H2 determinism/replay, ICM context,
and H3 runtime-readiness parity — is Verified** as one milestone with
three internally ordered, individually entry-reviewed and landed
sub-slices:

| Sub-slice | Scope                                                                                                                                | Corpus         | Applicable required parity | Landing commits    |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------ | -------------- | -------------------------- | ------------------ |
| R10a      | H1 content identity + H2 determinism/replay (`siralos_core::identity` extended, `siralos_core::determinism`)                         | v18, 182 files | 177/177                    | `b5642b9..8868278` |
| R10b      | ICM phase contracts / dependency manifests / staleness / provenance (`siralos_core::context`)                                        | v19, 195 files | 190/190                    | `5fe5361..a2269cd` |
| R10c      | H3 runtime readiness: causal identity, budgets, supervision lifecycle, fault injection, fail-closed doctor (`siralos_core::runtime`) | v20, 210 files | 205/205                    | `ef51f87..a456afb` |

## 1. One atomic promotion — decision

The three sub-slices were authorized as one milestone ([decision 14](14-r10-entry-review.md));
each advanced the shared corpus only with its own reconciliation commit,
and the complete local gate was re-observed on the final assembled tree
(see §2). A single Verified promotion therefore closes R10 exactly as
decided in [R10 Scope](05-r10-scope.md) §1; no per-sub-slice Verified
statuses exist to reconcile.

## 2. Gate evidence observed on the promoted tree

All outputs below were observed this session on a clean worktree at
executable baseline `a456afb71ab64c5504cd19e8eb7988d32d60a9dc` (the
docs-only wayfinder commits after it do not alter executable behavior):

- `npm run check` — **exit 0**: prettier, lint, typecheck, TypeScript
  tests (**211/211 test files**), architecture checks, identity ratchet,
  Rust architecture check, and `check:rust-all`.
- `npm run check:differential` — **parity held: 205/205 applicable
  required scenarios** at schema 3, corpus version 20, 210 scenario
  files; **4 explicit platform skips** (POSIX-only `state-dir.*` and
  `user-config-symlink.posix` fixtures on the Windows host); **0 accepted
  informational deviations**.
- `cargo fmt --all --check` — clean; `cargo clippy --workspace
--all-targets --all-features -- -D warnings` — clean; `cargo test
--workspace` — green (487 `siralos-core` unit tests including the 20
  new runtime tests; the harness `strict_loader` test accepts the
  checked-in digest-bound corpus with 210 scenarios).
- Environment note: an initial gate attempt reported spawn-timeout flakes
  traced to a stale node process and heavy background CPU load from an
  unrelated application; after clearing the stale process and reducing
  background load, the identical tree passed the complete gate. The
  failures never reproduced in isolation and moved between unrelated
  spawn-heavy suites between attempts — environmental contention, not a
  behavioral deviation.

## 3. Atomic surface advancement (this commit)

The following surfaces advance together and now record R10 as Verified
with executable worktree `a456afb71ab64c5504cd19e8eb7988d32d60a9dc`:

1. `ROADMAP.md` — Stage 3R narrative + current-position statement.
2. `docs/development/PROJECT_CONTEXT.md` — status block, position table
   (`R10 COMPLETE`, `R11-R12 NOT DUE`), milestone table row, and
   verified-worktree fields.
3. `docs/development/RUST_MIGRATION.md` — milestone table rows R8/R9/R10
   corrected to Verified together (the R8/R9 rows had drifted) plus an
   R10 completion note.
4. `AGENTS.md` — current-implementation bullet and Current line.
5. `README.md` — Current status line.
6. Wayfinder map — Notes frontier and Decisions index (this decision).
7. This ticket — resolved by this decision.

## 4. R11 entry state and carried-forward classifications

- **R11 entry state:** R7 + R8/R9 + R10 (a-c) all Verified; harness
  schema 3 at corpus v20 (210 files, 205 applicable required parity,
  4 platform skips, 0 accepted informational deviations) — satisfying the
  entry condition recorded in [R11 Gate](06-r11-gate.md).
- Carried forward explicitly into the R11 record: the four
  POSIX-only platform skips above remain classified as explicit platform
  skips (never passes), and zero accepted informational deviations exist
  at v20.
- Effect-boundary hardening, run-directory management, sandbox live
  conformance, recovery orchestration, and the Tier-1 cross-platform
  audit remain **R11-owned** exactly as bounded by [R10
  Scope](05-r10-scope.md) §3 and [R11 Gate](06-r11-gate.md).

## 5. Downstream authorization

This promotion authorizes **nothing downstream**. It records closure.
R11 requires its own entry review freezing concrete differential/effect
subjects before any R11 code lands; Stage 4 remains gated behind
R11 + R12 per [Stage 4 Entry Sequence](08-stage4-entry-sequence.md).

---

## Self-loop verification

| Criterion                                              | Direct evidence                                                                                                         | Status |
| ------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------- | ------ |
| Single-milestone promotion matches the frozen contract | §1 cites decisions 14/05 sub-slice structure; three landed slices share one corpus lineage v18→v20                      | pass   |
| Gates observed on the promoted tree                    | §2 lists this session's `npm run check` exit 0 and differential 205/205 @ v20 on clean `a456afb` worktree               | pass   |
| Seven surfaces advance atomically                      | §3 enumerates them; each surface edited in the same documentation commit                                                | pass   |
| R11 entry state + skips inherited explicitly           | §4 restates the [R11 Gate] entry condition with v20 numbers and names the 4 platform skips / 0 informational deviations | pass   |
| No downstream authorization                            | §5 explicitly authorizes nothing beyond recording closure                                                               | pass   |

Evidence ladder: L1 observed gate outputs this session (quoted verbatim
in §2); L2 file:line citations across the seven surfaces in §3; L3
promotion precedent (decisions 11/13); L4 this decision.
