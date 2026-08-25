# Decision — R13.4 Entry Review — Planning & Briefing Scenarios

**Wayfinder ticket:** [R13.4 Entry Review](../tickets/26-r13-4-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R13 Continuation Contract](21-r13-remaining-surface-parity-entry-review.md) (PASS) + R13.3 landed (`ef597de`, parity held 224/224 at corpus v26)
**Decided:** 2026-08-25 (resolver session, interactive HITL grilling over reads of `packages/core/src/planning/*`, `packages/core/src/tasks/task-runtime-planning.ts`, and the executor surface exports)
**Status:** **PASS — R13.4 scenario contract frozen; implementation authorized**
**Self-loop ledger:** 4 criteria, one decision pass (verification below)

> Mirrors [R13.1 Entry Review](22-r13-1-entry-review.md) / [R13.2 Entry
> Review](23-r13-2-entry-review.md) / [R13.3 Entry Review](24-r13-3-entry-review.md).
> No implementation lands in this record.

---

## Summary

R13.4 ports the host-owned planning model and the executor briefing
foundation to the Rust candidate and proves them differentially: the
immutable revisioned plan artifact with typed canonical digests, strict
plan-candidate validation, the host depth policy, the planning flow phases,
the task-runtime plan lifecycle (set/approve/invalidate with exact refusal
reasons), and the executor contracts — execution contract identity,
milestone-manifest acceptance ids, evidence-only acceptance evaluation,
deterministic brief compilation, workspace scope classification,
documentation selection, new-file discipline signals, bounded rendering,
and context-pack references.

## 1. Frozen scenario set (~20 fixtures, corpus v27)

### `planning-runtime` ×10

| #   | Case                            | What it proves                                                                                                                                                                                                                               |
| --- | ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | plan-model-identity             | `createTaskPlan`/`reviseTaskPlan` immutability; revision starts at 1 and advances by one; typed canonical plan digests are deterministic; `summarizePlan` shape; `hasMeaningfulAcceptanceCriteria` bar (≥2 criteria, ≥1 non-user verifiable) |
| 2   | plan-validation-strict          | candidate rejections with exact reasons: bounds (steps/touchpoints/statements), id patterns, verified touchpoints require a `rev_`+32hex handle, unknown step touchpoint refs, verification refs absent from the contract                    |
| 3   | planning-policy-depth           | the host depth decision (none/light/full) over injected policy inputs; provider/user input can never raise depth or bounds                                                                                                                   |
| 4   | planning-flow-phases            | the flow phase machine's deterministic transitions and invalid-transition refusals                                                                                                                                                           |
| 5   | plan-set-lifecycle              | `setTaskPlan`: first revision must be 1; same-id revisions advance by exactly one; replacement plans start at 1 with a fresh id (id reuse refused); terminal-task mutation refusal                                                           |
| 6   | plan-staleness-contract-advance | a TaskContract revision advance marks the current plan stale and invalidates any approval, with exact stale reasons and `plan_invalidated` activity records                                                                                  |
| 7   | plan-approval-binding           | approval binds the exact plan digest AND contract content digest; wrong revision/digest or stale state is refused; validation `requirements` stay descriptive and grant nothing                                                              |
| 8   | plan-revision-cap               | `maxPlanRevisions` (16) cap refuses further replanning with the exact reason                                                                                                                                                                 |
| 9   | plan-immutability-detach        | stored plan history is detached from caller-owned content (post-call mutations never reach history); accessors return detached copies                                                                                                        |
| 10  | plan-invalidate-reasons         | explicit invalidation carries the caller reason into state and the activity trail in deterministic order                                                                                                                                     |

### `executor-brief` ×10

| #   | Case                               | What it proves                                                                                                                                    |
| --- | ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | execution-contract-identity        | create/revise execution contracts; digest identity; briefs reference the contract by revision and digest                                          |
| 2   | milestone-manifest-acceptance-ids  | synthetic manifest validation; stable acceptance ids; requirement/deliverable/invariant bounds                                                    |
| 3   | acceptance-evaluator-evidence-only | acceptance is satisfied ONLY by host-observed evidence; executor claims never suffice; exact failure reasons                                      |
| 4   | brief-compile-determinism          | `compileExecutorBrief` twice yields identical fingerprints; briefs reference the execution contract by revision and never restate permanent rules |
| 5   | brief-active-working-set           | the current-step working set lists exact files with inclusion reasons under budgets                                                               |
| 6   | workspace-scope-classification     | verified vs candidate file classification with budgets from injected file states                                                                  |
| 7   | documentation-selection            | root + scoped AGENTS.md selection, accepted ADR inclusion, archive/superseded exclusion, budget respected                                         |
| 8   | new-file-discipline-signals        | rationale creation, proliferation heuristics, scope-diff classification (expected / justified expansion / unexplained expansion)                  |
| 9   | brief-render-bounded               | byte-equal rendering under the injected clock with truncation bounds; summary shape                                                               |
| 10  | context-pack-refs                  | context-pack reference assembly (contract/plan/instruction/touchpoint/capability/scope/working-set) is bounded and detached                       |

