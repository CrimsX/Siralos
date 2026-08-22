# Decision — R9 Entry Review — Freeze the Godot Stage-3 Contract

**Wayfinder ticket:** [R9 Entry Review — Freeze the Godot Stage-3 Contract Before Any Code](../tickets/12-r9-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R8 Verified Promotion](../decisions/11-r8-verified-promotion.md) (Verified at worktree `c075b3c`)
**Decided:** 2026-08-22 (resolver session, interactive review of `ROADMAP.md` §3.9–3.11, `ARCHITECTURE.md:171-176` approved-mutations fail-closed status note + `ARCHITECTURE.md:530+` quality/review layering, `SECURITY.md:210-228`, the R8 vs R9 cut at `decisions/04-r8-r9-cut.md` §1, and the TypeScript reference inventory at HEAD `0273930`)
**Status:** **PASS — R9 contract frozen; R9 authorized as next implementation slice**
**Self-loop ledger:** 5 criteria, one implementation pass (verification below)

> Wayfinder **Plan, don't do** — this is a decision, not R9 code. No Rust
> mutation/impact/development module lands, no differential subject is added,
> no corpus version advances. It mirrors decision 10 (entry review before any
> slice code; contract frozen; implementation authorized).

---

## Summary

Freeze **R9 — Optional Godot Stage-3 parity** as three surfaces on top of R8's
static understanding: **review context & impact intelligence** (available,
purely derived), **typed scene/resource prepared mutation contracts**
(prepare-only; apply and new checkpoints stay typed `unavailable`), and the
**deterministic core of the unified `/develop` workflow** (routing, ordering,
change-set and consistency models). The interactive provider-facing `/develop`
session loop is not part of the R9 differential gate.

## 1. Entry state

| Item                    | Value                                                                                                                                                                                                                                       |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Branch / worktree       | `main`, clean at `0273930`                                                                                                                                                                                                                  |
| Prior verified worktree | `c075b3cf5e5240dd275a35cdc1a5a30c3bda9195` (R8 Verified)                                                                                                                                                                                    |
| Corpus                  | schema 3, **version 16, 155 scenario files**, 150/150 applicable required parity                                                                                                                                                            |
| R9 executable Rust      | absent — no mutation/impact/development modules exist under `crates/siralos-core/src/godot` or `crates/siralos-adapters/src/godot`                                                                                                          |
| Gates at this gate      | `cargo fmt --all --check` PASS · clippy `-D warnings` PASS · `cargo test --workspace --locked` PASS (657) · `check:architecture` PASS · `check:rust` PASS · `check:differential` 150/150 PASS (observed this session on the identical tree) |

## 2. Co-located classification table

| Behavior                                    | Classification                                  | Frozen boundary and owner                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ------------------------------------------- | ----------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Review context & impact intelligence        | **R9 MUST PORT**                                | Bounded evidence-backed `ReviewContextManifest` derivation — primary changes, related surfaces, inherited/instantiated impact, signal consumers/producers, test surfaces, autoload dependencies, regression areas, recommended validation with honest `runtime_evidence_unavailable` — derived from R8's read-only index + bounded diffs; available without execution (`ROADMAP.md` §3.9; ADR 0025; TS oracle `packages/core/src/godot/impact/{review-context,impact-analyzer}.ts`). Owner: `siralos-core::godot::impact` models + derivation policy. Differential proves manifest semantics incl. honest runtime-only classification.                                                                                                                                                                                                                         |
| Scene/resource prepared mutation contracts  | **R9 MUST PORT (prepare-only)**                 | Typed operations, immutable prepared mutations bound to the exact source revision with complete preview + digest-bound one-time approval model, deterministic structural serialization, post-apply reparse/semantic verification contract; **`apply` and new checkpoint creation remain typed `unavailable`** while directory-relative write primitives are unsound (`ARCHITECTURE.md:171-176`; `SECURITY.md:210-228`; ADR 0026; ROADMAP §3.10). Provider tools are prepare-only; a stale revision returns the typed stale reason and raw `.tscn`/`.tres` text-edit fallback is never offered. Owner: `siralos-core::godot::scene_mutation` (operations/prepared/serializer/verify models; TS `packages/core/src/godot/scene-mutation/*`) + `siralos-adapters::godot::scene_mutation` preparation/serialization/reparse verification over unavailable effects. |
| Unified `/develop` deterministic core       | **R9 MUST PORT**                                | Surface routing (script-only/native-only/bounded mixed), unified multi-target change sets with per-target revision/fingerprint/approval/verification retention, derived dependency-based apply ordering, checkpoint-then-apply batch posture revalidating every target before any write (the writes stay unavailable), per-surface verification classification (GDScript parser/fresh-LSP; native reparse/semantic), cross-surface consistency with honest runtime-only disclosures, structured blocked dispositions (`ROADMAP.md` §3.11; ADR 0027; TS oracle `packages/core/src/godot/development/*`). Owner: `siralos-core::godot::development` models/policies.                                                                                                                                                                                             |
| Interactive `/develop` session loop         | **GENERIC SEAM ONLY — later**                   | The provider-driven interactive loop composes pieces whose parity lands elsewhere (provider loop R7 Verified; runtime-readiness H3 → R10). R9's differential gate covers only the deterministic core above; the live session is exercised by integration tests, not frozen subjects.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| Apply effects / mirrors / new checkpoints   | **MUST PORT the behavior: prove `unavailable`** | Every entry point that would write returns the same typed outcomes on both sides with zero filesystem effects (`SECURITY.md:210-228`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| Raw text-edit fallback for scenes/resources | **DO NOT PORT**                                 | Never offered on either side.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Marketplace / auto-install of the domain    | **DO NOT PORT — lean freeze**                   | ADR 0036 unchanged.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |

