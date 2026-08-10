import type {
  AcceptanceCriterionId,
  TaskContract,
  ReviseTaskContractInput,
} from "./task-contract.js";
import { reviseTaskContract, validateTaskContract } from "./task-contract.js";
import type { TaskPlan, TaskPlanId, TaskPlanState } from "../planning/planning-model.js";
import { NO_TASK_PLAN, PLAN_ID_PATTERN, PLANNING_LIMITS } from "../planning/planning-model.js";
import type { PlanningDepth } from "../planning/planning-model.js";
import type { PlanningDecisionReason } from "../planning/planning-policy.js";
import type {
  AcceptanceState,
  EvidenceKind,
  EvidenceRecord,
  EvidenceRef,
  EvidenceSource,
  FindingRef,
  ProgressState,
  ProgressStateValue,
  TaskId,
  TaskPhase,
  TaskReviewStatus,
  TaskState,
  TaskStepId,
  TaskStepSpec,
  TaskValidationStatus,
  WorkflowDisposition,
} from "./task-model.js";
import { isTerminalPhase } from "./task-model.js";
import type { TaskActivityEvent } from "./task-events.js";
import type { TaskRuntimeSnapshot } from "./task-snapshot.js";
import { deepFreeze } from "../domain/deep-freeze.js";
import { validatePlanCandidate } from "../planning/planning-validation.js";

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

/** Evidence sources are bounded references; never embed raw adapter output. */
export const MAX_EVIDENCE_SOURCE_BYTES = 4096;
/** Hard per-task evidence-record bound; task state cannot grow without limit. */
export const MAX_TASK_EVIDENCE_RECORDS = 256;
/** Hard task-shape bounds enforced before authoritative state is created. */
export const MAX_TASK_STEPS = 128;
export const MAX_TASK_FINDINGS = 128;
export const MAX_TASK_STEP_DESCRIPTION_BYTES = 4096;
export const MAX_TASK_FINDING_FIELD_BYTES = 4096;
export const MAX_TASK_EVIDENCE_ID_BYTES = 256;

/** Bounded recent-observation window for stuck-pattern detection. */
export const PROGRESS_WINDOW_SIZE = 8;
export const PROGRESS_DEGRADED_REPETITIONS = 3;
export const PROGRESS_STALLED_REPETITIONS = 5;

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
  /**
   * Store an immutable plan revision (host-owned). The plan must bind to
   * the current TaskContract revision; advancing a plan revision
   * invalidates any prior approval of an older revision.
   */
  setPlan(plan: TaskPlan): StepOpResult;
  /**
   * Bind approval to the EXACT current plan revision and current contract
   * revision. Approving never authorizes source edits or commands.
   */
  approvePlan(planId: TaskPlanId, planRevision: number): StepOpResult;
  /** Mark the current plan stale and its approval invalid (host decision). */
  invalidatePlan(reason: string): void;
  /** The full immutable current plan, when any. */
  currentPlan(): TaskPlan | null;
  /** Immutable plan revision history, oldest first. */
  planRevisions(): readonly TaskPlan[];

  /** Structured workflow disposition: a request that the host runtime evaluates. */
  submitDisposition(disposition: WorkflowDisposition, source?: "host" | "model"): DispositionResult;
  /** Host completion gate: completion requires steps, criteria, validation, review, findings. */
  evaluateCompletion(): CompletionEvaluation;
  /** Host finalize: completes only when the completion gate allows it. */
  completeTask(): CompletionResult;
  cancel(reason: string): void;
  fail(reason: string): void;
  markBlocked(reason: string): void;

  /** Feed one host-observed execution fact into the progress tracker. */
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

type Mutable<T> = {
  -readonly [K in keyof T]: T[K] extends readonly (infer U)[] ? Mutable<U>[] : T[K];
};

type MutableTaskState = Omit<Mutable<TaskState>, "plan"> & { plan: MutablePlanState };

/** The TaskState plan reference is mutable ONLY inside the runtime record. */
type MutablePlanState = {
  -readonly [K in keyof TaskPlanState]: TaskPlanState[K];
};

type DistributiveOmit<T, K extends keyof never> = T extends unknown ? Omit<T, K> : never;

interface InternalProgress {
  usefulObservations: number;
  repeatedActions: number;
  state: ProgressStateValue;
  lastProgressAtMs: number | null;
  stalledAtMs: number | null;
  window: Array<{ readonly key: string; readonly useful: boolean }>;
}

