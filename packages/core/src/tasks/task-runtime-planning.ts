import { deepFreeze } from "../domain/deep-freeze.js";
import {
  PLAN_ID_PATTERN,
  PLANNING_LIMITS,
  type TaskPlan,
  type TaskPlanId,
} from "../planning/planning-model.js";
import { validatePlanCandidate } from "../planning/planning-validation.js";
import { computeTaskPlanContentDigest } from "../identity/contract-plan-identity.js";
import type { StepOpResult } from "./task-runtime-model.js";
import type { TaskRecord, TaskRuntimeHooks } from "./task-runtime-record.js";
import { terminalTaskMutationReason } from "./task-runtime-state.js";

export function invalidatePlanForContractRevision(
  record: TaskRecord,
  hooks: TaskRuntimeHooks,
): void {
  const current = record.plans[record.plans.length - 1];
  if (current === undefined) {
    return;
  }
  record.state.plan.state = "stale";
  record.state.plan.staleReason = "The TaskContract revision advanced after this plan was created.";
  if (record.planApproval !== null) {
    record.state.plan.approval = "invalidated";
    hooks.appendActivity(record, {
      type: "plan_invalidated",
      planId: record.planApproval.planId,
      revision: record.planApproval.planRevision,
      reason: "The TaskContract revision advanced; the plan approval no longer applies.",
    });
  }
  record.planApproval = null;
  hooks.appendActivity(record, {
    type: "plan_invalidated",
    planId: current.id,
    revision: current.revision,
    reason: "The TaskContract revision advanced; the plan is stale until revalidated or replanned.",
  });
}

export function setTaskPlan(
  record: TaskRecord,
  plan: TaskPlan,
  hooks: TaskRuntimeHooks,
): StepOpResult {
  const terminalReason = terminalTaskMutationReason(record);
  if (terminalReason !== null) {
    return { status: "rejected", reason: terminalReason };
  }
  if (plan.taskId !== record.id) {
    return {
      status: "rejected",
      reason: `Plan ${plan.id} belongs to task ${plan.taskId}, not ${record.id}.`,
    };
  }
  if (plan.taskContractRevision !== record.contract.revision) {
    return {
      status: "rejected",
      reason: `Plan ${plan.id} binds to TaskContract revision ${plan.taskContractRevision}, but the current revision is ${record.contract.revision}.`,
    };
  }
  if (!PLAN_ID_PATTERN.test(plan.id)) {
    return { status: "rejected", reason: `Invalid plan id: ${plan.id}` };
  }
  if (!Number.isSafeInteger(plan.revision) || plan.revision < 1) {
    return { status: "rejected", reason: "A plan revision must be a positive safe integer." };
  }
  if (!Number.isSafeInteger(plan.createdAt) || plan.createdAt < 0) {
    return { status: "rejected", reason: "A plan requires a valid createdAt timestamp." };
  }
  const validated = validatePlanCandidate(plan, {
    contract: record.contract,
    depth: plan.depth,
  });
  if (!validated.ok) {
    return { status: "rejected", reason: `The plan is invalid: ${validated.reasons.join(" ")}` };
  }
  const previous = record.plans[record.plans.length - 1] ?? null;
  if (previous === null && plan.revision !== 1) {
    return { status: "rejected", reason: "The first plan revision must be 1." };
  }
  if (previous !== null && previous.id === plan.id && plan.revision !== previous.revision + 1) {
    return {
      status: "rejected",
      reason: `Plan ${plan.id} revision ${plan.revision} does not follow revision ${previous.revision}; plans are immutable and revisions only ever advance by one.`,
    };
  }
  if (previous !== null && previous.id !== plan.id) {
    if (plan.revision !== 1) {
      return {
        status: "rejected",
        reason: `Replacement plan ${plan.id} must begin at revision 1.`,
      };
    }
    if (record.plans.some((revision) => revision.id === plan.id)) {
      return {
        status: "rejected",
        reason: `Plan id ${plan.id} was already used by this task and cannot be restarted.`,
      };
    }
  }
  if (record.plans.length >= PLANNING_LIMITS.maxPlanRevisions) {
    return {
      status: "rejected",
      reason: `The task already holds the maximum of ${PLANNING_LIMITS.maxPlanRevisions} plan revisions; replanning is not possible within this bound.`,
    };
  }
  const priorApproved =
    record.planApproval !== null &&
    (record.planApproval.planId !== plan.id || record.planApproval.planRevision !== plan.revision);
  const storedPlan: TaskPlan = deepFreeze({
    id: plan.id,
    revision: plan.revision,
    digest: plan.digest,
    taskId: plan.taskId,
    taskContractRevision: plan.taskContractRevision,
    taskContractDigest: plan.taskContractDigest,
    depth: plan.depth,
    ...structuredClone(validated.content),
    createdAt: plan.createdAt,
  });
  record.plans.push(storedPlan);
  record.state.plan.planId = storedPlan.id;
  record.state.plan.planRevision = storedPlan.revision;
  record.state.plan.planDigest = storedPlan.digest.value;
  record.state.plan.depth = storedPlan.depth;
  record.state.plan.state = "current";
  record.state.plan.staleReason = null;
  if (priorApproved) {
    record.state.plan.approval = "invalidated";
    hooks.appendActivity(record, {
      type: "plan_invalidated",
      planId: record.planApproval!.planId,
      revision: record.planApproval!.planRevision,
      reason: "The plan identity or revision advanced; the previous approval no longer applies.",
    });
  } else {
    record.state.plan.approval = "none";
  }
  record.planApproval = null;
  hooks.appendActivity(record, {
    type: "plan_created",
    planId: storedPlan.id,
    revision: storedPlan.revision,
    depth: storedPlan.depth,
  });
  hooks.observeProgress(record, {
    action: "plan.created",
    fingerprint: `${storedPlan.id}:${storedPlan.revision}`,
    progress: true,
  });
  return { status: "ok" };
}

