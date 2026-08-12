import type { TaskPhase, TaskStepId, WorkflowDisposition } from "./task-model.js";
import type { TaskId } from "./task-model.js";
import type { EvidenceKind } from "./task-model.js";
import type { PlanningDepth, TaskPlanId } from "../planning/planning-model.js";
import type { PlanningDecisionReason } from "../planning/planning-policy.js";

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
    }
  | {
      readonly type: "planning_routed";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly depth: PlanningDepth;
      readonly reason: PlanningDecisionReason;
      readonly atMs: number;
    }
  | {
      readonly type: "plan_created";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly planId: TaskPlanId;
      readonly revision: number;
      readonly depth: "light" | "full";
      readonly atMs: number;
    }
  | {
      readonly type: "plan_rejected";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly reason: string;
      readonly atMs: number;
    }
  | {
      readonly type: "plan_approved";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly planId: TaskPlanId;
      readonly revision: number;
      /** Exact content digest of the approved plan revision. */
      readonly digest: string;
      readonly atMs: number;
    }
  | {
      readonly type: "plan_invalidated";
      readonly sequence: number;
      readonly taskId: TaskId;
      readonly planId: TaskPlanId;
      readonly revision: number;
      readonly reason: string;
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
  "planning_routed",
  "plan_created",
  "plan_rejected",
  "plan_approved",
  "plan_invalidated",
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
  "planId",
  "depth",
  "atMs",
] as const;
