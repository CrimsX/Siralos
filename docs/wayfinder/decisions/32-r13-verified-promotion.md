# Decision — R13 Verified Promotion — What Closes R13 Active?

**Wayfinder origin:** [R13 Execution Register](../tickets/22-r13-execution-register.md)
Definition of done + [R13 Continuation Contract](21-r13-remaining-surface-parity-entry-review.md) §4
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R13.1](../decisions/22-r13-1-entry-review.md) PASS + R13.2 PASS + R13.3 PASS + R13.4 PASS + R13.5a-d PASS
landed (`cli-session` closure at corpus v31, 231/231 applicable required parity) + local full-gate (this session)
**Decided:** 2026-08-27 (resolver session, inspection of the 231/231 local audit, this session's full-gate outputs, and the five-slice landing record)
**Status:** **PASS — R13 Verified at executable worktree `72e20be`**
**Self-loop ledger:** 5 criteria, one implementation pass plus one lint/format repair (verification below)

> Mirrors decisions 11/13/17/20: one atomic promotion closes the milestone;
> status advances only with executable evidence observed on the promoted
> tree.

---

## Summary

**Stage 3R R13 — Remaining TypeScript Surface Parity — is Verified.** All five slices landed:

- **R13.1** Host introspection & authority (`security-permissions` ×10, `command-catalog` ×5, `capability-doctor` ×5) at v24
- **R13.2** Workspace guidance (`instructions-resolution` ×7, `knowledge-revisions` ×7) at v25
- **R13.3** External knowledge boundaries (`reference-identity` ×10, `research-policy` ×10) at v26
- **R13.4** Planning & briefing (`planning-runtime` ×10, `executor-brief` ×10, synthetic) at v27
- **R13.5a** Slash-dispatch core (`cli-session` ×6) at v28
- **R13.5b** Briefing-service + real S3M manifests (`executor-brief` ×8) at v29
- **R13.5c** Deferred seams (`knowledge-revisions` seeding ×3, `reference-identity` access ×4, `research-policy` access ×4) at v30
- **R13.5d** Full `cli-session` closure (`cli-session` ×7: godot ×9, gdscript ×7, develop ×5, system ×8, queue ownership, sanitizer, ordering) at v31

Corpus **version 31, 236 scenario files**, local audit **231/231 applicable required scenarios** (4 platform skips, 0 informational) with byte-identical reference/candidate records. Fail-closed postures remain `unavailable` (zero new operational effects); every new case is exercised through the real TypeScript composition with scripted queues and the Rust candidate behind the same subject names.

## 1. Local audit evidence

On executable worktree `72e20be` (which includes the `1898a50` R13.5d landing plus two probe/lint repairs that do not change Rust product behavior):

- `npm run check:differential` — **parity held: 231/231 applicable required** at schema 3, corpus v31, with **4 explicit platform skips** and **0 informational deviations**; fresh local audit is internally byte-stable and commit-bound.
- `cargo fmt --check` clean; `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean; `cargo test --workspace` green (508 `siralos-core`, 207 `siralos-adapters`, 26 `siralos-cli`, 25 domain-conformance).
- `npm run format:check` clean; `npm run lint` clean (after removal of the now-unused `sessionInfoFor`/`cliApp` and stub imports); `npm run typecheck` clean; `npm test` (Vitest) green.

No Tier-1 cross-platform dispatch is required for R13 (local differential is the gate; R11 already closed the Tier-1 matrix). The Windows local audit is the promotion evidence; Linux/macOS parity is expected to hold by construction (deterministic, no platform-specific branches beyond the existing 4 skips).

## 2. Gate evidence observed on the promoted tree

This session, on the worktree whose executable content equals `72e20be`:

- TypeScript chain — `format:check`/`lint`/`typecheck` pass; `npm test` green; `check:architecture`/`check:identity`/`check:public`/`check:docs`/`check:context` pass.
- `check:differential` — **231/231** as above.
- `check:rust-all` — architecture clean, differential green, `fmt` clean, `clippy` clean, `test` green.

No check standard was weakened; the two probe/lint commits (`cfdf746` prettier, `27a361f`/`72e20be` unused-import removal) are non-behavioral for the Rust product (oracle stubs remain byte-identical to the candidate).

## 3. Atomic surface advancement (this commit)

The following surfaces advance together and now record **R13 as Verified** with executable worktree `72e20be` (and `1898a50` as the last executable landing before formatting/lint repairs):

1. `ROADMAP.md` — Stage 3R current-position statement (R13 Verified).
2. `docs/development/PROJECT_CONTEXT.md` — status block, position table (`R13 COMPLETE`), milestone table row, and verified-worktree fields (`72e20be` / `1898a50`).
3. `docs/development/RUST_MIGRATION.md` — R13 table row + completion paragraph.
4. `AGENTS.md` — current-implementation bullet and Current line (R13 Verified).
5. `README.md` — Current status line (R13 Verified).
6. `scripts/check-project-context.mjs` + test — `R13 COMPLETE` expectation bump and `Last verified commit` bump.
7. Wayfinder map — Notes frontier (R12 disposition next), Decisions index (this decision), and `22-r13-execution-register.md` closed.

All seven are edited in the same documentation commit; no executable Rust or TypeScript product code is changed in the promotion commit.

## 4. Downstream authorization

This promotion closes **R13** and unblocks **only** the [R12 Disposition] work: the retirement-vs-retention decision over the TypeScript reference, which requires its own HITL grilling per the frozen template in [R12 Disposition](07-r12-disposition.md) (amended by [R13 Continuation Contract](21-r13-remaining-surface-parity-entry-review.md) §4 to require `R1–R11 + R13 Verified`). Stage 4 remains gated behind R12 + the `stage4-entry-gate` per [Stage 4 Entry Sequence](08-stage4-entry-sequence.md). No unavailable effect was made operational to close this gate.

---

## Self-loop verification

| Criterion                              | Direct evidence                                                                                                                                                      | Status |
| -------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| All five R13 slices landed with parity | § Summary: v24–v31 landings, 231/231 local audit, 4 skips, 0 informational; `corpus/manifest.json` v31, `harness.rs` `CORPUS_VERSION 31`                             | pass   |
| Full local gate on the promoted tree   | §1–2: `format:check`/`lint`/`typecheck`/`npm test`/`check:architecture`/`check:identity`/`check:rust-all`/`check:differential` 231/231 green on `72e20be`            | pass   |
| Seven surfaces advance atomically      | §3 enumerates them; each edited in the same documentation commit                                                                                                     | pass   |
| Fail-closed posture preserved          | Zero new operational effects; all `unavailable`/`ask`/`deny` matrices still typed `unavailable` (grep `unavailable` in `harness_cli_session.rs` still 0 spawn paths) | pass   |
| No downstream authorization beyond R12 | §4 explicitly bounds what this promotion starts                                                                                                                      | pass   |

Evidence ladder: L1 inspected local audit (`audit.json` 231/231) and this session's gate outputs (quoted verbatim); L2 file:line citations across the surfaces in §3; L3 promotion precedent (decisions 11/13/17/20); L4 this decision.
