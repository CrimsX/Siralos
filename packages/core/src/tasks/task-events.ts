import type { TaskPhase, TaskStepId, WorkflowDisposition } from "./task-model.js";
import type { TaskId } from "./task-model.js";
import type { EvidenceKind } from "./task-model.js";

/**
 * Typed append-only task activity records (Stage 3 milestone 1).
 *
 * This is NOT event sourcing: the authoritative TaskState remains a
 * materialized object, and the activity log exists only for auditability,
 * debugging, future persistence, UI projection, and behavior tests. Events
 * are immutable after append, deterministically sequenced per task,
 * host-timestamped, and never carry secrets, hidden reasoning, or raw
 * provider continuation state.
 */

export type TaskActivityEvent =
  | {
      readonly type: "task_started";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly contractRevision: number;
      readonly atMs: number;
    }
  | {
      readonly type: "task_phase_changed";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly phase: TaskPhase;
      readonly atMs: number;
    }
  | {
      readonly type: "step_started";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly stepId: TaskStepId;
      readonly atMs: number;
    }
  | {
      readonly type: "step_completed";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly stepId: TaskStepId;
      readonly evidenceRefs: readonly {
        readonly evidenceId: string;
        readonly kind: EvidenceKind;
      }[];
      readonly atMs: number;
    }
  | {
      readonly type: "step_failed";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly stepId: TaskStepId;
      readonly reason: string;
      readonly atMs: number;
    }
  | {
      readonly type: "evidence_attached";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly evidenceId: string;
      readonly kind: EvidenceKind;
      readonly atMs: number;
    }
  | {
      readonly type: "criterion_verified";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly criterionId: string;
      readonly verifiedBy: string | null;
      readonly atMs: number;
    }
  | {
      readonly type: "task_contract_revised";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly revision: number;
      readonly atMs: number;
    }
  | {
      readonly type: "task_blocked";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly reason: string;
      readonly atMs: number;
    }
  | {
      readonly type: "task_completed";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly atMs: number;
    }
  | {
      readonly type: "task_cancelled";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly reason: string;
      readonly atMs: number;
    }
  | {
      readonly type: "task_failed";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly reason: string;
      readonly atMs: number;
    }
  | {
      readonly type: "disposition_submitted";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly disposition: WorkflowDisposition;
      readonly source: "host" | "model";
      readonly accepted: boolean;
      readonly note: string | null;
      readonly atMs: number;
    };

export const TASK_ACTIVITY_EVENT_TYPES = [
  "task_started",
  "task_phase_changed",
  "task_contract_revised",
  "step_started",
  "step_completed",
  "step_failed",
  "evidence_attached",
  "criterion_verified",
  "task_blocked",
  "task_completed",
  "task_cancelled",
  "task_failed",
  "disposition_submitted",
] as const;

/** Allowed top-level keys of any activity record (behavior-test allowlist). */
export const TASK_ACTIVITY_EVENT_KEYS = [
  "type",
  "sequence",
  "taskId",
  "contractRevision",
  "revision",
  "phase",
  "stepId",
  "evidenceRefs",
  "reason",
  "evidenceId",
  "kind",
  "criterionId",
  "verifiedBy",
  "disposition",
  "source",
  "accepted",
  "note",
  "atMs",
] as const;
