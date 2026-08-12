import type {
  AcceptanceCriterionId,
  TaskContract,
  ReviseTaskContractInput,
} from "./task-contract.js";
import { reviseTaskContract, validateTaskContract } from "./task-contract.js";
import type { TaskPlan, TaskPlanId } from "../planning/planning-model.js";
import type { PlanningDepth } from "../planning/planning-model.js";
import type { PlanningDecisionReason } from "../planning/planning-policy.js";
import type {
  EvidenceKind,
  EvidenceRef,
  EvidenceSource,
  FindingRef,
  ProgressState,
  TaskId,
  TaskPhase,
  TaskReviewStatus,
  TaskState,
  TaskStepId,
  TaskValidationStatus,
  WorkflowDisposition,
} from "./task-model.js";
import { isTerminalPhase } from "./task-model.js";
import type { TaskActivityEvent } from "./task-events.js";
import type { TaskRuntimeSnapshot } from "./task-snapshot.js";
import { deepFreeze } from "../domain/deep-freeze.js";
import type {
  CompletionEvaluation,
  CompletionResult,
  CreateTaskInput,
  CriterionResult,
  DispositionResult,
  EvidenceAttachResult,
  HostObservation,
  StepOpResult,
  TaskHandle,
  TaskRuntime,
  TaskRuntimeOptions,
} from "./task-runtime-model.js";
import type { TaskActivityDraft, TaskRecord, TaskRuntimeHooks } from "./task-runtime-record.js";
import {
  createInternalTaskProgress,
  observeTaskProgress,
  taskProgressSnapshot,
} from "./task-runtime-progress.js";
import { prepareTaskStepSpecs, validateAndCloneTaskFindings } from "./task-runtime-validation.js";
import {
  beginTaskStep,
  buildInitialTaskState,
  evaluateTaskCompletion,
  failTaskStep,
  finalizeTaskCompletion,
  markTaskCriterionFailed,
  reconcileTaskAcceptance,
  terminalTaskMutationReason,
  transitionTaskPhase,
} from "./task-runtime-state.js";
import {
  attachTaskEvidence,
  completeTaskStep,
  verifyTaskCriterion,
} from "./task-runtime-evidence.js";
import {
  approveTaskPlan,
  currentTaskPlan,
  invalidatePlanForContractRevision,
  invalidateTaskPlan,
  setTaskPlan,
  taskPlanRevisions,
} from "./task-runtime-planning.js";

export type {
  CompletionEvaluation,
  CompletionResult,
  CreateTaskInput,
  CriterionResult,
  DispositionResult,
  EvidenceAttachResult,
  HostObservation,
  StepOpResult,
  TaskHandle,
  TaskRuntime,
  TaskRuntimeOptions,
} from "./task-runtime-model.js";
export {
  PROGRESS_DEGRADED_REPETITIONS,
  PROGRESS_STALLED_REPETITIONS,
  PROGRESS_WINDOW_SIZE,
} from "./task-runtime-progress.js";
export {
  MAX_EVIDENCE_SOURCE_BYTES,
  MAX_TASK_EVIDENCE_ID_BYTES,
  MAX_TASK_EVIDENCE_RECORDS,
  MAX_TASK_FINDING_FIELD_BYTES,
  MAX_TASK_FINDINGS,
  MAX_TASK_STEP_DESCRIPTION_BYTES,
  MAX_TASK_STEPS,
} from "./task-runtime-validation.js";

/**
 * Host-owned structured task runtime (Stage 3 milestone 1).
 *
 * The runtime is the single authoritative owner of every mutable TaskState:
 * state is created, transitioned, and finalized only through this module's
 * handle API. Providers, adapters, the CLI, and the UI receive immutable
 * snapshots, projections, or events. Model input arrives only as typed
 * *requests* (dispositions, tool calls) — never as direct state mutation —
 * and `"complete"` is a completion request that still passes the host
 * completion gate. Task state is descriptive/control-flow state: it can
 * never grant capabilities, and security policy remains authoritative
 * elsewhere.
 *
 * The runtime is provider-neutral and sandbox-neutral: it observes typed
 * host observations for progress and never imports provider or sandbox
 * ports.
 */

