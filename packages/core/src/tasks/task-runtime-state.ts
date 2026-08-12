import { NO_TASK_PLAN } from "../planning/planning-model.js";
import type { TaskContract } from "./task-contract.js";
import type { AcceptanceState, TaskPhase, TaskStepId } from "./task-model.js";
import { isTerminalPhase } from "./task-model.js";
import type {
  CompletionEvaluation,
  CompletionResult,
  CreateTaskInput,
  CriterionResult,
  StepOpResult,
} from "./task-runtime-model.js";
import type {
  Mutable,
  MutableTaskState,
  TaskRecord,
  TaskRuntimeHooks,
} from "./task-runtime-record.js";
import { taskProgressSnapshot } from "./task-runtime-progress.js";
import { normalizeTaskIteration } from "./task-runtime-validation.js";

const ALLOWED_TRANSITIONS: Readonly<Record<TaskPhase, readonly TaskPhase[]>> = {
  prepared: ["working", "cancelled", "failed"],
  working: ["validating", "reviewing", "blocked", "cancelled", "failed"],
  validating: ["working", "reviewing", "blocked", "cancelled", "failed"],
  reviewing: ["working", "validating", "blocked", "cancelled", "failed"],
  blocked: ["working", "cancelled", "failed"],
  completed: [],
  cancelled: [],
  failed: [],
};

export function buildInitialTaskState(
  record: TaskRecord,
  input: CreateTaskInput,
  now: () => number,
): MutableTaskState {
  const steps = (input.steps ?? []).map((spec) => ({
    id: spec.id,
    description: spec.description,
    kind: spec.kind,
    status: "pending" as const,
    evidenceRefs: [],
    failedReason: null,
    blockedReason: null,
  }));
  const acceptance: Mutable<AcceptanceState>[] = record.contract.acceptanceCriteria.map(
    (criterion) => ({
      criterionId: criterion.id,
      description: criterion.description,
      verificationKind: criterion.verificationKind,
      status: "pending",
      verifiedBy: null,
      note: null,
    }),
  );
  return {
    taskId: record.id,
    contractRevision: record.contract.revision,
    contractDigest: record.contract.digest.value,
    phase: "prepared",
    plan: { ...NO_TASK_PLAN },
    steps,
    acceptance,
    currentFindings: [],
    evidence: [],
    validationStatus: "not_run",
    reviewStatus: "not_run",
    iteration: normalizeTaskIteration(input.iteration),
    progress: taskProgressSnapshot(record.progress),
    startedAtMs: now(),
    completedAtMs: null,
    terminalReason: null,
  };
}

export function findTaskStep(record: TaskRecord, stepId: TaskStepId) {
  return record.state.steps.find((step) => step.id === stepId) ?? null;
}

export function reconcileTaskAcceptance(record: TaskRecord, contract: TaskContract): void {
  const previous = new Map(record.state.acceptance.map((entry) => [entry.criterionId, entry]));
  record.state.acceptance = contract.acceptanceCriteria.map((criterion) => {
    const existing = previous.get(criterion.id);
    if (
      existing !== undefined &&
      existing.description === criterion.description &&
      existing.verificationKind === criterion.verificationKind
    ) {
      return { ...existing };
    }
    return {
      criterionId: criterion.id,
      description: criterion.description,
      verificationKind: criterion.verificationKind,
      status: "pending",
      verifiedBy: null,
      note: null,
    };
  });
}

export function terminalTaskMutationReason(record: TaskRecord): string | null {
  return isTerminalPhase(record.state.phase)
    ? `The task is terminal (${record.state.phase}); authoritative state can no longer be changed.`
    : null;
}

export function transitionTaskPhase(
  record: TaskRecord,
  phase: TaskPhase,
  hooks: TaskRuntimeHooks,
): StepOpResult {
  if (record.state.phase === phase) {
    return { status: "rejected", reason: `The task is already ${phase}.` };
  }
  if (!ALLOWED_TRANSITIONS[record.state.phase].includes(phase)) {
    return {
      status: "rejected",
      reason: `Phase transition ${record.state.phase} -> ${phase} is not allowed.`,
    };
  }
  record.state.phase = phase;
  if (isTerminalPhase(phase)) {
    record.state.completedAtMs = hooks.now();
  }
  hooks.appendActivity(record, { type: "task_phase_changed", phase });
  return { status: "ok" };
}

