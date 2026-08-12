import { computeArtifactDigest } from "../identity/artifact-digest.js";
import type { RunId } from "./identity.js";

/**
 * RuntimeBudget, cancellation, and restart reconciliation (Stage 3 —
 * Runtime Readiness & Operational Resilience, ADR 0031).
 *
 * Budgets list only what the existing sandbox/backend can enforce or
 * reliably observe; unsupported limits (memory/CPU) are exposed as
 * capability state, never pretended enforced. Cancellation is
 * deterministic and idempotent — repeated requests never create
 * competing cleanup flows. Restart reconciliation classifies incomplete
 * runs conservatively; Siralos restart never implies the external
 * process is gone.
 */

export interface RuntimeBudget {
  readonly startupTimeoutMs: number;
  readonly idleTimeoutMs: number;
  readonly hardLifetimeMs: number;
  readonly stdoutBytes: number;
  readonly stderrBytes: number;
  readonly artifactBytes: number;
  readonly artifactCount: number;
  readonly childProcessCount: number;
  /** Optional resource limits ONLY when the backend can enforce/observe them. */
  readonly memoryMb: number | null;
  readonly cpuPercent: number | null;
  readonly digest: string;
}

export interface RuntimeBudgetCapabilities {
  readonly memoryEnforced: boolean;
  readonly cpuEnforced: boolean;
}

export function createRuntimeBudget(input: {
  readonly startupTimeoutMs?: number;
  readonly idleTimeoutMs?: number;
  readonly hardLifetimeMs?: number;
  readonly stdoutBytes?: number;
  readonly stderrBytes?: number;
  readonly artifactBytes?: number;
  readonly artifactCount?: number;
  readonly childProcessCount?: number;
  readonly memoryMb?: number | null;
  readonly cpuPercent?: number | null;
}): RuntimeBudget {
  const budget: RuntimeBudget = {
    startupTimeoutMs: input.startupTimeoutMs ?? 15_000,
    idleTimeoutMs: input.idleTimeoutMs ?? 60_000,
    hardLifetimeMs: input.hardLifetimeMs ?? 300_000,
    stdoutBytes: input.stdoutBytes ?? 4 * 1024 * 1024,
    stderrBytes: input.stderrBytes ?? 4 * 1024 * 1024,
    artifactBytes: input.artifactBytes ?? 64 * 1024 * 1024,
    artifactCount: input.artifactCount ?? 128,
    childProcessCount: input.childProcessCount ?? 4,
    memoryMb: input.memoryMb ?? null,
    cpuPercent: input.cpuPercent ?? null,
    digest: "",
  };
  return {
    ...budget,
    digest: computeArtifactDigest({
      artifactType: "RuntimeBudget",
      schemaVersion: 1,
      payload: { ...budget },
    }).value,
  };
}

export const DEFAULT_RUNTIME_BUDGET: RuntimeBudget = createRuntimeBudget({});

export function renderRuntimeBudget(budget: RuntimeBudget): string {
  return `startup=${budget.startupTimeoutMs}ms idle=${budget.idleTimeoutMs}ms hard=${budget.hardLifetimeMs}ms stdout=${budget.stdoutBytes}B stderr=${budget.stderrBytes}B artifacts=${budget.artifactCount}x${budget.artifactBytes}B children=${budget.childProcessCount}${
    budget.memoryMb === null ? "" : ` mem=${budget.memoryMb}MB`
  }${budget.cpuPercent === null ? "" : ` cpu=${budget.cpuPercent}%`}`;
}

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

export type CancellationPhase = "none" | "requested" | "finalized";

export interface CancellationState {
  readonly phase: CancellationPhase;
  /** Single cleanup flow identity: repeated requests never duplicate it. */
  readonly cleanupFlowId: string | null;
  readonly requestedAtMs: number | null;
}

export function requestCancellation(
  state: CancellationState,
  runId: RunId,
  nowMs: number,
): { readonly state: CancellationState; readonly idempotent: boolean } {
  if (state.phase !== "none") {
    // Repeated cancellation requests return the SAME flow: no competing
    // cleanup flows, deterministic and idempotent.
    return { state, idempotent: true };
  }
  const cleanupFlowId = `cleanup_${runId}`;
  return {
    state: { phase: "requested", cleanupFlowId, requestedAtMs: nowMs },
    idempotent: false,
  };
}

export function finalizeCancellation(state: CancellationState): CancellationState {
  return { ...state, phase: "finalized" };
}

/** Deterministic cancellation semantics checklist (projection). */
export function renderCancellationState(state: CancellationState): string {
  return `cancellation=${state.phase}${state.cleanupFlowId === null ? "" : ` flow=${state.cleanupFlowId}`}`;
}

// ---------------------------------------------------------------------------
// Restart reconciliation
// ---------------------------------------------------------------------------

export type IncompleteRunClassification = "interrupted" | "unknown" | "cleanup_required";

export interface IncompleteRunRecord {
  readonly runId: RunId;
  readonly lastKnownState: string;
  readonly lastObservedAtMs: number;
}

/**
 * Conservative classification of a run left non-terminal by a Siralos
 * restart. We never assume the external process is gone solely because
 * Siralos restarted; without a reliable process observation the record
 * is `unknown`, with cleanup required when run-owned state may exist.
 */
export function classifyIncompleteRun(
  record: IncompleteRunRecord,
  runStateMayExist: boolean,
): { readonly classification: IncompleteRunClassification; readonly reason: string } {
  if (record.lastKnownState === "prepared") {
    return {
      classification: "interrupted",
      reason: "The run never started; it was interrupted during preparation.",
    };
  }
  if (
    record.lastKnownState === "starting" ||
    record.lastKnownState === "running" ||
    record.lastKnownState === "terminating"
  ) {
    return {
      classification: runStateMayExist ? "cleanup_required" : "unknown",
      reason: runStateMayExist
        ? "Run-owned state may exist; conservative cleanup is required before any new run."
        : "The external process state is unknown after restart; the run is classified unknown, never success.",
    };
  }
  return {
    classification: "unknown",
    reason:
      "The run record is not in a recognized active state; classified conservatively as unknown.",
  };
}

export function renderIncompleteRunClassification(
  classification: IncompleteRunClassification,
): string {
  return `incomplete run classified: ${classification}`;
}
