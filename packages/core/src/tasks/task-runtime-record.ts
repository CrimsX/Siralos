import type { TaskPlan, TaskPlanId, TaskPlanState } from "../planning/planning-model.js";
import type { TaskContract } from "./task-contract.js";
import type { TaskActivityEvent } from "./task-events.js";
import type {
  ProgressState,
  ProgressStateValue,
  TaskId,
  TaskState,
  TaskStepSpec,
} from "./task-model.js";
import type { HostObservation } from "./task-runtime-model.js";
import type { TaskRuntimeSnapshot } from "./task-snapshot.js";

export type Mutable<T> = {
  -readonly [K in keyof T]: T[K] extends readonly (infer U)[] ? Mutable<U>[] : T[K];
};

export type MutableTaskState = Omit<Mutable<TaskState>, "plan"> & {
  plan: MutablePlanState;
};

export type MutablePlanState = {
  -readonly [K in keyof TaskPlanState]: TaskPlanState[K];
};

type DistributiveOmit<T, K extends keyof never> = T extends unknown ? Omit<T, K> : never;

export type TaskActivityDraft = DistributiveOmit<TaskActivityEvent, "sequence" | "taskId" | "atMs">;

export interface InternalTaskProgress {
  usefulObservations: number;
  repeatedActions: number;
  state: ProgressStateValue;
  lastProgressAtMs: number | null;
  stalledAtMs: number | null;
  window: Array<{ readonly key: string; readonly useful: boolean }>;
}

export interface TaskRecord {
  readonly id: TaskId;
  readonly specs: ReadonlyMap<string, TaskStepSpec>;
  contract: TaskContract;
  readonly contractRevisions: TaskContract[];
  readonly snapshot: TaskRuntimeSnapshot;
  state: MutableTaskState;
  readonly plans: TaskPlan[];
  planApproval: {
    readonly planId: TaskPlanId;
    readonly planRevision: number;
    readonly taskContractRevision: number;
    readonly approvedAt: number;
  } | null;
  readonly activity: TaskActivityEvent[];
  sequence: number;
  readonly progress: InternalTaskProgress;
}

export interface TaskRuntimeHooks {
  readonly now: () => number;
  readonly appendActivity: (record: TaskRecord, event: TaskActivityDraft) => void;
  readonly observeProgress: (record: TaskRecord, observation: HostObservation) => ProgressState;
}
