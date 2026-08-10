import { canonicalizeJson, sha256Hex } from "../godot/digest.js";
import { deepFreeze } from "../domain/deep-freeze.js";
import type { AcceptanceCriterionId, TaskContract } from "../tasks/task-contract.js";
import type { TaskId } from "../tasks/task-model.js";

/**
 * Host-owned structured planning model (Stage 3 milestone 7, ADR 0020).
 *
 * Planning is a runtime-owned phase: the host decides whether planning is
 * needed and at what depth; the planner is a read-only advisor; the plan is
 * an immutable revisioned artifact bound to exactly one TaskContract
 * revision; and plan approval never authorizes source edits or commands.
 *
 * A `TaskPlan` is immutable once revisioned. Material changes produce a new
 * revision; revision N is never mutated in place. The plan carries only
 * structured content and public rationale — never private model reasoning —
 * and it never grants capability: there is no capability/policy surface in
 * the model, validation rejects policy-shaped claims, and security policy
 * remains authoritative outside planning.
 */

export type PlanningDepth = "none" | "light" | "full";

export type TaskPlanId = string;

/** Whether a touchpoint was actually inspected (verified) or is likely but
 * unconfirmed (candidate). Guesses are never promoted to verified. */
export type TouchpointConfidence = "verified" | "candidate";

export type PlanRiskSeverity = "low" | "medium" | "high";

export interface PlanScope {
  /** What the plan intends to cover (bounded statements). */
  readonly inScope: readonly string[];
  /** What the plan deliberately does not cover (bounded statements). */
  readonly outOfScope: readonly string[];
}

export interface PlanTouchpoint {
  /** Touchpoint identifier referenced by steps. */
  readonly id: string;
  /** Workspace-relative path; candidate touchpoints may use glob patterns. */
  readonly path: string;
  readonly confidence: TouchpointConfidence;
  /**
   * Workspace revision handle of the inspected file state. REQUIRED for
   * verified touchpoints: a verified claim must point at the exact
   * inspected revision (`rev_` + 32 hex chars).
   */
  readonly revision?: string;
  /**
   * Bounded evidence reference in `kind:ref` form (e.g. `read:src/player/player.gd`,
   * `api:CharacterBody2D`, `reference:gdunit4@commit`, `research:<requestId>`,
   * `knowledge:<subject>`). Never embeds source text.
   */
  readonly evidence?: string;
  readonly note?: string;
}

export interface PlanConstraint {
  readonly id: string;
  readonly description: string;
}

export interface PlanRisk {
  readonly id: string;
  readonly severity: PlanRiskSeverity;
  readonly description: string;
}

export interface PlanStep {
  readonly id: string;
  readonly title: string;
  readonly description?: string;
  /** Ids of the plan touchpoints this step is expected to touch. */
  readonly expectedTouchpoints: readonly string[];
  /** Ids of TaskContract acceptance criteria this step helps satisfy. */
  readonly verification?: readonly AcceptanceCriterionId[];
}

export interface PlanValidationStrategy {
  /** Bounded primary validation checks (e.g. "check-only parse", "fresh LSP diagnostics"). */
  readonly checks: readonly string[];
  /**
   * Descriptive requirements (e.g. "workspace mutation", "existing project
   * tests"). These are DESCRIPTIVE only — they never grant anything.
   */
  readonly requirements?: readonly string[];
}

export interface PlanRollbackStrategy {
  readonly description: string;
}

/**
 * Planner-supplied plan content. The planner proposes content only; plan
 * identity (id, revision, taskId, taskContractRevision, createdAt) is
 * host-owned and assigned by `createTaskPlan` after validation.
 */
export interface TaskPlanContent {
  readonly objective: string;
  readonly scope: PlanScope;
  readonly nonGoals: readonly string[];
  readonly touchpoints: readonly PlanTouchpoint[];
  readonly constraints: readonly PlanConstraint[];
  readonly risks: readonly PlanRisk[];
  readonly steps: readonly PlanStep[];
  readonly validation: PlanValidationStrategy;
  readonly rollback?: PlanRollbackStrategy;
  /** Concise public rationale; never private model reasoning. */
  readonly rationale?: string;
}

/** Immutable host-owned planning artifact. */
export interface TaskPlan {
  readonly id: TaskPlanId;
  /** Immutable revision identity; starts at 1 and only ever increases. */
  readonly revision: number;
  readonly taskId: TaskId;
  /** The exact TaskContract revision this plan was created against. */
  readonly taskContractRevision: number;
  readonly depth: "light" | "full";
  readonly objective: string;
  readonly scope: PlanScope;
  readonly nonGoals: readonly string[];
  readonly touchpoints: readonly PlanTouchpoint[];
  readonly constraints: readonly PlanConstraint[];
  readonly risks: readonly PlanRisk[];
  readonly steps: readonly PlanStep[];
  readonly validation: PlanValidationStrategy;
  readonly rollback?: PlanRollbackStrategy;
  readonly rationale?: string;
  readonly createdAt: number;
}

