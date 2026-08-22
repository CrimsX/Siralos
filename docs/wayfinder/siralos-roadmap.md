# Wayfinder Map — Siralos Roadmap (Stage 3R R7 → R12 → Stage 4 Entry)

## Destination

A verified, decision-ready route from the current R7.5 CLI candidate (pending independent review) through **R7 Verified** to the **R12 retirement/retention decision**, making Stage 4–6 scope and entry conditions unambiguous so any executor can take the next ticket without rediscovering authority, parity, or lean-product boundaries.

The map is done when no decision remains before an executor could: (1) promote R7.5 through independent review, (2) sequence R8–R11 with frozen differential subjects, and (3) state the exact R12 evidence that opens Stage 4. The destination is a decision/thin spec, not the implementation itself.

## Notes

- Domain: Siralos — Stage 3R migration (TypeScript oracle → Rust candidate), lean product model ADR 0036.
- Current position: R1–R7 Verified, R8 **Verified** at `c075b3cf5e52` / `baeb447` (six Godot Stage-2 surfaces ported and evidence-backed; corpus v16 155 files, 150/150 applicable required scenarios across all five frozen `godot-*` subjects; fail-closed posture mechanically preserved). R9 entry-reviewed (`12-r9-entry-review.md` PASS) — R9 is the next authorized slice; R10+ not due. See `docs/development/PROJECT_CONTEXT.md`, `docs/development/RUST_MIGRATION.md`, `ROADMAP.md`, `docs/wayfinder/decisions/12-r9-entry-review.md`.
- Skills every session should consult: **self-loop** (+ `.agents/skills/self-loop/references/verification-protocol.md`), `docs/development/RUST_STYLE.md`, `docs/development/R7_BEHAVIOR_EXTRACTION.md` §14, `ARCHITECTURE.md` § Rust candidate, `AGENTS.md` workspace/security rules. **Standing rule: self-loop on every prompt** — every future prompt (human or follow-up) must invoke the `self-loop` skill and its `references/verification-protocol.md` ledger (criterion → evidence → pass/fail/unknown → challenge → repair), even when Wayfinder says one ticket per session. Skill load is per-prompt; loop budget is one coherent pass + up to two repairs unless the prompt explicitly changes budget.
- Standing preferences: Wayfinder **Plan, don't do** — tickets resolve decisions, not deliverables, unless Notes explicitly carries execution into the map. One ticket per session (research may parallel). Reference closed tickets by **name**, never bare id. Lean discipline: profiles/context/skills/plugins are declarative composition — do not pull multi-agent machinery, general Hooks, TaskGraph, workflow engines, marketplaces, or automatic acquisition forward (ADR 0036).
- Tracker: **local-markdown** fallback (no issue-tracker binding configured — run `/setup-matt-pocock-skills` if a hosted tracker is desired). The map lives at `docs/wayfinder/siralos-roadmap.md`; tickets live under `docs/wayfinder/tickets/`. This is the canonical artifact (label `wayfinder:map` in hosted trackers).

## Decisions so far

<!-- one line per closed ticket: gist + link; map never restates the ticket body -->

