# Decision — R11 Entry Review — Freeze the Full-Parity Closure Contract

**Wayfinder ticket:** [R11 Entry Review — Freeze the Full-Parity Closure Contract Before Any Code](../tickets/18-r11-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R10 Verified Promotion](../decisions/17-r10-verified-promotion.md) (R10 Verified @ executable worktree `a456afb`)
**Decided:** 2026-08-23 (resolver session, interactive review of [R11 Gate](06-r11-gate.md) §1–§4, the TS reference surfaces `packages/core/src/{domain/failure,determinism/decisions,workspace,checkpoints,runtime}/**`, the Rust entry state at HEAD, and the current sandbox-conformance posture)
**Status:** **PASS — R11 contract frozen; R11 authorized as the next implementation slice**
**Self-loop ledger:** 5 criteria, one implementation pass (verification below)

> Mirrors decisions 10/12/14/15/16: contract frozen before any slice code.
> No Rust module lands, no corpus advances, no sandbox is claimed
> available.

---

## Summary

Freeze **R11 — full differential, effect-boundary, security, recovery, and
cross-platform parity** as ONE implementation slice with three ordered,
evidence-gated work items. R11 adds **two new differential subjects**
(`workspace-apply`, `recovery-taxonomy`) on top of carried-forward
subjects, runs the live sandbox conformance truthfully, and produces the
Tier-1 cross-platform audit at the final corpus version. Nothing flips a
fail-closed boundary operational.

## 1. Entry state

| Item                                             | Value                                                                                                                           |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------- |
| Branch / worktree                                | `main`, clean                                                                                                                   |
| Prior verified worktree                          | `a456afb71ab64c5504cd19e8eb7988d32d60a9dc` (R10 Verified)                                                                       |
| Corpus                                           | schema 3, version 20, 210 files, 205/205 applicable required parity, 4 platform skips, 0 accepted informational deviations      |
| Entry conditions ([R11 Gate](06-r11-gate.md) §1) | all satisfied: R7/R8/R9/R10 Verified; harness schema stable at 3; fail-closed posture unchanged (`check-project-context` green) |

## 2. Frozen subject schemas

### `workspace-apply` (new; name verbatim from [R11 Gate](06-r11-gate.md) §2.1)

> Implementation correction (R11.1 landing): this schema anticipated a
> digest/revision binding check BEFORE the unavailable outcome. The
> oracle's actual behavior is strictly stronger — the boundary refuses
> before ANY input inspection, so prepared-payload binding state can
> never change the outcome. The landed fixtures pin that reality.

Exercises the prepared-effect application boundary of the REAL TypeScript
reference (`packages/core/src/workspace/**` prepared-mutation/apply
contracts) against `siralos-adapters::workspace`. Scenarios declare
prepared effects (create/edit/delete/undo) bound to exact digests and
source revisions, plus protected-path targets, and assert:

- apply, undo application, and delete report the same typed
  `unavailable` outcome class on both sides with identical reason codes
- NO file is created, modified, or deleted on either host (the harness
  workdir stays byte-identical — asserted by construction: probes never
  receive write authority and records carry no mutation receipts)
- digest/revision binding is verified BEFORE the unavailable outcome
  (stale-bound prepared effects still refuse with the binding error,
  never silently)
- protected-path classification matches case-sensitively on POSIX and
  case-insensitively on Windows/macOS fixtures where applicable:
  `AGENTS.md` at any depth and `.siralos/**` are rejected before any
  availability question is asked

### `recovery-taxonomy` (new)

One scenario per failure dimension of [R11 Gate](06-r11-gate.md) §2.3,
driving ALREADY-PORTED typed surfaces on both sides and asserting the
stable machine-branchable codes:

| Dimension                 | TS oracle surface                                             | Rust surface                                                                  |
| ------------------------- | ------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| retryable / non-retryable | `packages/core/src/determinism/decisions.ts` retry policy     | `siralos_core::determinism::classify_retry`                                   |
| capability denied         | `packages/core/src/domain/failure.ts` `CAPABILITY_DENIED`     | `siralos_core::domain::failure`                                               |
| stale / conflict          | `…/failure.ts` `STALE_ACTIVATION`                             | same enum, R6 parity                                                          |
| resource exceeded         | `…/failure.ts` `RESOURCE_EXCEEDED{kind}`                      | same enum + R10c budget admission                                             |
| unavailable / unsupported | effect-boundary outcomes (`workspace-prepare`, Godot runners) | identical adapters                                                            |
| uncertain                 | restart reconciliation + checkpoint reconciliation classes    | `siralos_core::runtime::classify_incomplete_run`, checkpoint store inspection |
| cancelled                 | provider turn cancellation + domain `CANCELLED`               | R7.1 kernel + R6 enum                                                         |