## 3. Behavioral boundaries (citations)

- Prepare binds to the exact source revision; revision mismatch is a typed
  stale outcome, never an automatic re-read (`ADR 0026`;
  `packages/adapters/src/godot/scene-mutation/scene-mutation-service.ts`).
- Approval is modeled digest-bound and one-time; at R9 nothing consumable is
  ever approved because execution refuses first.
- Post-apply reparse verification is specified and unit-testable against the
  serializer even while apply itself is unavailable — parity covers the
  contract, not the effect.
- Review manifests never fabricate runtime evidence: unexecutable
  recommendations classify as `runtime_evidence_unavailable` on both sides.

## 4. Differential plan (frozen subject names)

- `godot-review-context` — manifest derivation over declared static models and
  bounded diffs; includes the honest `runtime_evidence_unavailable` matrix.
- `godot-mutation-prepare` — operation validation, prepared-mutation binding to
  revision + preview + digest; stale-revision typed reason; apply/checkpoint
  typed `unavailable` without effects.
- `godot-develop-plan` — deterministic `/develop` core: surface routing,
  dependency-based apply ordering, batch revalidation posture, verification
  and consistency classifications, blocked dispositions.

The real corpus version bump lands with the R9 implementation reconciliation,
not this decision. Fixtures need no engine binary and perform no writes.

## 5. Evidence assignment

| Boundary                                                | Evidence layer                                                            | Owner                               |
| ------------------------------------------------------- | ------------------------------------------------------------------------- | ----------------------------------- |
| Impact/review manifests                                 | Core unit + property tests + differential `godot-review-context`          | `siralos-core`                      |
| Prepared mutation contracts (bind/preview/digest/stale) | Core unit + adapter orchestration + differential `godot-mutation-prepare` | `siralos-core` / `siralos-adapters` |
| Routing/ordering/classification/dispositions            | Core unit + differential `godot-develop-plan`                             | `siralos-core`                      |
| Fail-closed apply/checkpoint reporting                  | Differential typed `unavailable` outcomes with zero effects               | Harness                             |

## 6. Security/architecture review (scoped PASS)

The frozen contract preserves the fail-closed posture by construction:
prepare-only tooling, refuse-before-effect application, no new checkpoints, no
mirrors, no spawn paths. `npm run check:rust` will enforce crate ownership and
core neutrality once `siralos_core::godot::{impact, scene_mutation,
development}` land; the existing FORBIDDEN_CORE_SYMBOL_PATTERN narrowing from
Commit hygiene (exact `pub mod godot;` allowance in `lib.rs`) already guards
the seam.

## 7. Measurement

Per `RUST_STYLE.md:568-589`: benchmarks appear only where a hot spot is
measured (candidates: structural serializer throughput, review-manifest
derivation over large indexes). None are speculative at freeze time.

## 8. Authorization

**PASS — R9 implementation is authorized as the next slice.** This does not
authorize R10+, Stage-4 entry, or a corpus promotion; the corpus bump lands
with the R9 reconciliation commit.

---

## Self-loop verification (this decision)

| Criterion                                                                   | Direct evidence                                                                                                                                                                                     | Status |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Three surfaces classified with owners + fail-closed rows                    | §2 table restates `decisions/04-r8-r9-cut.md` §1 R9 rows + adds the deterministic-workflow row with citations                                                                                       | pass   |
| Differential subjects grounded in deterministic-without-effects TS behavior | §4 names map to pure modules (`impact/*`, `scene-mutation/{operations,prepared,serializer,verify}`, `development/{unified-change-set,unified-order,cross-surface-consistency,blocked-disposition}`) | pass   |
| Entry state audited                                                         | §1 table observed this session (clean tree `0273930`, corpus v16/155, gates PASS)                                                                                                                   | pass   |
| Measurement discipline honored                                              | §7 evidence-first rule; no speculative benchmark                                                                                                                                                    | pass   |
| Authorization limited to R9                                                 | §8; R10+/Stage-4 explicitly untouched                                                                                                                                                               | pass   |

Evidence ladder: L1 observed entry state + TS inventory reads; L2 file:line
citations (ROADMAP §3.9–3.11, ARCHITECTURE 171/492/530, SECURITY 210-228);
L3 porting-gate precedent (decision 10); L4 this decision markdown.