- [R7.5 Review Rubric — When Is the /context + /tools Candidate PASS?](decisions/01-r7-5-review-rubric.md) — 5-section PASS rubric frozen (byte-equal strings, 16 CLI tests + 11 differential scenarios, sanitizer advisory, flag scope, executable checklist); `cargo fmt/clippy/test` + `check:architecture/rust` observed PASS in DSH partial sandbox.
- [R7 Verified Promotion — What Closes R7 Active?](decisions/02-r7-verified-promotion.md) — DoD frozen for R7A+7.1-7.5 (corpus v12/104→v13/120→v15/133, gates fmt/clippy/test/check:rust/arch/differential 128/128), 7-surface atomic promotion table, R8 needs entry-review (not immediate), lean Stage 4 guardrail, executable promotion checklist.
- [Godot Boundaries Research — What siralos-core Isolation Must Protect for R8/R9](decisions/03-godot-boundaries.md) — fact sheet with 4 enforcement tables (domain neutrality via FORBIDDEN_CORE_SYMBOL_PATTERN, R4 hardening, adapter-vs-core ownership, ADR 0036 lean quotes) at file:line; `cargo fmt` + `check:rust/arch` observed PASS.
- [R8 vs R9 Cut — Which Godot Parity Ships in Which Slice?](decisions/04-r8-r9-cut.md) — frozen two-row table: 6 R8 surfaces (discovery, recovery, knowledge, check-only, bounded LSP, read-only scenes) + 3 R9 surfaces (prepared mutation, review/impact, unified workflow), each with fail-closed posture + Not-in-R8/R9 list (no placeholder domains/marketplaces/auto-install); `fmt`/`check:rust/arch` PASS, no code written.
- [R10 Scope — H1/H2/ICM + H3 Runtime-Readiness Parity Slice Shape](decisions/05-r10-scope.md) — one Verified milestone with 3 ordered entry-reviewed sub-slices (R10a H1+H2, R10b ICM, R10c H3) justified by `H1→H2→ICM→H3` hard dependencies; 14 proposed differential subjects with core seams (`siralos-core::identity/determinism/context/runtime`); R11 boundary explicit (effect-boundary/run-directory/security/recovery/cross-platform stay R11); `fmt`/`check:rust/arch` PASS.
- [R11 Gate — Full Differential, Effect-Boundary, Security, Recovery, Cross-Platform Closure Criteria](decisions/06-r11-gate.md) — entry on R7+R8/R9+R10a-c Verified + harness schema 3; 4 pillars (effect-boundary typed `unavailable`, security truthful `test:sandbox`, typed recovery 7 dims, Tier-1 cross-platform digest-bound audit) + RUST_STYLE measurement discipline + PASS corpus/7-surface artefacts; `fmt`/`check:rust/arch` PASS.
- [R12 Disposition — Retirement vs Retention Evidence Template](decisions/07-r12-disposition.md) — 8-row shared evidence table + 5-field retention block vs retirement audit-retention split, 8-surface atomic disposition table (7 docs + stage4-entry-gate), 8 guardrails (R11 owner, predecessors Verified, audit-bound, Stage 4 four-thing gate, no early removal, typed retention trigger, retained audits, grilling HITL); `fmt`/`check:rust/arch` PASS.
- [Stage 4 Entry — Generic Controlled Runtime vs Godot Adapter Layering](decisions/08-stage4-entry-sequence.md) — 7-step Stage-4 sequence frozen generic-first (RUST_MIGRATION 719-732), generic Host vs Godot Plugin table, 4-arrow entry gate (R1-R11 + Stage 1-3 audit + R12 + stage4-entry-gate.md 17 criteria), two lean sentences (Host not a plugin + composition model), Stages 4-6 confirmed not guaranteed; `fmt`/`check:rust/arch` PASS.
- [R8 Entry Review — Freeze R8 Contract, Subjects, Measurement](decisions/10-r8-entry-review.md) — **PASS — R8 contract frozen; authorized as next implementation slice** (6 surfaces, 13-row co-located table, 5 differential subjects, measurement per `RUST_STYLE.md:568-589`); `fmt`/`check:rust/arch` PASS.
- [R8 Verified Promotion — What Closes R8 Active?](decisions/11-r8-verified-promotion.md) — **R8 Verified at worktree `c075b3c`** (all gates observed PASS incl. differential 150/150 at corpus v16/155 files; fail-closed grep sweep zero spawn paths; atomic 9-surface advancement); R9 requires its own entry review.
- [R9 Entry Review — Freeze the Godot Stage-3 Contract](decisions/12-r9-entry-review.md) — **PASS — R9 contract frozen; authorized as next implementation slice** (3 surfaces: review/impact intelligence, prepare-only mutation contracts, deterministic `/develop` core; differential subjects `godot-review-context`/`godot-mutation-prepare`/`godot-develop-plan`; apply/checkpoints stay typed `unavailable`).

## Not yet specified

<!-- In-scope fog — you can see it's coming but can't ticket sharply yet; graduates as frontier moves -->

- ~~Exact shape of the R7.5 independent-review **PASS/FAIL** rubric~~ — **decided** in [R7.5 Review Rubric](decisions/01-r7-5-review-rubric.md) (strings §1, authority §2, sanitizer advisory §3, flag scope §4, checklist §5).
- ~~Granularity of **R8 vs R9** slice boundaries~~ — **decided** in [R8 vs R9 Cut](decisions/04-r8-r9-cut.md) (6+3 surfaces, per-row `unavailable` posture, Not-in-R8/R9 list); corpus subject names remain advisory until the R8 entry review freezes them.
- ~~**R10 (H1/H2/ICM + H3 runtime-readiness) parity** subject breakdown~~ — **decided** in [R10 Scope](decisions/05-r10-scope.md) (one Verified + 3 sub-slices `H1→H2→ICM→H3`; 14 subjects with core seams; R11 exclusion list); advisory until the R10 entry review freezes corpus names.
- ~~**R11 full differential/effect-boundary/security/recovery/cross-platform parity** closure criteria~~ — **decided** in [R11 Gate](decisions/06-r11-gate.md) (entry on R7+R8/R9+R10a-c; 4 pillars + measurement discipline; 7-surface PASS artefacts).
- ~~**R12 retirement/retention disposition** evidence template~~ — **decided** in [R12 Disposition](decisions/07-r12-disposition.md) (8 shared evidences + 5-field retention vs retirement retained-audits, 8 guardrails); disposition itself waits on R11 Verified.
- ~~Stage 4 **Controlled Runtime Execution** generic host boundary vs. Godot runtime adapter layering~~ — **decided** in [Stage 4 Entry Sequence](decisions/08-stage4-entry-sequence.md) (generic 4.1-4.2 before Godot 4.3+ specialization, four-thing ordered gate, stages 4-6 not guaranteed).

## Out of scope

<!-- Conscious scope boundary — never graduates; returns only if Destination is redrawn -->

- General Hooks, built-in multi-agent frameworks, TaskGraph, generic workflow engines, agent teams/Fleet, distributed workers, plugin marketplaces, plugin dependency graphs, automatic Skill/Plugin acquisition, model-router architecture, generic Memory subsystem, GUI/TUI runtime ownership — explicitly **FUTURE / NOT DUE** per `ARCHITECTURE.md` (lean constitution ADR 0036).
- Adding a Godot package, placeholder domains, or marketing content ahead of R7/R8.
- Re-enabling pathname-based filesystem/process approximations that the fail-closed posture intentionally reports as `unavailable`.
