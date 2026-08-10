# ADR 0020 — Host-Controlled Planning Foundation

- Status: accepted (Stage 3 milestone 7)
- Date: current milestone
- Related: ADR 0014 (task runtime), ADR 0015 (projection boundaries),
  ADR 0016 (workspace revision and structural reads), ADR 0017 (project
  instructions and knowledge), ADR 0019 (self-reference and capability
  diagnostics)

## Context

Every task should not pay planning overhead: trivial bounded edits do not
need a model-generated plan, and a plan that costs tokens and context for
every request is a tax. But model-decided planning is unstable — whether a
plan exists, how deep it is, and what it authorizes becomes a property of
the model's whim rather than of the task. Free-form plan prose cannot be
runtime state: the host cannot bind approvals to it, cannot detect
staleness against it, and cannot project it deterministically. And plan
approval needs exact revision semantics: approving "the plan" is
meaningless unless it is clear WHICH revision of the plan was approved and
that later revisions do not inherit the approval.

Solaris already has host-owned task state (`TaskContract`, `TaskState`,
activity log), projection boundaries, evidence, knowledge, references,
and diagnostics. Planning can now be structured and runtime-controlled:
the host decides whether planning is needed, the planner is read-only, a
plan is an immutable revisioned artifact bound to the exact TaskContract
revision, and approving a plan never approves the later edits.

## Decision

Introduce a host-controlled planning phase between the TaskContract and
execution:

1. **Deterministic `PlanningPolicy`** — an application-owned pure policy
   selects `none | light | full` from host-visible task facts (explicit
   user plan request, inspection-only work, mutation expectation,
   protected-config involvement, multi-subsystem span, research
   requirement, capability uncertainty, narrow-repair with a known
   surface, known-touchpoint count, acceptance-criterion count). The
   policy is deterministic (identical inputs produce identical decisions),
   never invokes a model to classify complexity, and prefers `light` over
   `none` when signals are ambiguous while using `full` only on concrete
   complexity/risk signals.

2. **None/light/full modes** — `none` skips the planner entirely and runs
   through the existing Task Runtime path (no extra model call); `light`
   produces a compact plan (objective, bounded steps, expected primary
   touchpoints, primary validation — no fake filler for full-plan fields);
   `full` produces the complete structured plan (scope, non-goals,
   verified/candidate touchpoints, architecture constraints, risks,
   acceptance linkage, validation strategy, rollback considerations) with
   bounded size and step counts.

3. **Structured immutable `TaskPlan`** — a plan is a typed host-owned
   artifact: identity, revision, task id, the exact TaskContract revision
   it was created against, depth, objective, scope, non-goals,
   touchpoints, constraints, risks, steps, validation, rollback, public
   rationale. Revisions are immutable: a material change produces rev N+1;
   rev N is never mutated and stays inspectable. Plan identity (id,
   revision, task, contract revision, timestamps) is host-assigned; the
   planner supplies content only.

4. **Verified vs candidate touchpoints** — a verified touchpoint records
   the exact inspected workspace revision handle (`rev_` + 32 hex) and an
   evidence reference (`read:path`, `api:Symbol`, `reference:alias@rev`,
   `knowledge:subject`, ...); a candidate touchpoint is explicitly
   unverified (globs allowed) and is never promoted to verified without
   inspection evidence. Guesses are never represented as facts.

5. **Read-only planner** — the planner runs in a structurally read-only
   capability profile: workspace read/search/structural reads, Godot
   inspection and API knowledge, references, policy-gated research,
   self-reference. Mutation, process execution, approval-granting,
   checkpoint, and undo surfaces are absent from its tool registry BY
   CONSTRUCTION and additionally hidden by the ToolProjector's `planning`
   mode; the executor refuses any non-read-only tool at the runtime
   boundary and executes only tools the host projection marks `available`
   (a visible-but-gated call fails without execution). The planner cannot approve its own plan, cannot approve
   edits, cannot broaden capabilities, cannot mark validation or the task
   complete, and cannot choose planning depth (the host routes).

6. **Planner context separate from executor context** — every planning
   attempt uses a fresh provider instance and a fresh conversation;
   planner and executor never share provider-private continuation state.
   The executor context later receives only the validated structured
   plan (projected as a bounded contextual `Task plan` segment — current
   revision only, never plan history, never transcripts) plus the
   TaskContract/TaskState/instructions/evidence as before. Private model
   reasoning is never stored in TaskState, plan history, or activity.

