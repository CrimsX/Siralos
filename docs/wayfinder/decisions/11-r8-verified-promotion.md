# Decision — R8 Verified Promotion — What Closes R8 Active?

**Wayfinder ticket:** [R8 Verified Promotion — What Closes R8 Active?](../tickets/11-r8-verified-promotion.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R8 Entry Review](../decisions/10-r8-entry-review.md) (PASS — six surfaces + five differential subjects frozen)
**Decided:** 2026-08-22 (resolver session, evidence sweep against the frozen contract at worktree `c075b3cf5e5240dd275a35cdc1a5a30c3bda9195`)
**Status:** **R8 Verified — promotion executed atomically on this decision's evidence**
**Self-loop ledger:** 5 criteria, one implementation pass (verification below)

> Mirrors the R7 precedent (`02-r7-verified-promotion.md` §2): the promotion is
> one commit advancing every status surface; a partial advance is drift.

---

## Summary

**R8 — Optional Godot Stage-2 parity is Verified** at executable worktree
`c075b3cf5e5240dd275a35cdc1a5a30c3bda9195`: all six frozen surfaces are ported,
all five frozen differential subjects hold required parity at corpus version 16,
and the fail-closed posture is mechanically intact. R8 Verified does **not**
authorize R9 — R9 requires its own entry review.

## 1. Observed gate artefacts on the verified worktree

| Artefact                                                                        | Observed result                                                                                                                          |
| ------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `cargo fmt --all --check`                                                       | PASS                                                                                                                                     |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | PASS                                                                                                                                     |
| `cargo test --workspace --all-targets --all-features --locked`                  | PASS — 204 adapters + 25 conformance + 59 cli (1 ignored) + 369 core = 657 passed                                                        |
| `npm run check:architecture`                                                    | "Architecture check passed."                                                                                                             |
| `npm run check:rust`                                                            | "Rust architecture check passed." (core domain neutrality incl. narrowed `lib.rs` allowance, cli → adapters → core)                      |
| `npm run check:differential`                                                    | **parity held: 150/150 applicable required scenarios, 4 explicit platform skips, 0 deviations — corpus schema 3, version 16, 155 files** |

Local observation supersedes the R7-era EPERM caveat; Tier-1 CI remains the
audit mechanism of record.

## 2. Frozen-contract reconciliation (`10-r8-entry-review.md`)

| Frozen row                            | Implementation evidence                                                                                                                                                                                                                                                                              |
| ------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Engine discovery & profiling          | `siralos-core::godot::{selection, installations, capabilities, engine_profile, compatibility}` + `siralos-adapters::godot::{discovery/*, profile/engine_profiler}`; deterministic selection with recorded rationale; `.app` bundle + PATH + configured candidates; differential `godot-discovery` ×4 |
| Recovery contracts (fail-closed)      | `siralos-adapters::godot::process::recovery_runner` — typed unavailable, no mirror, no launch                                                                                                                                                                                                        |
| Version-bound API knowledge           | `siralos-core::godot::{knowledge, api}` + `siralos-adapters::godot::knowledge::{api_dump, api_index, service}`; deterministic symbol identities; differential `godot-knowledge` ×5                                                                                                                   |
| GDScript check-only contracts         | `siralos-core::godot::gdscript` digests/preparation + `siralos-adapters::godot::{diagnostics/*, process/godot_check_only_runner}`; `--script`↔`--check-only` pairing structurally tested; differential `godot-diagnostics` ×4                                                                        |
| Bounded LSP                           | `siralos-adapters::godot::lsp::{frame_parser, json_rpc, file_uri, port_allocator, normalizers, service}` + `siralos-core::godot::lsp`; recovery-tuple pairing tested; differential `godot-lsp` ×4                                                                                                    |
| Read-only scene/resource intelligence | `siralos-core::godot::scene` bounded parsers/Variant/tree/index + `siralos-adapters::godot::scene::service`; differential `godot-scene-resolve` ×5 (incl. the three selection-rule divergence pins)                                                                                                  |
| Probe invocation proves `unavailable` | Differential knowledge/diagnostics/lsp scenarios assert typed `unavailable` outcomes produced by the production service stacks on both implementations                                                                                                                                               |

Fail-closed sweep: `rg 'std::process::Command|\.spawn\(' crates/{core,adapters}/src/godot`
returns **zero matches** — no Godot module launches a process or creates
mirrors/probe directories. Argument discipline is enforced by
`npm run check:architecture` plus runner unit tests, not comments.

Measurement per `RUST_STYLE.md:568-589`: no speculative benchmark added; the
differential harness itself pins scene-parse semantics, and no measured hot
spot motivated a benchmark in this slice.

## 3. Atomic status-surface advancement

One promotion commit advances: `PROJECT_CONTEXT.md` fenced head + Stage-3R
table + stage block, `RUST_MIGRATION.md` R8 Verified section, `ROADMAP.md`
Stage-3R bullet + Current tail, `README.md` status badge + Current bullet,
`AGENTS.md` current-implementation paragraph + intended-direction Current
line, `scripts/check-project-context.mjs` metadata ratchet (+ its test), and
the Wayfinder map Notes/Decisions index. New verified pointer:
`Last verified commit` = `Latest verified executable worktree` =
`c075b3cf5e5240dd275a35cdc1a5a30c3bda9195`.

## 4. R9 is not authorized

R9 (prepared mutation, review context & impact intelligence, unified
`/develop`) waits on its own entry review mirroring `10-r8-entry-review.md`.
Nothing here satisfies, scopes, or schedules R10+ or Stage-4 entry.

---

## Self-loop verification (this decision)

| Criterion                                                       | Direct evidence                                                                                          | Status |
| --------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | ------ |
| Six surfaces ported with owners matching §5 of the entry review | File inventory above; `check:rust` crate-direction + neutrality PASS                                     | pass   |
| Five frozen subjects at required parity, corpus v16             | Manifest counts (discovery 4 / knowledge 5 / diagnostics 4 / lsp 4 / scene-resolve 5); audit 150/150     | pass   |
| Fail-closed posture mechanically intact                         | Zero spawn-path grep hits; typed `unavailable` differential outcomes from production services both sides | pass   |
| Full gate set observed PASS on one worktree                     | §1 table, all commands run locally on clean `c075b3c` tree                                               | pass   |
| Promotion atomicity + non-authorization of R9                   | §3 surface list advanced in one commit; §4                                                               | pass   |

Evidence ladder: L1 observed gates + manifest counts on the verified worktree;
L2 fail-closed grep sweep; L3 porting-gate precedent (`02-r7-verified-promotion.md`);
L4 this decision.
