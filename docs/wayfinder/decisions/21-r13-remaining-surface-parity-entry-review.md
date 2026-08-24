# Decision — Remaining-Surface Ports Entry Review — Freeze the Continuation Contract (R13)

**Wayfinder ticket:** [Remaining-Surface Ports Entry Review](../tickets/21-remaining-surface-ports-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** nothing — R11 Verified (`eea0029e70aae7248b3e1022c3be1cb669fd5a09`) opened this frontier
**Decided:** 2026-08-24 (resolver session, interactive HITL grilling over the module-size inventory, the unported-surface list from [R12 Disposition Execution](../tickets/20-r12-disposition.md), and the existing Rust crate seams)
**Status:** **PASS — R13 contract frozen; R13.1 authorized as the next implementation slice**
**Self-loop ledger:** 4 criteria, one implementation pass (verification below)

> Mirrors [R10 Scope](05-r10-scope.md) / [R11 Entry Review](18-r11-entry-review.md):
> this decision freezes scope and subjects only — no implementation lands here.

---

## Summary

The R12 verdict was deferred by HITL decision (2026-08-24) until every
TypeScript Stage-3 surface has a Rust differential subject. This review
freezes that continuation as **milestone R13 — Remaining TypeScript
Surface Parity**: five ordered, individually entry-reviewed sub-slices,
one frozen subject set, corpus bumps at slice reconciliation commits.
R12 remains the disposition-only HITL decision after R13.

## 1. Frozen slice structure

| Slice                                    | Scope                                                                                                                                                                                          | Frozen differential subjects                                   | Depends on                          | Approx. size      |
| ---------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- | ----------------------------------- | ----------------- |
| **R13.1** Host introspection & authority | command catalog/registry/runners; security policy evaluation + behavioral-config classification; self-reference + capability doctor; approval binding                                          | `security-permissions`, `command-catalog`, `capability-doctor` | existing tool/projection seams      | ~25 src files     |
| **R13.2** Workspace guidance             | instruction resolution precedence; knowledge coordinator/revisions/retrieval                                                                                                                   | `instructions-resolution`, `knowledge-revisions`               | R4 workspace revisions + projection | ~7 src files      |
| **R13.3** External knowledge boundaries  | references identity/resolver/materializer; research denied-by-default policy + bounded transports                                                                                              | `reference-identity`, `research-policy`                        | workspace + security policy (R13.1) | ~11 src files     |
| **R13.4** Planning & briefing            | planning runtime/policy/model; executor contract/manifests/acceptance/brief compiler/workspace-scope/documentation-context                                                                     | `planning-runtime`, `executor-brief`                           | R13.2 + identity/context crates     | ~21 src files     |
| **R13.5** CLI product composition        | full slash-command interactive session over the ported seams (**full behavioral parity** per HITL); mutation/execution commands port as their typed-`unavailable` reporting, never operational | `cli-session`                                                  | all previous slices                 | composition-heavy |

Ordering rationale: authority/introspection first (consumed by later
slices, read-only posture), workspace guidance second, policy-gated
external boundaries third, planning/briefing fourth (largest, depends on
guidance), CLI composition last (depends on everything).

## 2. Corpus mechanics

Schema stays `3`. Each slice bumps the corpus version at its own
reconciliation commit (v24 at R13.1, continuing per slice). Subject
names above are frozen now, mirroring [R10 Entry Review](14-r10-entry-review.md);
scenario-level fixtures are owned by each sub-slice's own entry review.

## 3. Boundaries — not in R13

- No typed-`unavailable` effect becomes operational (fail-closed posture
  unchanged; mutation/execution commands report availability truthfully).
- No product feature beyond TypeScript behavioral parity; lean porting
  discipline applies ([RUST_MIGRATION.md] lean-porting clause): port
  required observable behavior, not TypeScript structure.
- No real network in `research-policy` scenarios — the denied-by-default
  posture and bounded-transport contracts are exercised with typed
  refusals and fake/bounded sources, matching the reference adapters.
- No Stage 4 work; Stage 4 entry remains gated behind R12 +
  stage4-entry-gate.md.

## 4. Template amendment record

[R12 Disposition](07-r12-disposition.md) guardrail (b)'s predecessor
list is amended by this review from "R1–R11 all Verified" to
"**R1–R11 + R13 all Verified**". Intent unchanged: no disposition over
an unverified surface — the amendment exists precisely because the
deferred verdict revealed surfaces the original numbering predated.
The amendment is recorded here and on the ticket; decision 07's file is
not retroactively edited.

## 5. Authorization

**R13.1 (Host introspection & authority) is authorized to begin now**
with its own entry review freezing concrete scenarios before code,
matching the R8/R9/R10/R11 pattern. R13.2–R13.5 are sequenced but require
their own entry reviews at their turn. Slice landings are tracked in the
[R13 Execution Register](../tickets/22-r13-execution-register.md).

---

## Self-loop verification

| Criterion                                           | Direct evidence                                                                                                                                                                                                                   | Status |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Every unported surface has exactly one owning slice | §1 table covers instructions/knowledge, references/research, planning/executor, self/doctor, commands/security/approval, CLI product layer — cross-checked against the inventory on [ticket 20](../tickets/20-r12-disposition.md) | pass   |
| Subject set frozen before any code                  | §1 names nine subjects; §2 pins bump mechanics to reconciliation commits                                                                                                                                                          | pass   |
| Fail-closed and lean guardrails carried forward     | §3 explicit boundary list; template guardrails a–h unaffected except the recorded §4 predecessor-list amendment                                                                                                                   | pass   |
| Human decided the three material cuts               | HITL answers 2026-08-24: structure approved as proposed; R13 inserted with template amendment; R13.5 full parity                                                                                                                  | pass   |