7. **Strict validation boundary** — planner output is untrusted data
   validated by the host (`validatePlanCandidate`): depth must match the
   host routing, steps/touchpoints/fields bounded, verified touchpoints
   must carry exact revision handles, referenced acceptance criteria must
   exist, paths must be safe workspace-relative paths (no escapes, no
   `@reference/` namespaces), secrets are rejected, and policy-shaped
   capability claims are rejected (a plan that says "enable unrestricted
   network" is rejected — it can never even LOOK like a grant). Malformed
   output is a planning failure retried within a bounded budget (2
   attempts), never silently treated as plan prose.

8. **Plan approval binds to the exact plan revision** — approving plan
   rev N approves ONLY rev N against ONLY the recorded TaskContract
   revision. Advancing the plan revision or the TaskContract revision
   invalidates the prior approval (state becomes `invalidated`, stale
   approvals are refused at the runtime boundary). Plan approval NEVER
   authorizes source edits or commands: every mutation still requires the
   exact change-set preparation → one-time approval → checkpoint → apply
   path, and every command keeps its own approval rules. Plan
   requirements are descriptive and grant nothing.

9. **TaskState integration** — TaskState carries a bounded plan reference
   (plan id, revision, depth, state current/stale, approval state, stale
   reason); the full immutable plan lives in the runtime's plan history.
   TaskState remains the execution-progress authority: plan steps are
   proposed structure, never competing mutable progress, and plan
   acceptance never materializes plan steps into TaskState steps at this
   stage. A TaskContract revision change marks the bound plan stale and
   invalidates its approval automatically. Planning activity
   (`planning_routed`, `plan_created`, `plan_rejected`, `plan_approved`,
   `plan_invalidated`) appends to the existing activity log.

10. **Planning budgets** — the planner has a bounded budget (max tool
    rounds, per-turn tool calls, text events, output/tool/aggregate bytes,
    timeout, max attempts), requires an explicit provider completion event,
    rejects post-completion events and duplicate call ids, and
    repeated identical no-progress reads fail planning cleanly while
    feeding the host progress tracker (they never count as indefinite
    progress and never grant mutation capability).

11. **Full-plan acceptance gate** — full-plan execution requires the exact
    current plan revision to be approved and
    meaningful acceptance criteria in the TaskContract (at least two
    criteria, one host-verifiable) before any source mutation; a full
    plan on a contract without them is blocked with a precise reason.
    Plans never create a second acceptance system and never change
    acceptance criteria.

12. **CLI surfaces** — `/plan <request>` (plan-only: create a planning
    task, run read-only planning, print the structured plan, stop with
    zero workspace changes and zero mutation checkpoints) and `/develop`
    routing (host-controlled depth before the executor provider call;
    `--plan` / `--plan-light` force the depth; full plans ask for plan
    approval through the interactive reviewer; denial or cancellation stops
    before the executor, and verified-touchpoint staleness invalidates the
    plan before the acceptance/approval gate and execution;
    planning failure/cancel terminates the workflow cleanly).
    `/development-status`, `/status`, and `/task-status` show planning
    depth, plan revision, plan state, approval state, and staleness.

## Alternatives rejected

- **Always plan** — every trivial edit pays a planner model call; rejected
  for cost.
- **Never plan** — complex tasks get no explicit preparation; rejected.
- **Ask an LLM whether a plan is necessary** — model-decided planning is
  unstable and non-deterministic; the host routes from host-visible facts.
- **Planner receives write tools** — a planner that can edit is not
  advisory; structurally read-only instead, enforced at the boundary.
- **Free-form Markdown plan as authoritative runtime state** — prose
  cannot bind approvals, detect staleness, or project deterministically;
  structured typed plans only.
- **Approving a plan automatically approving future edits** — the critical
  invariant is reversed: plan approval and mutation approval stay
  separate, exact, one-time paths.
- **Mutating plan revisions in place** — revisions are immutable; changes
  advance the revision and invalidate old approvals.
- **Planner and executor sharing provider-private continuation** — planner
  context is fresh and isolated; the executor receives only the validated
  plan.
- **A plan declaring capability requirements that act as grants** — plan
  content is descriptive; policy-shaped claims are rejected at validation.

## Consequences

### Benefits

- Lower cost for simple tasks: `none` routing never invokes a planner
  provider.
- Explicit complex-task preparation: full plans justify their cost with
  scope, risks, validation, and rollback.
- Reproducibility: planning depth is a deterministic function of host
  facts.
- Better acceptance mapping: full/light plans link steps to existing
  TaskContract acceptance criteria; the gate blocks mutation without
  meaningful criteria.
- Safer approvals: plan approval binds to the exact plan revision, and a
  changed plan or contract invalidates the old approval; plan approval
  never pre-approves edits or commands.
- Future multi-agent compatibility: planning is a host-owned phase other
  agents can consume without trusting model prose.

### Costs

- Additional plan models and state (TaskPlan, revisions, approval state).
- Planning routing logic (PlanningPolicy).
- Additional behavior/effect tests and architecture enforcement.
- The planner itself is provider-driven; with only the deterministic fake
  provider installed, `/plan` and `/develop` planning produce the fake
  provider's structured plan — real planning quality depends on a real
  provider.