export function beginTaskStep(
  record: TaskRecord,
  stepId: TaskStepId,
  hooks: TaskRuntimeHooks,
): StepOpResult {
  const terminalReason = terminalTaskMutationReason(record);
  if (terminalReason !== null) {
    return { status: "rejected", reason: terminalReason };
  }
  const step = findTaskStep(record, stepId);
  if (step === null) {
    return { status: "rejected", reason: `Unknown step: ${stepId}` };
  }
  if (step.status === "active") {
    return { status: "rejected", reason: `Step ${stepId} is already active.` };
  }
  if (step.status === "completed") {
    return { status: "rejected", reason: `Step ${stepId} is already completed.` };
  }
  step.status = "active";
  step.failedReason = null;
  step.blockedReason = null;
  hooks.appendActivity(record, { type: "step_started", stepId });
  return { status: "ok" };
}

export function failTaskStep(
  record: TaskRecord,
  stepId: TaskStepId,
  reason: string,
  hooks: TaskRuntimeHooks,
): StepOpResult {
  const terminalReason = terminalTaskMutationReason(record);
  if (terminalReason !== null) {
    return { status: "rejected", reason: terminalReason };
  }
  const step = findTaskStep(record, stepId);
  if (step === null) {
    return { status: "rejected", reason: `Unknown step: ${stepId}` };
  }
  if (step.status === "completed") {
    return { status: "rejected", reason: `Step ${stepId} is already completed.` };
  }
  step.status = "failed";
  step.failedReason = reason;
  hooks.appendActivity(record, { type: "step_failed", stepId, reason });
  return { status: "ok" };
}

export function markTaskCriterionFailed(
  record: TaskRecord,
  criterionId: string,
  note?: string,
): CriterionResult {
  const terminalReason = terminalTaskMutationReason(record);
  if (terminalReason !== null) {
    return { status: "rejected", reason: terminalReason };
  }
  const criterion = record.state.acceptance.find((entry) => entry.criterionId === criterionId);
  if (criterion === undefined) {
    return { status: "rejected", reason: `Unknown acceptance criterion: ${criterionId}` };
  }
  criterion.status = "failed";
  criterion.verifiedBy = null;
  criterion.note = note ?? null;
  return { status: "failed", reason: null };
}

export function evaluateTaskCompletion(record: TaskRecord): CompletionEvaluation {
  const missing: string[] = [];
  for (const step of record.state.steps) {
    if (step.status !== "completed") {
      missing.push(`step not completed: ${step.id}`);
    }
  }
  for (const criterion of record.state.acceptance) {
    if (criterion.status !== "satisfied") {
      missing.push(`acceptance criterion not satisfied: ${criterion.criterionId}`);
    }
  }
  if (record.state.validationStatus !== "clean" && record.state.validationStatus !== "warnings") {
    missing.push(`validation is ${record.state.validationStatus} (clean required)`);
  }
  if (record.state.reviewStatus !== "clean") {
    missing.push(`review is ${record.state.reviewStatus} (clean required)`);
  }
  const blocking = record.state.currentFindings.filter(
    (finding) => finding.severity === "critical" || finding.severity === "high",
  );
  if (blocking.length > 0) {
    missing.push(`${blocking.length} blocking finding(s) unresolved`);
  }
  return { allowed: missing.length === 0, missing };
}

export function finalizeTaskCompletion(
  record: TaskRecord,
  hooks: TaskRuntimeHooks,
): CompletionResult {
  const terminalReason = terminalTaskMutationReason(record);
  if (terminalReason !== null) {
    return { status: "rejected", reasons: [terminalReason] };
  }
  const evaluation = evaluateTaskCompletion(record);
  if (!evaluation.allowed) {
    return { status: "rejected", reasons: [...evaluation.missing] };
  }
  record.state.phase = "completed";
  record.state.completedAtMs = hooks.now();
  record.state.terminalReason = null;
  hooks.appendActivity(record, { type: "task_completed" });
  hooks.observeProgress(record, {
    action: "task.completed",
    fingerprint: record.id,
    progress: true,
  });
  return { status: "completed" };
}