No new recovery machinery: the subject proves code STABILITY across
implementations so Stage-4 recovery can branch on codes without substring
matching.

### Carried forward unchanged

`workspace-prepare` (prepared+cancelled), `checkpoint.*` (storage
inspection over the legacy metadata layout, invariant fail-closed,
creation unavailable), `git-inspection.*`, `godot-*` (zero spawn paths),
and every other existing subject remain required at their current
posture. The run-directory row of [R11 Gate](06-r11-gate.md) §2.1 stays
carried by the `godot-lsp`/`godot-diagnostics` unavailable scenarios — no
private directory is created even when a probe is requested.

## 3. Non-differential pillars (PASS artefacts)

- **Sandbox (§2.2):** run `npm run test:sandbox` live on this host and
  retain its output in the R11 promotion evidence: on Windows it must
  report skipped/setup-required LOUDLY — recorded as skip, never pass;
  Linux/macOS runs are expected only where the backend is installed, and
  an absent backend reports loudly unavailable (not secure).
- **Cross-platform (§2.4):** Tier-1 audit = per-platform migration-audit
  JSON (oracle/candidate/audit provenance, exact counts) produced by the
  SAME harness invocation on Linux, Windows, and macOS at the final
  corpus version, plus replay-stress determinism. This host can produce
  the Windows artifact directly; Linux/macOS artifacts require CI or an
  external Tier-1 runner and are named explicitly as the promotion's
  evidence gap until attached — a missing platform artifact blocks the
  R11 promotion rather than degrading it.

## 4. Ordering, corpus mechanics, measurement

- Ordered work items inside one slice: **R11.1** effect-boundary parity
  (`workspace-apply` + any checkpoint extension the implementation
  proves necessary) → **R11.2** `recovery-taxonomy` → **R11.3** sandbox +
  cross-platform audit artefacts → single R11 Verified promotion
  (seven-surface pattern, [R11 Gate](06-r11-gate.md) §4.4).
- Corpus advances when fixtures land, never at a review ([decision
  14](14-r10-entry-review.md) §4 rule): v21 lands with the last fixture
  commit; final numbering may stay v21 if slices share reconciliation.
- Measurement per [R11 Gate](06-r11-gate.md) §3: no benchmark without a
  measured hotspot; any `unsafe`/lock/dynamic-dispatch needs measured
  justification (the workspace forbids unsafe outright).

## 5. Authorization

**PASS — R11 authorized as the next implementation slice** against the
frozen contract above. This does NOT authorize flipping any `unavailable`
boundary operational, claiming Windows sandbox availability, skipping a
Tier-1 platform, or starting R12 disposition work (that gates on R11
Verified).

---

## Self-loop verification

| Criterion                                    | Direct evidence                                                                                                                                             | Status |
| -------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Subjects grounded in real reference surfaces | §2 names TS files and Rust modules per dimension; `DomainFailure`/`classify_retry` confirmed present on both sides this session                             | pass   |
| Effect-boundary honesty preserved            | `workspace-apply` asserts refusal-before-binding-check ordering and zero mutation; §2 carried-forward rows keep every other boundary at typed `unavailable` | pass   |
| Sandbox truthfulness                         | §3 requires live output retained, Windows = loud skip, absence = loudly unavailable, never secure                                                           | pass   |
| Cross-platform gap made explicit             | §3 names Linux/macOS artifacts as promotion-blocking evidence requiring CI/Tier-1 runners — not silently dropped                                            | pass   |
| Scope/ordering bounded                       | §4 freezes ordered items, corpus-at-landing rule, measurement discipline; §5 authorizes nothing downstream                                                  | pass   |

Evidence ladder: L1 observed entry state + module inventories; L2
citations ([R11 Gate] §1–§4, `failure.ts`, `decisions.rs:437`,
`siralos-core/src/domain/failure.rs:135`); L3 porting-gate precedent; L4
this decision.