export function createTaskRuntime(options: TaskRuntimeOptions = {}): TaskRuntime {
  const now = options.now ?? Date.now;
  const records = new Map<TaskId, TaskRecord>();

  function appendActivity(record: TaskRecord, event: TaskActivityDraft): void {
    record.sequence += 1;
    record.activity.push({
      ...(event as TaskActivityEvent),
      sequence: record.sequence,
      taskId: record.id,
      atMs: now(),
    });
  }

  function observeProgress(record: TaskRecord, observation: HostObservation): ProgressState {
    return observeTaskProgress(record.progress, observation, now);
  }

  const hooks: TaskRuntimeHooks = { now, appendActivity, observeProgress };

  function buildRecord(input: CreateTaskInput): TaskRecord {
    const specs = prepareTaskStepSpecs(input.steps);
    const contract = deepFreeze(structuredClone(input.contract));
    const snapshot = deepFreeze(structuredClone(input.snapshot));
    const record: TaskRecord = {
      id: contract.id,
      specs,
      contract,
      contractRevisions: [contract],
      snapshot,
      state: undefined as unknown as TaskRecord["state"],
      plans: [],
      planApproval: null,
      activity: [],
      sequence: 0,
      progress: createInternalTaskProgress(),
    };
    record.state = buildInitialTaskState(
      record,
      {
        ...input,
        contract,
        snapshot,
        steps: [...specs.values()],
      },
      now,
    );
    appendActivity(record, {
      type: "task_started",
      contractRevision: input.contract.revision,
    });
    observeProgress(record, {
      action: "task.started",
      fingerprint: input.contract.revision.toString(),
    });
    return record;
  }

  function createHandle(record: TaskRecord): TaskHandle {
    return {
      taskId: record.id,

      snapshot(): TaskState {
        return structuredClone(record.state);
      },
      contract(): TaskContract {
        return record.contract;
      },
      contractRevisions(): readonly TaskContract[] {
        return record.contractRevisions.map((revision) => structuredClone(revision));
      },
      reviseContract(changes: ReviseTaskContractInput): TaskContract {
        const terminalReason = terminalTaskMutationReason(record);
        if (terminalReason !== null) {
          throw new Error(terminalReason);
        }
        const revision = reviseTaskContract(record.contract, changes);
        record.contractRevisions.push(revision);
        record.contract = revision;
        record.state.contractRevision = revision.revision;
        record.state.contractDigest = revision.digest.value;
        reconcileTaskAcceptance(record, revision);
        invalidatePlanForContractRevision(record, hooks);
        appendActivity(record, {
          type: "task_contract_revised",
          revision: revision.revision,
        });
        return revision;
      },
      runtimeSnapshot(): TaskRuntimeSnapshot {
        return record.snapshot;
      },
      activityLog(): readonly TaskActivityEvent[] {
        return record.activity.map((event) => structuredClone(event));
      },
      recordExecutionInputManifest(inputManifestDigest: string): void {
        if (!/^[0-9a-f]{64}$/.test(inputManifestDigest)) {
          throw new Error(
            "An execution-input manifest digest must be 64 lowercase hex characters.",
          );
        }
        appendActivity(record, {
          type: "execution_input_recorded",
          inputManifestDigest,
        });
      },
      recordReproducibilityManifest(reproducibilityDigest: string): void {
        if (!/^[0-9a-f]{64}$/.test(reproducibilityDigest)) {
          throw new Error("A reproducibility manifest digest must be 64 lowercase hex characters.");
        }
        appendActivity(record, {
          type: "reproducibility_recorded",
          reproducibilityDigest,
        });
      },

      transitionPhase(phase: TaskPhase): StepOpResult {
        return transitionTaskPhase(record, phase, hooks);
      },
      beginStep(stepId: TaskStepId): StepOpResult {
        return beginTaskStep(record, stepId, hooks);
      },
      completeStep(stepId: TaskStepId, evidenceRefs: readonly EvidenceRef[]): StepOpResult {
        return completeTaskStep(record, stepId, evidenceRefs, hooks);
      },
      failStep(stepId: TaskStepId, reason: string): StepOpResult {
        return failTaskStep(record, stepId, reason, hooks);
      },
      attachEvidence(input: {
        readonly id: string;
        readonly kind: EvidenceKind;
        readonly source: EvidenceSource;
      }): EvidenceAttachResult {
        return attachTaskEvidence(record, input, hooks);
      },
      verifyCriterion(
        criterionId: AcceptanceCriterionId,
        verifiedBy: string | null,
        note?: string,
      ): CriterionResult {
        return verifyTaskCriterion(record, criterionId, verifiedBy, note, hooks);
      },
      markCriterionFailed(criterionId: AcceptanceCriterionId, note?: string): CriterionResult {
        return markTaskCriterionFailed(record, criterionId, note);
      },
      setFindings(findings: readonly FindingRef[]): void {
        if (terminalTaskMutationReason(record) !== null) {
          return;
        }
        record.state.currentFindings = validateAndCloneTaskFindings(findings);
      },
      setValidationStatus(status: TaskValidationStatus): void {
        if (terminalTaskMutationReason(record) !== null) {
          return;
        }
        record.state.validationStatus = status;
      },
      setReviewStatus(status: TaskReviewStatus): void {
        if (terminalTaskMutationReason(record) !== null) {
          return;
        }
        record.state.reviewStatus = status;
      },
      setIteration(iteration: number): void {
        if (terminalTaskMutationReason(record) !== null || !Number.isFinite(iteration)) {
          return;
        }
        record.state.iteration = Math.max(0, Math.floor(iteration));
      },

      routePlanning(depth: PlanningDepth, reason: PlanningDecisionReason): void {
        if (terminalTaskMutationReason(record) !== null) {
          return;
        }
        record.state.plan.depth = depth;
        appendActivity(record, { type: "planning_routed", depth, reason });
      },
      rejectPlan(reason: string): void {
        if (terminalTaskMutationReason(record) !== null) {
          return;
        }
        appendActivity(record, { type: "plan_rejected", reason });
      },
      setPlan(plan: TaskPlan): StepOpResult {
        return setTaskPlan(record, plan, hooks);
      },
      approvePlan(planId: TaskPlanId, planRevision: number): StepOpResult {
        return approveTaskPlan(record, planId, planRevision, hooks);
      },
      invalidatePlan(reason: string): void {
        invalidateTaskPlan(record, reason, hooks);
      },
      currentPlan(): TaskPlan | null {
        return currentTaskPlan(record);
      },
      planRevisions(): readonly TaskPlan[] {
        return taskPlanRevisions(record);
      },

      submitDisposition(
        disposition: WorkflowDisposition,
        source: "host" | "model" = "host",
      ): DispositionResult {
        const terminalReason = terminalTaskMutationReason(record);
        if (terminalReason !== null) {
          return {
            accepted: false,
            disposition,
            evaluation: disposition.type === "complete" ? evaluateTaskCompletion(record) : null,
            reason: terminalReason,
          };
        }
        if (disposition.type === "complete") {
          const evaluation = evaluateTaskCompletion(record);
          if (evaluation.allowed) {
            const completed = finalizeTaskCompletion(record, hooks);
            if (completed.status === "completed") {
              appendActivity(record, {
                type: "disposition_submitted",
                disposition,
                source,
                accepted: true,
                note: "completion gate passed",
              });
              return { accepted: true, disposition, evaluation, reason: null };
            }
          }
          const reason = evaluation.missing[0] ?? "completion gate not satisfied";
          appendActivity(record, {
            type: "disposition_submitted",
            disposition,
            source,
            accepted: false,
            note: reason,
          });
          return { accepted: false, disposition, evaluation, reason };
        }
        if (disposition.type === "blocked") {
          const transition = transitionTaskPhase(record, "blocked", hooks);
          if (transition.status === "rejected") {
            appendActivity(record, {
              type: "disposition_submitted",
              disposition,
              source,
              accepted: false,
              note: transition.reason,
            });
            return { accepted: false, disposition, evaluation: null, reason: transition.reason };
          }
          record.state.terminalReason = disposition.reason;
          appendActivity(record, { type: "task_blocked", reason: disposition.reason });
          appendActivity(record, {
            type: "disposition_submitted",
            disposition,
            source,
            accepted: true,
            note: null,
          });
          return { accepted: true, disposition, evaluation: null, reason: null };
        }
        appendActivity(record, {
          type: "disposition_submitted",
          disposition,
          source,
          accepted: true,
          note: disposition.nextAction ?? null,
        });
        return { accepted: true, disposition, evaluation: null, reason: null };
      },
      evaluateCompletion(): CompletionEvaluation {
        return evaluateTaskCompletion(record);
      },
      completeTask(): CompletionResult {
        return finalizeTaskCompletion(record, hooks);
      },
      cancel(reason: string): void {
        if (isTerminalPhase(record.state.phase)) {
          return;
        }
        record.state.phase = "cancelled";
        record.state.completedAtMs = now();
        record.state.terminalReason = reason;
        appendActivity(record, { type: "task_cancelled", reason });
      },
      fail(reason: string): void {
        if (isTerminalPhase(record.state.phase)) {
          return;
        }
        record.state.phase = "failed";
        record.state.completedAtMs = now();
        record.state.terminalReason = reason;
        appendActivity(record, { type: "task_failed", reason });
      },
      markBlocked(reason: string): void {
        const transition = transitionTaskPhase(record, "blocked", hooks);
        if (transition.status === "ok") {
          record.state.terminalReason = reason;
          appendActivity(record, { type: "task_blocked", reason });
        }
      },

      observe(observation: HostObservation): ProgressState {
        if (terminalTaskMutationReason(record) !== null) {
          return taskProgressSnapshot(record.progress);
        }
        return observeProgress(record, observation);
      },
      progress(): ProgressState {
        return taskProgressSnapshot(record.progress);
      },
    };
  }

  return {
    createTask(input: CreateTaskInput): TaskHandle {
      const contract = validateTaskContract(input.contract);
      if (records.has(contract.id)) {
        throw new Error(`A task with id ${contract.id} already exists.`);
      }
      const record = buildRecord({ ...input, contract });
      records.set(record.id, record);
      return createHandle(record);
    },
    getTask(taskId: TaskId): TaskHandle | null {
      const record = records.get(taskId);
      return record === undefined ? null : createHandle(record);
    },
    listTasks(): readonly TaskHandle[] {
      return [...records.values()].map((record) => createHandle(record));
    },
    latestTask(): TaskHandle | null {
      const record = [...records.values()].pop();
      return record === undefined ? null : createHandle(record);
    },
  };
}
