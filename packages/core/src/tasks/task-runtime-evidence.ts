import type { AcceptanceCriterionId } from "./task-contract.js";
import type {
  EvidenceKind,
  EvidenceRecord,
  EvidenceRef,
  EvidenceSource,
  TaskStepId,
} from "./task-model.js";
import type { CriterionResult, EvidenceAttachResult, StepOpResult } from "./task-runtime-model.js";
import type { Mutable, TaskRecord, TaskRuntimeHooks } from "./task-runtime-record.js";
import { findTaskStep, terminalTaskMutationReason } from "./task-runtime-state.js";
import { MAX_TASK_EVIDENCE_RECORDS, validateEvidencePayload } from "./task-runtime-validation.js";

export function completeTaskStep(
  record: TaskRecord,
  stepId: TaskStepId,
  refs: readonly EvidenceRef[],
  hooks: TaskRuntimeHooks,
): StepOpResult {
  const terminalReason = terminalTaskMutationReason(record);
  if (terminalReason !== null) {
    return { status: "rejected", reason: terminalReason };
  }
  const step = findTaskStep(record, stepId);
  const spec = record.specs.get(stepId);
  if (step === null || spec === undefined) {
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
  hooks.appendActivity(record, {
    type: "step_completed",
    stepId,
    evidenceRefs: refs.map((ref) => ({ evidenceId: ref.evidenceId, kind: ref.kind })),
  });
  hooks.observeProgress(record, { action: "step.completed", fingerprint: stepId, progress: true });
  return { status: "ok" };
}

export function attachTaskEvidence(
  record: TaskRecord,
  input: { readonly id: string; readonly kind: EvidenceKind; readonly source: EvidenceSource },
  hooks: TaskRuntimeHooks,
): EvidenceAttachResult {
  const terminalReason = terminalTaskMutationReason(record);
  if (terminalReason !== null) {
    return { status: "rejected", reason: terminalReason };
  }
  const validated = validateEvidencePayload(input);
  if (!validated.ok) {
    return { status: "rejected", reason: validated.reason };
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
  const entry: Mutable<EvidenceRecord> = {
    id: input.id,
    kind: input.kind,
    taskId: record.id,
    source: validated.source,
    attachedAtMs: hooks.now(),
  };
  record.state.evidence.push(entry);
  hooks.appendActivity(record, {
    type: "evidence_attached",
    evidenceId: input.id,
    kind: input.kind,
  });
  hooks.observeProgress(record, {
    action: "evidence.attached",
    fingerprint: input.kind,
    progress: true,
  });
  return { status: "attached", reason: null };
}

export function verifyTaskCriterion(
  record: TaskRecord,
  criterionId: AcceptanceCriterionId,
  verifiedBy: string | null,
  note: string | undefined,
  hooks: TaskRuntimeHooks,
): CriterionResult {
  const terminalReason = terminalTaskMutationReason(record);
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
  hooks.appendActivity(record, { type: "criterion_verified", criterionId, verifiedBy });
  hooks.observeProgress(record, {
    action: "criterion.verified",
    fingerprint: criterionId,
    progress: true,
  });
  return { status: "verified", reason: null };
}