export function approveTaskPlan(
  record: TaskRecord,
  planId: TaskPlanId,
  planRevision: number,
  hooks: TaskRuntimeHooks,
): StepOpResult {
  const terminalReason = terminalTaskMutationReason(record);
  if (terminalReason !== null) {
    return { status: "rejected", reason: terminalReason };
  }
  const current = record.plans[record.plans.length - 1] ?? null;
  if (current === null || current.id !== planId) {
    return {
      status: "rejected",
      reason: `No current plan matches ${planId}; nothing was approved.`,
    };
  }
  if (current.revision !== planRevision) {
    return {
      status: "rejected",
      reason: `Approval binds to the exact plan revision: plan ${planId} is revision ${current.revision}, not ${planRevision}; the stale approval is refused.`,
    };
  }
  if (current.taskContractRevision !== record.contract.revision) {
    return {
      status: "rejected",
      reason: `Plan ${planId} binds to TaskContract revision ${current.taskContractRevision}, which is no longer current; the approval is refused.`,
    };
  }
  if (current.taskContractDigest !== record.contract.digest.value) {
    return {
      status: "rejected",
      reason: `Plan ${planId} binds to a different TaskContract content digest; the approval is refused.`,
    };
  }
  if (current.digest.value !== computeTaskPlanContentDigest(current)) {
    return {
      status: "rejected",
      reason: `Plan ${planId} content does not match its own identity digest; the approval is refused.`,
    };
  }
  if (record.state.plan.state === "stale") {
    return { status: "rejected", reason: "The current plan is stale and cannot be approved." };
  }
  record.planApproval = {
    planId: current.id,
    planRevision: current.revision,
    planDigest: current.digest.value,
    taskContractRevision: current.taskContractRevision,
    taskContractDigest: current.taskContractDigest,
    approvedAt: hooks.now(),
  };
  record.state.plan.approval = "approved";
  hooks.appendActivity(record, {
    type: "plan_approved",
    planId: current.id,
    revision: current.revision,
    digest: current.digest.value,
  });
  return { status: "ok" };
}

export function invalidateTaskPlan(
  record: TaskRecord,
  reason: string,
  hooks: TaskRuntimeHooks,
): void {
  if (terminalTaskMutationReason(record) !== null) {
    return;
  }
  const current = record.plans[record.plans.length - 1] ?? null;
  if (current === null) {
    return;
  }
  record.state.plan.state = "stale";
  record.state.plan.staleReason = reason;
  if (record.planApproval !== null) {
    record.state.plan.approval = "invalidated";
  }
  record.planApproval = null;
  hooks.appendActivity(record, {
    type: "plan_invalidated",
    planId: current.id,
    revision: current.revision,
    reason,
  });
}

export function currentTaskPlan(record: TaskRecord): TaskPlan | null {
  const current = record.plans[record.plans.length - 1] ?? null;
  return current === null ? null : structuredClone(current);
}

export function taskPlanRevisions(record: TaskRecord): readonly TaskPlan[] {
  return record.plans.map((plan) => structuredClone(plan));
}