interface TaskRecord {
  readonly id: TaskId;
  readonly specs: ReadonlyMap<string, TaskStepSpec>;
  contract: TaskContract;
  readonly contractRevisions: TaskContract[];
  readonly snapshot: TaskRuntimeSnapshot;
  state: MutableTaskState;
  /** Immutable plan revision history, oldest first (host-inspectable). */
  readonly plans: TaskPlan[];
  /** Most recent plan approval; binds to exact plan + contract revisions. */
  planApproval: {
    readonly planId: TaskPlanId;
    readonly planRevision: number;
    readonly taskContractRevision: number;
    readonly approvedAt: number;
  } | null;
  readonly activity: TaskActivityEvent[];
  sequence: number;
  readonly progress: InternalProgress;
}

const textEncoder = new TextEncoder();
const TASK_STEP_ID_PATTERN = /^[A-Za-z][A-Za-z0-9._-]{0,63}$/;
const TASK_STEP_KINDS = new Set(["research", "implementation", "review"]);
const FINDING_SEVERITIES = new Set(["critical", "high", "medium", "low"]);
const EVIDENCE_SOURCE_TYPE_BY_KIND: Readonly<Record<EvidenceKind, EvidenceSource["type"]>> = {
  workspace_read: "workspace_read",
  api_lookup: "api_lookup",
  lsp_query: "lsp_query",
  change_preview: "change_preview",
  mutation_receipt: "mutation",
  checkpoint: "checkpoint",
  parser_result: "parser",
  lsp_result: "lsp",
  validation_result: "validation",
  review_result: "review",
  reference_read: "reference_read",
  reference_search: "reference_search",
  research: "research",
};
const EVIDENCE_KINDS = new Set<EvidenceKind>(
  Object.keys(EVIDENCE_SOURCE_TYPE_BY_KIND) as EvidenceKind[],
);

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