/**
 * Plan approval record. Approval binds to the EXACT plan revision and the
 * EXACT TaskContract revision; either advancing invalidates it. Plan
 * approval authorizes nothing but the plan's acceptance as the execution
 * reference — never source edits, commands, or capabilities.
 */
export interface PlanApproval {
  readonly planId: TaskPlanId;
  readonly planRevision: number;
  readonly taskContractRevision: number;
  readonly approvedAt: number;
}

/**
 * Bounded plan reference embedded in TaskState. TaskState never embeds
 * giant plan text — the full immutable plan lives in the runtime's plan
 * history and TaskState carries identity, depth, staleness, and approval
 * state only.
 */
export interface TaskPlanState {
  readonly planId: TaskPlanId | null;
  /** 0 when no plan exists. */
  readonly planRevision: number;
  readonly depth: PlanningDepth;
  readonly state: "none" | "current" | "stale";
  readonly approval: "none" | "approved" | "invalidated";
  readonly staleReason: string | null;
}

export const NO_TASK_PLAN: TaskPlanState = Object.freeze({
  planId: null,
  planRevision: 0,
  depth: "none",
  state: "none",
  approval: "none",
  staleReason: null,
});

/**
 * Deterministic plan bounds. These are host-owned and immutable: provider
 * output and user configuration can never raise them, and an oversized
 * candidate is rejected rather than normalized.
 */
export const PLANNING_LIMITS = {
  /** Maximum plan steps (light plans are stricter, see below). */
  maxSteps: 12,
  maxStepsLight: 6,
  maxTouchpoints: 24,
  maxConstraints: 12,
  maxRisks: 12,
  maxNonGoals: 16,
  maxScopeEntries: 16,
  maxValidationChecks: 12,
  maxValidationRequirements: 8,
  maxExpectedTouchpointsPerStep: 12,
  maxVerificationRefsPerStep: 12,
  /** Maximum UTF-8 bytes of the serialized plan content. */
  maxPlanContentBytes: 32 * 1024,
  maxObjectiveBytes: 2048,
  maxStatementBytes: 512,
  maxStepTitleBytes: 256,
  maxStepDescriptionBytes: 1024,
  maxRollbackBytes: 1024,
  maxRationaleBytes: 1024,
  maxPathBytes: 1024,
  maxEvidenceBytes: 256,
  maxRevisionBytes: 128,
  maxNoteBytes: 512,
  /** Maximum plan revisions one task may accumulate (hard bound). */
  maxPlanRevisions: 16,
} as const;

export const PLAN_ID_PATTERN = /^plan-[A-Za-z0-9._-]{1,95}$/;
export const PLAN_STEP_ID_PATTERN = /^[A-Za-z][A-Za-z0-9._-]{0,63}$/;
export const PLAN_TOUCHPOINT_ID_PATTERN = /^[A-Za-z][A-Za-z0-9._-]{0,63}$/;
export const PLAN_CONSTRAINT_ID_PATTERN = /^[A-Za-z][A-Za-z0-9._-]{0,63}$/;
export const PLAN_RISK_ID_PATTERN = /^[A-Za-z][A-Za-z0-9._-]{0,63}$/;
/** Workspace revision handles are opaque `rev_` + 32 hex chars. */
export const PLAN_REVISION_HANDLE_PATTERN = /^rev_[0-9a-f]{32}$/;

export interface CreateTaskPlanInput {
  readonly id: TaskPlanId;
  readonly taskId: TaskId;
  readonly taskContractRevision: number;
  readonly depth: "light" | "full";
  readonly content: TaskPlanContent;
  readonly createdAt: number;
}

function copyStrings(values: readonly string[]): string[] {
  return values.map((value) => value);
}

function copyTouchpoints(values: readonly PlanTouchpoint[]): PlanTouchpoint[] {
  return values.map((touchpoint) => ({ ...touchpoint }));
}

function copySteps(values: readonly PlanStep[]): PlanStep[] {
  return values.map((step) => ({
    ...step,
    expectedTouchpoints: [...step.expectedTouchpoints],
    ...(step.verification === undefined ? {} : { verification: [...step.verification] }),
  }));
}

function copyConstraints(values: readonly PlanConstraint[]): PlanConstraint[] {
  return values.map((value) => ({ ...value }));
}

function copyRisks(values: readonly PlanRisk[]): PlanRisk[] {
  return values.map((value) => ({ ...value }));
}

