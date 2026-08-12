import { computeArtifactDigest } from "../identity/artifact-digest.js";

/**
 * Causal runtime identity (Stage 3 — Runtime Readiness & Operational
 * Resilience, ADR 0031).
 *
 * Hierarchical causal correlation (NOT distributed tracing):
 *
 *   TaskId → PhaseId → RunId → OperationId/ProcessId → Evidence/Artifact
 *
 * Every future runtime artifact or process observation is traceable to
 * the run that produced it. IDs are deterministic from host inputs, so
 * equivalent inputs produce equivalent identities.
 */

export type PhaseId = string;
export type RunId = string & { readonly __runId: unique symbol };
export type OperationId = string & { readonly __operationId: unique symbol };

export interface RunIdentityInput {
  readonly taskId: string;
  readonly phaseId: PhaseId;
  /** Run sequence within the phase (1-based). */
  readonly sequence: number;
  /** Run kind for the id domain (e.g. "runtime"). */
  readonly kind?: string;
}

/** Deterministic run id: `run_<kind>_<24hex of task:phase:seq>`. */
export function createRunId(input: RunIdentityInput): RunId {
  if (input.taskId.length === 0 || input.phaseId.length === 0) {
    throw new Error("A run identity requires a task id and a phase id.");
  }
  if (!Number.isSafeInteger(input.sequence) || input.sequence < 1) {
    throw new Error("A run sequence must be a positive safe integer.");
  }
  const kind = input.kind ?? "runtime";
  const digest = computeArtifactDigest({
    artifactType: "RunId",
    schemaVersion: 1,
    payload: { taskId: input.taskId, phaseId: input.phaseId, sequence: input.sequence, kind },
  }).value;
  return `run_${kind}_${digest.slice(0, 24)}` as RunId;
}

/** Deterministic operation id within a run. */
export function createOperationId(runId: RunId, operation: string): OperationId {
  const digest = computeArtifactDigest({
    artifactType: "OperationId",
    schemaVersion: 1,
    payload: { runId, operation },
  }).value;
  return `op_${digest.slice(0, 24)}` as OperationId;
}

/** Causal trace reference preserved on evidence and artifacts. */
export interface RunTraceRef {
  readonly taskId: string;
  readonly phaseId: PhaseId;
  readonly runId: RunId;
  readonly operationId: OperationId | null;
  /** Producer identity (e.g. "process-supervisor", "artifact-capture"). */
  readonly producer: string;
}

export function createRunTraceRef(input: {
  readonly taskId: string;
  readonly phaseId: PhaseId;
  readonly runId: RunId;
  readonly operationId?: OperationId | null;
  readonly producer: string;
}): RunTraceRef {
  return {
    taskId: input.taskId,
    phaseId: input.phaseId,
    runId: input.runId,
    operationId: input.operationId ?? null,
    producer: input.producer,
  };
}

/** Bounded human-readable trace line (projection, never authority). */
export function formatRunTraceRef(trace: RunTraceRef): string {
  return `task=${trace.taskId} phase=${trace.phaseId} run=${trace.runId}${
    trace.operationId === null ? "" : ` op=${trace.operationId}`
  } producer=${trace.producer}`;
}
