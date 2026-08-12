import type { TaskContract } from "../tasks/task-contract.js";
import type { TaskPlan } from "../planning/planning-model.js";
import { computeArtifactDigest, type ArtifactDigest } from "./artifact-digest.js";
import { computeSectionDelta } from "./semantic-delta.js";

/**
 * TaskContract / TaskPlan content identity (Stage 3 — Content Identity &
 * Delta Verification, ADR 0028).
 *
 * revision = chronological identity; digest = exact content identity.
 * The digest is computed over CONTENT ONLY (revision is excluded), so
 * content-identical revisions share a digest and any material content
 * change produces a new digest. Deltas describe what changed between two
 * exact identities; the authoritative state remains the full current
 * artifact.
 */

export const TASK_CONTRACT_IDENTITY_SCHEMA = 1;
export const TASK_PLAN_IDENTITY_SCHEMA = 1;

/** Canonical content payload of a contract revision (revision excluded). */
export function taskContractContentPayload(contract: Omit<TaskContract, "digest">): unknown {
  return {
    id: contract.id,
    request: contract.request,
    ...(contract.context === undefined ? {} : { context: contract.context }),
    constraints: contract.constraints,
    acceptanceCriteria: contract.acceptanceCriteria,
    pausePolicy: contract.pausePolicy,
  };
}

/** Typed content digest of a contract revision. */
export function computeTaskContractArtifactDigest(
  contract: Omit<TaskContract, "digest">,
): ArtifactDigest {
  return computeArtifactDigest({
    artifactType: "TaskContract",
    schemaVersion: TASK_CONTRACT_IDENTITY_SCHEMA,
    payload: taskContractContentPayload(contract),
  });
}

/** Hex content digest of a contract revision. */
export function computeTaskContractContentDigest(contract: Omit<TaskContract, "digest">): string {
  return computeTaskContractArtifactDigest(contract).value;
}

export type TaskContractChangeSection =
  "request" | "context" | "constraints" | "acceptanceCriteria" | "pausePolicy";

/** Derived semantic delta between two contract revisions. */
export interface TaskContractDelta {
  readonly baseRevision: number;
  readonly resultRevision: number;
  readonly baseDigest: string;
  readonly resultDigest: string;
  readonly changed: readonly TaskContractChangeSection[];
  readonly unchanged: readonly TaskContractChangeSection[];
  readonly unchangedContent: boolean;
}

export function computeTaskContractDelta(
  base: TaskContract,
  result: TaskContract,
): TaskContractDelta {
  const sectionKeys: readonly TaskContractChangeSection[] = [
    "request",
    "context",
    "constraints",
    "acceptanceCriteria",
    "pausePolicy",
  ];
  const sections: Record<TaskContractChangeSection, unknown> = {
    request: base.request,
    context: base.context,
    constraints: base.constraints,
    acceptanceCriteria: base.acceptanceCriteria,
    pausePolicy: base.pausePolicy,
  };
  const resultSections: Record<TaskContractChangeSection, unknown> = {
    request: result.request,
    context: result.context,
    constraints: result.constraints,
    acceptanceCriteria: result.acceptanceCriteria,
    pausePolicy: result.pausePolicy,
  };
  const { changed, unchanged } = computeSectionDelta(sections, resultSections, sectionKeys);
  return {
    baseRevision: base.revision,
    resultRevision: result.revision,
    baseDigest: computeTaskContractContentDigest(base),
    resultDigest: computeTaskContractContentDigest(result),
    changed: changed as TaskContractChangeSection[],
    unchanged: unchanged as TaskContractChangeSection[],
    unchangedContent: changed.length === 0,
  };
}

/** Canonical content payload of a plan revision (identity fields excluded). */
export function taskPlanContentPayload(plan: Omit<TaskPlan, "digest">): unknown {
  return {
    objective: plan.objective,
    scope: plan.scope,
    nonGoals: plan.nonGoals,
    touchpoints: plan.touchpoints,
    constraints: plan.constraints,
    risks: plan.risks,
    steps: plan.steps,
    validation: plan.validation,
    ...(plan.rollback === undefined ? {} : { rollback: plan.rollback }),
    ...(plan.rationale === undefined ? {} : { rationale: plan.rationale }),
  };
}

/** Typed content digest of a plan revision. */
export function computeTaskPlanArtifactDigest(plan: Omit<TaskPlan, "digest">): ArtifactDigest {
  return computeArtifactDigest({
    artifactType: "TaskPlan",
    schemaVersion: TASK_PLAN_IDENTITY_SCHEMA,
    payload: taskPlanContentPayload(plan),
  });
}

/** Hex content digest of a plan revision. */
export function computeTaskPlanContentDigest(plan: Omit<TaskPlan, "digest">): string {
  return computeTaskPlanArtifactDigest(plan).value;
}

export type TaskPlanChangeSection =
  | "objective"
  | "scope"
  | "nonGoals"
  | "touchpoints"
  | "constraints"
  | "risks"
  | "steps"
  | "validation"
  | "rollback"
  | "rationale";

/** Derived semantic delta between two plan revisions. */
export interface TaskPlanDelta {
  readonly planId: string;
  readonly baseRevision: number;
  readonly resultRevision: number;
  readonly baseDigest: string;
  readonly resultDigest: string;
  readonly changed: readonly TaskPlanChangeSection[];
  readonly unchanged: readonly TaskPlanChangeSection[];
  readonly unchangedContent: boolean;
  /** True when the plan's TaskContract binding changed. */
  readonly contractBindingChanged: boolean;
}

export function computeTaskPlanDelta(base: TaskPlan, result: TaskPlan): TaskPlanDelta {
  const sectionKeys: readonly TaskPlanChangeSection[] = [
    "objective",
    "scope",
    "nonGoals",
    "touchpoints",
    "constraints",
    "risks",
    "steps",
    "validation",
    "rollback",
    "rationale",
  ];
  const sections: Record<TaskPlanChangeSection, unknown> = {
    objective: base.objective,
    scope: base.scope,
    nonGoals: base.nonGoals,
    touchpoints: base.touchpoints,
    constraints: base.constraints,
    risks: base.risks,
    steps: base.steps,
    validation: base.validation,
    rollback: base.rollback,
    rationale: base.rationale,
  };
  const resultSections: Record<TaskPlanChangeSection, unknown> = {
    objective: result.objective,
    scope: result.scope,
    nonGoals: result.nonGoals,
    touchpoints: result.touchpoints,
    constraints: result.constraints,
    risks: result.risks,
    steps: result.steps,
    validation: result.validation,
    rollback: result.rollback,
    rationale: result.rationale,
  };
  const { changed, unchanged } = computeSectionDelta(sections, resultSections, sectionKeys);
  return {
    planId: base.id,
    baseRevision: base.revision,
    resultRevision: result.revision,
    baseDigest: computeTaskPlanContentDigest(base),
    resultDigest: computeTaskPlanContentDigest(result),
    changed: changed as TaskPlanChangeSection[],
    unchanged: unchanged as TaskPlanChangeSection[],
    unchangedContent: changed.length === 0,
    contractBindingChanged: base.taskContractDigest !== result.taskContractDigest,
  };
}