function buildContent(content: TaskPlanContent): TaskPlanContent {
  return {
    objective: content.objective,
    scope: {
      inScope: copyStrings(content.scope.inScope),
      outOfScope: copyStrings(content.scope.outOfScope),
    },
    nonGoals: copyStrings(content.nonGoals),
    touchpoints: copyTouchpoints(content.touchpoints),
    constraints: copyConstraints(content.constraints),
    risks: copyRisks(content.risks),
    steps: copySteps(content.steps),
    validation: {
      checks: copyStrings(content.validation.checks),
      ...(content.validation.requirements === undefined
        ? {}
        : { requirements: copyStrings(content.validation.requirements) }),
    },
    ...(content.rollback === undefined ? {} : { rollback: { ...content.rollback } }),
    ...(content.rationale === undefined ? {} : { rationale: content.rationale }),
  };
}

/**
 * Create the first immutable plan revision. The host supplies identity and
 * content that already passed `validatePlanCandidate`; the returned plan is
 * frozen and never mutated in place.
 */
export function createTaskPlan(input: CreateTaskPlanInput): TaskPlan {
  if (!PLAN_ID_PATTERN.test(input.id)) {
    throw new Error(`Invalid plan id: ${input.id}`);
  }
  if (!Number.isSafeInteger(input.taskContractRevision) || input.taskContractRevision < 1) {
    throw new Error("A plan requires a positive safe-integer task contract revision.");
  }
  if (input.depth !== "light" && input.depth !== "full") {
    throw new Error("A plan requires depth light or full.");
  }
  if (!Number.isSafeInteger(input.createdAt) || input.createdAt < 0) {
    throw new Error("A plan requires a non-negative safe-integer createdAt timestamp.");
  }
  const plan: TaskPlan = {
    id: input.id,
    revision: 1,
    taskId: input.taskId,
    taskContractRevision: input.taskContractRevision,
    depth: input.depth,
    ...buildContent(input.content),
    createdAt: input.createdAt,
  };
  return deepFreeze(plan);
}

export interface ReviseTaskPlanInput {
  readonly content: TaskPlanContent;
}

/**
 * Produce the next immutable plan revision. The previous revision object is
 * untouched; the returned plan carries `revision + 1` and the same plan id.
 * Any existing approval of the previous revision is invalid by construction
 * (the runtime invalidates it when the revision advances).
 */
export function reviseTaskPlan(previous: TaskPlan, changes: ReviseTaskPlanInput): TaskPlan {
  if (!PLAN_ID_PATTERN.test(previous.id)) {
    throw new Error(`Invalid plan id: ${previous.id}`);
  }
  if (
    !Number.isSafeInteger(previous.revision) ||
    previous.revision < 1 ||
    previous.revision >= Number.MAX_SAFE_INTEGER
  ) {
    throw new Error("A previous plan revision must be a positive incrementable safe integer.");
  }
  if (!Number.isSafeInteger(previous.taskContractRevision) || previous.taskContractRevision < 1) {
    throw new Error("A previous plan requires a positive safe-integer task contract revision.");
  }
  if (!Number.isSafeInteger(previous.createdAt) || previous.createdAt < 0) {
    throw new Error("A previous plan requires a non-negative safe-integer createdAt timestamp.");
  }
  if (previous.depth !== "light" && previous.depth !== "full") {
    throw new Error("A previous plan requires depth light or full.");
  }
  const plan: TaskPlan = {
    ...previous,
    revision: previous.revision + 1,
    ...buildContent(changes.content),
  };
  return deepFreeze(plan);
}

/** Deterministic digest over a plan revision (approval binding identity). */
export function computePlanRevisionDigest(plan: TaskPlan): string {
  return sha256Hex(canonicalizeJson(plan));
}

/** Compact deterministic description of a plan's public shape (for activity records). */
export function summarizePlan(plan: TaskPlan): string {
  const verified = plan.touchpoints.filter(
    (touchpoint) => touchpoint.confidence === "verified",
  ).length;
  const candidates = plan.touchpoints.length - verified;
  return [
    `${plan.depth} rev ${plan.revision}`,
    `${plan.steps.length} steps`,
    `${verified} verified / ${candidates} candidate touchpoints`,
  ].join(", ");
}

/**
 * Whether a contract carries meaningful acceptance criteria for full-plan
 * execution: at least two criteria, at least one host-verifiable
 * (non-user) criterion. Full-plan mutation execution is blocked until the
 * contract meets this bar (Part I §25).
 */
export function hasMeaningfulAcceptanceCriteria(contract: TaskContract): boolean {
  if (contract.acceptanceCriteria.length < 2) {
    return false;
  }
  return contract.acceptanceCriteria.some((criterion) => criterion.verificationKind !== "user");
}