export function createTaskRuntime(options: TaskRuntimeOptions = {}): TaskRuntime {
  const now = options.now ?? Date.now;
  const records = new Map<TaskId, TaskRecord>();

  function appendActivity(
    record: TaskRecord,
    event: DistributiveOmit<TaskActivityEvent, "sequence" | "taskId" | "atMs">,
  ): void {
    record.sequence += 1;
    record.activity.push({
      ...(event as TaskActivityEvent),
      sequence: record.sequence,
      taskId: record.id,
      atMs: now(),
    });
  }

  function observeProgress(record: TaskRecord, observation: HostObservation): ProgressState {
    const progress = record.progress;
    const key = `${observation.action}:${observation.fingerprint}`;
    const alreadyInWindow = progress.window.some((entry) => entry.key === key);
    const fresh = observation.progress === true || !alreadyInWindow;
    if (fresh) {
      progress.usefulObservations += 1;
      progress.lastProgressAtMs = now();
    }
    progress.window.push({ key, useful: fresh });
    if (progress.window.length > PROGRESS_WINDOW_SIZE) {
      progress.window.shift();
    }
    // Count occurrences of the newest key inside the bounded window, and
    // whether the window contains any genuinely new useful observation.
    // A single repeated action, an alternating loop, and a run with no new
    // state across the whole window all surface deterministically, while a
    // genuinely new observation restores the healthy state.
    const occurrences = progress.window.filter((entry) => entry.key === key).length;
    const usefulInWindow = progress.window.filter((entry) => entry.useful).length;
    progress.repeatedActions = occurrences;
    if (occurrences >= PROGRESS_STALLED_REPETITIONS || usefulInWindow === 0) {
      progress.state = "stalled";
      if (progress.stalledAtMs === null) {
        progress.stalledAtMs = now();
      }
    } else if (occurrences >= PROGRESS_DEGRADED_REPETITIONS) {
      progress.state = "degraded";
      progress.stalledAtMs = null;
    } else {
      progress.state = "healthy";
      progress.stalledAtMs = null;
    }
    return progressSnapshot(progress);
  }

  function progressSnapshot(progress: InternalProgress): ProgressState {
    return {
      state: progress.state,
      usefulObservations: progress.usefulObservations,
      repeatedActions: progress.repeatedActions,
      lastProgressAtMs: progress.lastProgressAtMs,
      stalledAtMs: progress.stalledAtMs,
    };
  }

  function buildInitialState(record: TaskRecord, input: CreateTaskInput): MutableTaskState {
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
      phase: "prepared",
      plan: { ...NO_TASK_PLAN },
      steps,
      acceptance,
      currentFindings: [],
      evidence: [],
      validationStatus: "not_run",
      reviewStatus: "not_run",
      iteration:
        input.iteration === undefined || !Number.isFinite(input.iteration)
          ? 0
          : Math.max(0, Math.floor(input.iteration)),
      progress: progressSnapshot(record.progress),
      startedAtMs: now(),
      completedAtMs: null,
      terminalReason: null,
    };
  }

  function buildRecord(input: CreateTaskInput): TaskRecord {
    if ((input.steps ?? []).length > MAX_TASK_STEPS) {
      throw new Error(`A task accepts at most ${MAX_TASK_STEPS} steps.`);
    }
    const specs = new Map<string, TaskStepSpec>();
    for (const spec of input.steps ?? []) {
      if (!TASK_STEP_ID_PATTERN.test(spec.id)) {
        throw new Error(`Invalid task step id: ${spec.id}`);
      }
      if (specs.has(spec.id)) {
        throw new Error(`Duplicate task step id: ${spec.id}`);
      }
      const description = spec.description.trim();
      if (description.length === 0) {
        throw new Error(`Task step ${spec.id} requires a non-empty description.`);
      }
      if (textEncoder.encode(description).length > MAX_TASK_STEP_DESCRIPTION_BYTES) {
        throw new Error(
          `Task step ${spec.id} description exceeds ${MAX_TASK_STEP_DESCRIPTION_BYTES} UTF-8 bytes.`,
        );
      }
      if (!TASK_STEP_KINDS.has(spec.kind)) {
        throw new Error(`Task step ${spec.id} has invalid kind ${String(spec.kind)}.`);
      }
      if (spec.accepts.length === 0) {
        throw new Error(`Task step ${spec.id} accepts no evidence kinds.`);
      }
      if (spec.accepts.some((kind) => !EVIDENCE_KINDS.has(kind))) {
        throw new Error(`Task step ${spec.id} contains an invalid evidence kind.`);
      }
      if (new Set(spec.accepts).size !== spec.accepts.length) {
        throw new Error(`Task step ${spec.id} contains duplicate evidence kinds.`);
      }
      specs.set(spec.id, deepFreeze({ ...spec, description, accepts: [...spec.accepts] }));
    }
    const contract = deepFreeze(structuredClone(input.contract));
    const snapshot = deepFreeze(structuredClone(input.snapshot));
    const record: TaskRecord = {
      id: contract.id,
      specs,
      contract,
      contractRevisions: [contract],
      snapshot,
      state: undefined as unknown as MutableTaskState,
      plans: [],
      planApproval: null,
      activity: [],
      sequence: 0,
      progress: {
        usefulObservations: 0,
        repeatedActions: 0,
        state: "healthy",
        lastProgressAtMs: null,
        stalledAtMs: null,
        window: [],
      },
    };
    record.state = buildInitialState(record, {
      ...input,
      contract,
      snapshot,
      steps: [...specs.values()],
    });
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

  function findStep(record: TaskRecord, stepId: TaskStepId) {
    return record.state.steps.find((step) => step.id === stepId) ?? null;
  }

  /**
   * Reconcile materialized acceptance state with a new contract revision.
   * An unchanged criterion keeps its evidence-backed state. A new criterion,
   * or one whose meaning/verification kind changed under the same id, starts
   * pending. Removed criteria disappear from the authoritative state.
   */
  function reconcileAcceptanceState(record: TaskRecord, contract: TaskContract): void {
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

  function terminalMutationReason(record: TaskRecord): string | null {
    return isTerminalPhase(record.state.phase)
      ? `The task is terminal (${record.state.phase}); authoritative state can no longer be changed.`
      : null;
  }

  function transitionPhaseLocked(record: TaskRecord, phase: TaskPhase): StepOpResult {
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
      record.state.completedAtMs = now();
    }
    appendActivity(record, { type: "task_phase_changed", phase });
    return { status: "ok" };
  }

  function completeStepLocked(
    record: TaskRecord,
    stepId: TaskStepId,
    refs: readonly EvidenceRef[],
  ): StepOpResult {
    const terminalReason = terminalMutationReason(record);
    if (terminalReason !== null) {
      return { status: "rejected", reason: terminalReason };
    }
    const step = findStep(record, stepId);
    if (step === null) {
      return { status: "rejected", reason: `Unknown step: ${stepId}` };
    }
    const spec = record.specs.get(stepId);
    if (spec === undefined) {
      return { status: "rejected", reason: `Unknown step: ${stepId}` };
    }
    if (step.status !== "active") {
      return {
        status: "rejected",
        reason: `Step ${stepId} is not active (status: ${step.status}).`,
      };
    }
    if (refs.length === 0) {
      return {
        status: "rejected",
        reason: `Step ${stepId} requires at least one evidence reference.`,
      };
    }
    const seen = new Set<string>();
    for (const ref of refs) {
      if (seen.has(ref.evidenceId)) {
        return { status: "rejected", reason: `Duplicate evidence reference: ${ref.evidenceId}` };
      }
      seen.add(ref.evidenceId);
      if (!spec.accepts.includes(ref.kind)) {
        return {
          status: "rejected",
          reason: `Step ${stepId} (${spec.kind}) does not accept evidence kind ${ref.kind}.`,
        };
      }
      const evidence = record.state.evidence.find((entry) => entry.id === ref.evidenceId);
      if (evidence === undefined || evidence.taskId !== record.id) {
        return {
          status: "rejected",
          reason: `Unknown evidence reference for this task: ${ref.evidenceId}`,
        };
      }
      if (evidence.kind !== ref.kind) {
        return {
          status: "rejected",
          reason: `Evidence ${ref.evidenceId} is kind ${evidence.kind}, not ${ref.kind}.`,
        };
      }
    }
    step.status = "completed";
    step.failedReason = null;
    step.blockedReason = null;
    step.evidenceRefs = refs.map((ref) => ({ ...ref }));
    appendActivity(record, {
      type: "step_completed",
      stepId,
      evidenceRefs: refs.map((ref) => ({ evidenceId: ref.evidenceId, kind: ref.kind })),
    });
    observeProgress(record, { action: "step.completed", fingerprint: stepId, progress: true });
    return { status: "ok" };
  }

  function attachEvidenceLocked(
    record: TaskRecord,
    input: { readonly id: string; readonly kind: EvidenceKind; readonly source: EvidenceSource },
  ): EvidenceAttachResult {
    const terminalReason = terminalMutationReason(record);
    if (terminalReason !== null) {
      return { status: "rejected", reason: terminalReason };
    }
    if (input.id.trim().length === 0) {
      return { status: "rejected", reason: "Evidence requires a non-empty id." };
    }
    if (textEncoder.encode(input.id).length > MAX_TASK_EVIDENCE_ID_BYTES) {
      return {
        status: "rejected",
        reason: `Evidence id exceeds the ${MAX_TASK_EVIDENCE_ID_BYTES}-byte bound.`,
      };
    }
    if (!EVIDENCE_KINDS.has(input.kind)) {
      return { status: "rejected", reason: `Unknown evidence kind: ${String(input.kind)}` };
    }
    if (input.source.type !== EVIDENCE_SOURCE_TYPE_BY_KIND[input.kind]) {
      return {
        status: "rejected",
        reason: `Evidence kind ${input.kind} requires source type ${EVIDENCE_SOURCE_TYPE_BY_KIND[input.kind]}, not ${input.source.type}.`,
      };
    }
    if (record.state.evidence.some((entry) => entry.id === input.id)) {
      return { status: "rejected", reason: `Evidence id already attached: ${input.id}` };
    }
    if (record.state.evidence.length >= MAX_TASK_EVIDENCE_RECORDS) {
      return {
        status: "rejected",
        reason: `The task already has the maximum of ${MAX_TASK_EVIDENCE_RECORDS} evidence records.`,
      };
    }
    let serialized: string;
    let source: EvidenceSource;
    try {
      serialized = JSON.stringify(input.source);
      source = structuredClone(input.source);
    } catch {
      return {
        status: "rejected",
        reason: "Evidence source must be finite JSON-serializable data.",
      };
    }
    const bytes = textEncoder.encode(serialized).length;
    if (bytes > MAX_EVIDENCE_SOURCE_BYTES) {
      return {
        status: "rejected",
        reason: `Evidence source exceeds the ${MAX_EVIDENCE_SOURCE_BYTES}-byte bound; attach a reference, not raw output.`,
      };
    }
    const entry: Mutable<EvidenceRecord> = {
      id: input.id,
      kind: input.kind,
      taskId: record.id,
      source,
      attachedAtMs: now(),
    };
    record.state.evidence.push(entry);
    appendActivity(record, { type: "evidence_attached", evidenceId: input.id, kind: input.kind });
    observeProgress(record, {
      action: "evidence.attached",
      fingerprint: input.kind,
      progress: true,
    });
    return { status: "attached", reason: null };
  }

  function verifyCriterionLocked(
    record: TaskRecord,
    criterionId: AcceptanceCriterionId,
    verifiedBy: string | null,
    note: string | undefined,
  ): CriterionResult {
    const terminalReason = terminalMutationReason(record);
    if (terminalReason !== null) {
      return { status: "rejected", reason: terminalReason };
    }
    const criterion = record.state.acceptance.find((entry) => entry.criterionId === criterionId);
    if (criterion === undefined) {
      return { status: "rejected", reason: `Unknown acceptance criterion: ${criterionId}` };
    }
    if (verifiedBy !== null && !record.state.evidence.some((entry) => entry.id === verifiedBy)) {
      return { status: "rejected", reason: `Unknown evidence reference: ${verifiedBy}` };
    }
    criterion.status = "satisfied";
    criterion.verifiedBy = verifiedBy;
    criterion.note = note ?? null;
    appendActivity(record, { type: "criterion_verified", criterionId, verifiedBy });
    observeProgress(record, {
      action: "criterion.verified",
      fingerprint: criterionId,
      progress: true,
    });
    return { status: "verified", reason: null };
  }

  function evaluateCompletionLocked(record: TaskRecord): CompletionEvaluation {
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

  function finalizeCompleteLocked(record: TaskRecord): CompletionResult {
    const terminalReason = terminalMutationReason(record);
    if (terminalReason !== null) {
      return { status: "rejected", reasons: [terminalReason] };
    }
    const evaluation = evaluateCompletionLocked(record);
    if (!evaluation.allowed) {
      return { status: "rejected", reasons: [...evaluation.missing] };
    }
    record.state.phase = "completed";
    record.state.completedAtMs = now();
    record.state.terminalReason = null;
    appendActivity(record, { type: "task_completed" });
    observeProgress(record, { action: "task.completed", fingerprint: record.id, progress: true });
    return { status: "completed" };
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
        const terminalReason = terminalMutationReason(record);
        if (terminalReason !== null) {
          throw new Error(terminalReason);
        }
        const revision = reviseTaskContract(record.contract, changes);
        record.contractRevisions.push(revision);
        record.contract = revision;
        record.state.contractRevision = revision.revision;
        reconcileAcceptanceState(record, revision);
        // A material TaskContract change makes any plan bound to an older
        // revision stale and any plan approval invalid: never silently
        // execute an old plan against a changed contract.
        if (record.plans.length > 0) {
          const current = record.plans[record.plans.length - 1] as TaskPlan;
          record.state.plan.state = "stale";
          record.state.plan.staleReason =
            "The TaskContract revision advanced after this plan was created.";
          if (record.planApproval !== null) {
            record.state.plan.approval = "invalidated";
            appendActivity(record, {
              type: "plan_invalidated",
              planId: record.planApproval.planId,
              revision: record.planApproval.planRevision,
              reason: "The TaskContract revision advanced; the plan approval no longer applies.",
            });
          }
          record.planApproval = null;
          appendActivity(record, {
            type: "plan_invalidated",
            planId: current.id,
            revision: current.revision,
            reason:
              "The TaskContract revision advanced; the plan is stale until revalidated or replanned.",
          });
        }
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

      transitionPhase(phase: TaskPhase): StepOpResult {
        return transitionPhaseLocked(record, phase);
      },
      beginStep(stepId: TaskStepId): StepOpResult {
        const terminalReason = terminalMutationReason(record);
        if (terminalReason !== null) {
          return { status: "rejected", reason: terminalReason };
        }
        const step = findStep(record, stepId);
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
        appendActivity(record, { type: "step_started", stepId });
        return { status: "ok" };
      },
      completeStep(stepId: TaskStepId, evidenceRefs: readonly EvidenceRef[]): StepOpResult {
        return completeStepLocked(record, stepId, evidenceRefs);
      },
      failStep(stepId: TaskStepId, reason: string): StepOpResult {
        const terminalReason = terminalMutationReason(record);
        if (terminalReason !== null) {
          return { status: "rejected", reason: terminalReason };
        }
        const step = findStep(record, stepId);
        if (step === null) {
          return { status: "rejected", reason: `Unknown step: ${stepId}` };
        }
        if (step.status === "completed") {
          return { status: "rejected", reason: `Step ${stepId} is already completed.` };
        }
        step.status = "failed";
        step.failedReason = reason;
        appendActivity(record, { type: "step_failed", stepId, reason });
        return { status: "ok" };
      },
      attachEvidence(input: {
        readonly id: string;
        readonly kind: EvidenceKind;
        readonly source: EvidenceSource;
      }): EvidenceAttachResult {
        return attachEvidenceLocked(record, input);
      },
      verifyCriterion(
        criterionId: AcceptanceCriterionId,
        verifiedBy: string | null,
        note?: string,
      ): CriterionResult {
        return verifyCriterionLocked(record, criterionId, verifiedBy, note);
      },
      markCriterionFailed(criterionId: AcceptanceCriterionId, note?: string): CriterionResult {
        const terminalReason = terminalMutationReason(record);
        if (terminalReason !== null) {
          return { status: "rejected", reason: terminalReason };
        }
        const criterion = record.state.acceptance.find(
          (entry) => entry.criterionId === criterionId,
        );
        if (criterion === undefined) {
          return { status: "rejected", reason: `Unknown acceptance criterion: ${criterionId}` };
        }
        criterion.status = "failed";
        criterion.verifiedBy = null;
        criterion.note = note ?? null;
        return { status: "failed", reason: null };
      },
      setFindings(findings: readonly FindingRef[]): void {
        if (terminalMutationReason(record) !== null) {
          return;
        }
        if (findings.length > MAX_TASK_FINDINGS) {
          throw new Error(`A task accepts at most ${MAX_TASK_FINDINGS} current findings.`);
        }
        const ids = new Set<string>();
        for (const finding of findings) {
          if (finding.findingId.trim().length === 0 || finding.source.trim().length === 0) {
            throw new Error("Task findings require non-empty ids and sources.");
          }
          if (
            textEncoder.encode(finding.findingId).length > MAX_TASK_FINDING_FIELD_BYTES ||
            textEncoder.encode(finding.source).length > MAX_TASK_FINDING_FIELD_BYTES
          ) {
            throw new Error(
              `Task finding fields cannot exceed ${MAX_TASK_FINDING_FIELD_BYTES} UTF-8 bytes.`,
            );
          }
          if (ids.has(finding.findingId)) {
            throw new Error(`Duplicate task finding id: ${finding.findingId}`);
          }
          ids.add(finding.findingId);
          if (!FINDING_SEVERITIES.has(finding.severity)) {
            throw new Error(`Invalid task finding severity: ${String(finding.severity)}`);
          }
        }
        record.state.currentFindings = findings.map((finding) => ({ ...finding }));
      },
      setValidationStatus(status: TaskValidationStatus): void {
        if (terminalMutationReason(record) !== null) {
          return;
        }
        record.state.validationStatus = status;
      },
      setReviewStatus(status: TaskReviewStatus): void {
        if (terminalMutationReason(record) !== null) {
          return;
        }
        record.state.reviewStatus = status;
      },
      setIteration(iteration: number): void {
        if (terminalMutationReason(record) !== null || !Number.isFinite(iteration)) {
          return;
        }
        record.state.iteration = Math.max(0, Math.floor(iteration));
      },

      routePlanning(depth: PlanningDepth, reason: PlanningDecisionReason): void {
        if (terminalMutationReason(record) !== null) {
          return;
        }
        record.state.plan.depth = depth;
        appendActivity(record, { type: "planning_routed", depth, reason });
      },
      rejectPlan(reason: string): void {
        if (terminalMutationReason(record) !== null) {
          return;
        }
        appendActivity(record, { type: "plan_rejected", reason });
      },
      setPlan(plan: TaskPlan): StepOpResult {
        const terminalReason = terminalMutationReason(record);
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
          return {
            status: "rejected",
            reason: `The plan is invalid: ${validated.reasons.join(" ")}`,
          };
        }
        const previous = record.plans[record.plans.length - 1] ?? null;
        if (previous === null && plan.revision !== 1) {
          return { status: "rejected", reason: "The first plan revision must be 1." };
        }
        if (
          previous !== null &&
          previous.id === plan.id &&
          plan.revision !== previous.revision + 1
        ) {
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
        // Advancing a plan revision OR replacing the plan invalidates any
        // prior approval: approval binds to the exact plan id and revision.
        const priorApproved =
          record.planApproval !== null &&
          (record.planApproval.planId !== plan.id ||
            record.planApproval.planRevision !== plan.revision);
        const storedPlan: TaskPlan = deepFreeze({
          id: plan.id,
          revision: plan.revision,
          taskId: plan.taskId,
          taskContractRevision: plan.taskContractRevision,
          depth: plan.depth,
          ...structuredClone(validated.content),
          createdAt: plan.createdAt,
        });
        record.plans.push(storedPlan);
        record.state.plan.planId = storedPlan.id;
        record.state.plan.planRevision = storedPlan.revision;
        record.state.plan.depth = storedPlan.depth;
        record.state.plan.state = "current";
        record.state.plan.staleReason = null;
        if (priorApproved) {
          record.state.plan.approval = "invalidated";
          appendActivity(record, {
            type: "plan_invalidated",
            planId: record.planApproval!.planId,
            revision: record.planApproval!.planRevision,
            reason:
              "The plan identity or revision advanced; the previous approval no longer applies.",
          });
        } else {
          record.state.plan.approval = "none";
        }
        record.planApproval = null;
        appendActivity(record, {
          type: "plan_created",
          planId: storedPlan.id,
          revision: storedPlan.revision,
          depth: storedPlan.depth,
        });
        observeProgress(record, {
          action: "plan.created",
          fingerprint: `${storedPlan.id}:${storedPlan.revision}`,
          progress: true,
        });
        return { status: "ok" };
      },
      approvePlan(planId: TaskPlanId, planRevision: number): StepOpResult {
        const terminalReason = terminalMutationReason(record);
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
        if (record.state.plan.state === "stale") {
          return {
            status: "rejected",
            reason: "The current plan is stale and cannot be approved.",
          };
        }
        record.planApproval = {
          planId: current.id,
          planRevision: current.revision,
          taskContractRevision: current.taskContractRevision,
          approvedAt: now(),
        };
        record.state.plan.approval = "approved";
        appendActivity(record, {
          type: "plan_approved",
          planId: current.id,
          revision: current.revision,
        });
        return { status: "ok" };
      },
      invalidatePlan(reason: string): void {
        if (terminalMutationReason(record) !== null) {
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
        appendActivity(record, {
          type: "plan_invalidated",
          planId: current.id,
          revision: current.revision,
          reason,
        });
      },
      currentPlan(): TaskPlan | null {
        const current = record.plans[record.plans.length - 1] ?? null;
        return current === null ? null : structuredClone(current);
      },
      planRevisions(): readonly TaskPlan[] {
        return record.plans.map((plan) => structuredClone(plan));
      },

      submitDisposition(
        disposition: WorkflowDisposition,
        source: "host" | "model" = "host",
      ): DispositionResult {
        const terminalReason = terminalMutationReason(record);
        if (terminalReason !== null) {
          return {
            accepted: false,
            disposition,
            evaluation: disposition.type === "complete" ? evaluateCompletionLocked(record) : null,
            reason: terminalReason,
          };
        }
        if (disposition.type === "complete") {
          const evaluation = evaluateCompletionLocked(record);
          if (evaluation.allowed) {
            const completed = finalizeCompleteLocked(record);
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
          const transition = transitionPhaseLocked(record, "blocked");
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
        return evaluateCompletionLocked(record);
      },
      completeTask(): CompletionResult {
        return finalizeCompleteLocked(record);
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
        const transition = transitionPhaseLocked(record, "blocked");
        if (transition.status === "ok") {
          record.state.terminalReason = reason;
          appendActivity(record, { type: "task_blocked", reason });
        }
      },

      observe(observation: HostObservation): ProgressState {
        if (terminalMutationReason(record) !== null) {
          return progressSnapshot(record.progress);
        }
        return observeProgress(record, observation);
      },
      progress(): ProgressState {
        return progressSnapshot(record.progress);
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
