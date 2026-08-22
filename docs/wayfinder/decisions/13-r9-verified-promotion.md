# Decision — R9 Verified Promotion — What Closes R9 Active?

**Wayfinder ticket:** [R9 Verified Promotion — What Closes R9 Active?](../tickets/13-r9-verified-promotion.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R9 Entry Review](../decisions/12-r9-entry-review.md) (PASS — three surfaces + `godot-review-context` / `godot-mutation-prepare` / `godot-develop-plan` frozen)
**Decided:** 2026-08-22 (resolver session, evidence sweep against the frozen contract at worktree `1623e800f8034d07825d7c6582768c27a91a973e`)
**Status:** **R9 Verified — promotion executed atomically on this decision's evidence**
**Self-loop ledger:** 5 criteria, one implementation pass (verification below)

> Mirrors `11-r8-verified-promotion.md`: one commit advances every status
> surface; a partial advance is drift.

---

## Summary

**R9 — Optional Godot Stage-3 parity (deterministic core) is Verified** at
executable worktree `1623e800f8034d07825d7c6582768c27a91a973e`: all three
frozen surfaces are ported, all three frozen differential subjects hold
required parity at corpus version 17, and the fail-closed posture is
mechanically intact. R9 Verified does **not** authorize R10 — R10 requires
its own entry review (`decisions/05-r10-scope.md`).

## 1. Observed gate artefacts on the verified worktree

| Artefact                                                                        | Observed result                                                                                                                          |
| ------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `cargo fmt --all --check`                                                       | PASS                                                                                                                                     |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | PASS                                                                                                                                     |
| `cargo test --workspace --all-targets --all-features --locked`                  | PASS — 207 adapters (+3 scene-mutation) + 25 conformance + 59 cli (1 ignored) + 400 core = **691 passed**                                |
| `npm run check:architecture`                                                    | "Architecture check passed."                                                                                                             |
| `npm run check:rust`                                                            | "Rust architecture check passed."                                                                                                        |
| `npm run check:differential`                                                    | **parity held: 162/162 applicable required scenarios, 4 explicit platform skips, 0 deviations — corpus schema 3, version 17, 167 files** |

## 2. Frozen-contract reconciliation (`12-r9-entry-review.md`)

| Frozen row                                 | Implementation evidence                                                                                                                                                                                                                                                                                                                                                                                                |
| ------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Review context & impact intelligence       | `siralos_core::godot::impact` (typed vocabularies, boundary validator with oracle-exact messages, bounded breadth-first analyzer over an injected sync relationship source); differential `godot-review-context` ×4 covering verified graphs, stale exclusion, autoload global-reach disclosure, and depth-bound classification                                                                                        |
| Scene/resource prepared mutation contracts | `siralos_core::godot::scene_mutation` (11 typed operations, oracle-exact validation, semantic expectations with resolved add-node paths, fingerprint over canonical JSON of the exact binding tuple) + `siralos-adapters::godot::scene_mutation` prepare-only orchestration whose apply is typed `Unavailable` before any effect; differential `godot-mutation-prepare` ×4 including a traversal-path rejection record |
| Unified `/develop` deterministic core      | `siralos_core::godot::development` (surface routing with faithful `\b` signal emulation, cross-target dependency edges, topological order with path tie-break and cycle rejection, structured blocked dispositions); differential `godot-develop-plan` ×4 incl. dependency-over-path ordering, unresolved references, and empty-target routing                                                                         |
| Interactive `/develop` session loop        | GENERIC SEAM ONLY per the entry review — not part of this slice; unchanged                                                                                                                                                                                                                                                                                                                                             |
| Apply effects / mirrors / new checkpoints  | Prove `unavailable`: adapter `apply` returns the single typed outcome before any effect; zero spawn/write paths in any new module                                                                                                                                                                                                                                                                                      |

Fail-closed sweep: `rg 'std::process::Command|\.spawn\(' crates/{core,adapters}/src/godot`
returns **zero matches**. Measurement per `RUST_STYLE.md:568-589`: no hot
spot was measured in this slice, so no speculative benchmark was added.

One repair landed during implementation: the `.gd` request-signal matcher
initially tested the word boundary only at the maximal path-class run start;
the oracle tries every run start, so "res://enemy.gd" failed to route. Fixed
at root cause with the mixed/script-only fixtures pinning both behaviors.

## 3. Atomic status-surface advancement

One promotion commit advances: `PROJECT_CONTEXT.md` fenced head + Stage-3R
table + stage block, `RUST_MIGRATION.md` R9 Verified section, `ROADMAP.md`
Stage-3R bullet + Current tail, `README.md` status badge + Current bullet,
`AGENTS.md` current-implementation paragraph + intended-direction Current
line, `scripts/check-project-context.mjs` metadata ratchet (+ its test), and
the Wayfinder map Notes/Decisions index. New verified pointer:
`Last verified commit` = `Latest verified executable worktree` =
`1623e800f8034d07825d7c6582768c27a91a973e`.

## 4. R10 is not authorized

R10 (H1/H2/ICM + H3 runtime-readiness) waits on its own entry review
freezing corpus names against `decisions/05-r10-scope.md`. Nothing here
satisfies, scopes, or schedules R11, R12, or Stage-4 entry.

---

## Self-loop verification (this decision)

| Criterion                                                      | Direct evidence                                                                                | Status |
| -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ------ |
| Three surfaces ported with owners matching the entry review §5 | File inventory above; adapters ownership row now materialized (`scene_mutation` orchestration) | pass   |
| Three frozen subjects at required parity, corpus v17           | Manifest counts (review-context ×4 / mutation-prepare ×4 / develop-plan ×4); audit 162/162     | pass   |
| Fail-closed posture mechanically intact                        | Zero spawn-path grep hits; adapter `apply` single typed-unavailable variant                    | pass   |
| Full gate set observed PASS on one worktree                    | §1 table, commands run on clean `1623e80` tree                                                 | pass   |
| Promotion atomicity + non-authorization of R10                 | §3 surface list advanced in one commit; §4                                                     | pass   |

Evidence ladder: L1 observed gates + manifest counts on the verified
worktree; L2 fail-closed grep sweep; L3 porting-gate precedent
(`02-r7-verified-promotion.md`, `11-r8-verified-promotion.md`); L4 this
decision.
