import type {
  AcceptanceCriterionId,
  ReviseTaskContractInput,
  TaskContract,
} from "./task-contract.js";
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
  TaskStepSpec,
  TaskValidationStatus,
  WorkflowDisposition,
} from "./task-model.js";
import type { TaskActivityEvent } from "./task-events.js";
import type { TaskRuntimeSnapshot } from "./task-snapshot.js";

export interface TaskRuntimeOptions {
  readonly now?: () => number;
}

/** One host-observed execution fact fed to the progress tracker. */
export interface HostObservation {
  /** Canonical action identity, e.g. `tool.workspace.read`. */
  readonly action: string;
  /** Canonical result fingerprint: equal results produce equal values. */
  readonly fingerprint: string;
  /** Host asserts this observation represents genuinely new useful state. */
  readonly progress?: boolean;
}

export interface CreateTaskInput {
  readonly contract: TaskContract;
  /** Immutable configuration snapshot captured when the task starts. */
  readonly snapshot: TaskRuntimeSnapshot;
  readonly steps?: readonly TaskStepSpec[];
  readonly iteration?: number;
}

export type StepOpResult =
  { readonly status: "ok" } | { readonly status: "rejected"; readonly reason: string };

export interface EvidenceAttachResult {
  readonly status: "attached" | "rejected";
  readonly reason: string | null;
}

export interface CompletionEvaluation {
  readonly allowed: boolean;
  readonly missing: readonly string[];
}

export type CompletionResult =
  | { readonly status: "completed" }
  | { readonly status: "rejected"; readonly reasons: readonly string[] };

export interface DispositionResult {
  readonly accepted: boolean;
  readonly disposition: WorkflowDisposition;
  /** Completion-gate evaluation when the disposition was a completion request. */
  readonly evaluation: CompletionEvaluation | null;
  readonly reason: string | null;
}

export interface CriterionResult {
  readonly status: "verified" | "failed" | "rejected";
  readonly reason: string | null;
}

export interface TaskHandle {
  readonly taskId: TaskId;

  /** Immutable snapshot of the authoritative state (fresh copy). */
  snapshot(): TaskState;
  contract(): TaskContract;
  /** All contract revisions, oldest first (immutable history). */
  contractRevisions(): readonly TaskContract[];
  /** Produce the next immutable contract revision; the current revision is never mutated. */
  reviseContract(changes: ReviseTaskContractInput): TaskContract;
  runtimeSnapshot(): TaskRuntimeSnapshot;
  /** Append-only activity records (fresh copies). */
  activityLog(): readonly TaskActivityEvent[];
  /**
   * Record the exact execution-input manifest digest of an execution
   * iteration (ADR 0028). Digest references only — the full manifest
   * stays with the host.
   */
  recordExecutionInputManifest(inputManifestDigest: string): void;

  /** Host-controlled phase transition; validated against the transition table. */
  transitionPhase(phase: TaskPhase): StepOpResult;
  beginStep(stepId: TaskStepId): StepOpResult;
  /** Evidence-backed completion: refs must exist, be task-scoped, and be accepted by the step. */
  completeStep(stepId: TaskStepId, evidenceRefs: readonly EvidenceRef[]): StepOpResult;
  failStep(stepId: TaskStepId, reason: string): StepOpResult;
  attachEvidence(input: {
    readonly id: string;
    readonly kind: EvidenceKind;
    readonly source: EvidenceSource;
  }): EvidenceAttachResult;
  verifyCriterion(
    criterionId: AcceptanceCriterionId,
    verifiedBy: string | null,
    note?: string,
  ): CriterionResult;
  markCriterionFailed(criterionId: AcceptanceCriterionId, note?: string): CriterionResult;
  /** Replace the task's evidence-backed findings list (host-observed). */
  setFindings(findings: readonly FindingRef[]): void;
  setValidationStatus(status: TaskValidationStatus): void;
  setReviewStatus(status: TaskReviewStatus): void;
  setIteration(iteration: number): void;

  /** Record the host's planning-depth routing (deterministic policy). */
  routePlanning(depth: PlanningDepth, reason: PlanningDecisionReason): void;
  /** Record a host-observed plan rejection (invalid candidate, denial). */
  rejectPlan(reason: string): void;
  /** Store an immutable plan revision bound to the current TaskContract. */
  setPlan(plan: TaskPlan): StepOpResult;
  /** Bind approval to the exact current plan and contract revisions. */
  approvePlan(planId: TaskPlanId, planRevision: number): StepOpResult;
  /** Mark the current plan stale and its approval invalid. */
  invalidatePlan(reason: string): void;
  currentPlan(): TaskPlan | null;
  planRevisions(): readonly TaskPlan[];

  submitDisposition(disposition: WorkflowDisposition, source?: "host" | "model"): DispositionResult;
  evaluateCompletion(): CompletionEvaluation;
  completeTask(): CompletionResult;
  cancel(reason: string): void;
  fail(reason: string): void;
  markBlocked(reason: string): void;

  observe(observation: HostObservation): ProgressState;
  progress(): ProgressState;
}

export interface TaskRuntime {
  createTask(input: CreateTaskInput): TaskHandle;
  getTask(taskId: TaskId): TaskHandle | null;
  listTasks(): readonly TaskHandle[];
  /** Most recently created task. */
  latestTask(): TaskHandle | null;
}