## 2. Mechanics

- Probe layout follows the established pattern: new oracle probes
  (`planning-runtime-oracle.mjs`, `executor-brief-oracle.mjs`) execute the
  **real TypeScript reference** functions with bounded stdin JSON and emit
  canonical outcome records; the Rust side ports the corresponding modules
  behind the same subject names. Placement obeys the dependency direction
  (`cli → adapters → core`; core imports no adapters).
- Corpus bumps **v27** at the R13.4 reconciliation commit; schema stays 3.
- **Injected clock everywhere**: `createdAt`, `approvedAt`, and rendered
  timestamps come from one fixed clock so records are byte-stable.
- **Synthetic fixtures only** (HITL decision 2026-08-25): execution
  contracts, milestone manifests, documentation indexes, workspace file
  states, and acceptance evidence are fixture constants inside the probes —
  never the repository's real ADRs, AGENTS.md files, or S3M8 manifest.
  Both surfaces are pure/injectable; no temporary filesystem fixtures are
  needed beyond the existing task-kernel seams.
- The Rust port targets `siralos_core::planning` (model/policy/flow/
  validation) plus the runtime-planning functions on the existing
  `siralos_core::task` kernel, and `siralos_core::executor`; no adapters
  are required by these surfaces.
- Acceptance mirrors every prior slice: all applicable required scenarios
  at byte parity plus focused Rust unit tests; the full local gate passes
  on the reconciliation tree.

## 3. Boundaries — not in R13.4

- `briefing-service.ts` composition wiring and the real S3M manifests' CLI
  use are deferred to R13.5 CLI product composition (HITL decision
  2026-08-25); R13.4 ports the pure contracts/compiler/scopes only.
- No capability surface changes: plan approval still authorizes nothing;
  documentation selection remains derived context that never grants
  capability; acceptance remains host-evidence-only.
- Knowledge seeding, reference access tools, and CLI session composition
  remain R13.5 (per [R13.2 Entry Review](23-r13-2-entry-review.md),
  [R13.3 Entry Review](24-r13-3-entry-review.md)).

## 4. Authorization

Implementation of R13.4 is authorized against this frozen set; landings
are recorded in the [R13 Execution Register](../tickets/22-r13-execution-register.md).

---

## Self-loop verification

| Criterion                                    | Direct evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Status |
| -------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Cases exercise reference-observable behavior | §1 cites surfaces verified by reading the TS sources this session: `createTaskPlan`/`reviseTaskPlan` identity fields (`planning-model.ts:296-374`), `PLANNING_LIMITS`/patterns, `validatePlanCandidate` seam via `setTaskPlan`, runtime lifecycle functions (`task-runtime-planning.ts:42-242`: sequencing, replacement rules, staleness on contract advance, approval binding to both digests, cap at `maxPlanRevisions`), executor exports for contract/manifest/brief/scope/documentation/discipline | pass   |
| Determinism posture preserved                | §2 injects one fixed clock; all executor inputs are injectable/pure (verified from export signatures — selection takes entries as inputs, no fs reads); §2 mandates synthetic fixtures only                                                                                                                                                                                                                                                                                                             | pass   |
| Overlap resolved, no double port             | TaskContract/plan-digest primitives already exist in `siralos_core::identity` (contract-plan identity landed at R10a); knowledge/research/reference seams stay with their slices; briefing-service + real manifests deferred to R13.5                                                                                                                                                                                                                                                                   | pass   |
| Human decided the material cuts              | HITL answers 2026-08-25: 10+10 split approved; synthetic fixtures only; policy + flow included as cases 3–4; briefing-service deferral confirmed                                                                                                                                                                                                                                                                                                                                                        | pass   |
