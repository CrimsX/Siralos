import { computeArtifactDigest } from "../identity/artifact-digest.js";
import { createOrderingPolicy } from "../determinism/context.js";
import type { RunId } from "./identity.js";
import type { RuntimeArtifactRef } from "./artifacts.js";

/**
 * Failure taxonomy, terminal RunOutcome, process supervision contract,
 * and liveness (Stage 3 — Runtime Readiness & Operational Resilience,
 * ADR 0031).
 *
 * One typed failure taxonomy for Stage 4; distinct terminal dispositions
 * (failure / cancellation / resource limit / successful exit / uncertain
 * cleanup) are never collapsed into a single process_failed. The
 * supervisor state machine is prepared → starting → running →
 * terminating → terminal; the outcome has exactly one terminal execution
 * disposition plus an independent cleanup status.
 */

export type RuntimeFailureKind =
  | "readiness_failed"
  | "spawn_failed"
  | "sandbox_denied"
  | "startup_timeout"
  | "idle_timeout"
  | "hard_timeout"
  | "cancelled"
  | "process_crashed"
  | "kill_failed"
  | "output_limit"
  | "artifact_limit"
  | "environment_unavailable"
  | "cleanup_failed";

export const RUNTIME_FAILURE_KINDS: readonly RuntimeFailureKind[] = [
  "readiness_failed",
  "spawn_failed",
  "sandbox_denied",
  "startup_timeout",
  "idle_timeout",
  "hard_timeout",
  "cancelled",
  "process_crashed",
  "kill_failed",
  "output_limit",
  "artifact_limit",
  "environment_unavailable",
  "cleanup_failed",
] as const;

export type RunTerminalStatus =
  "success" | "failure" | "cancelled" | "resource_limit" | "uncertain";

export interface RunTiming {
  readonly startedAtMs: number | null;
  readonly terminalAtMs: number | null;
  readonly totalMs: number | null;
}

export interface ResourceSummary {
  readonly stdoutBytes: number;
  readonly stderrBytes: number;
  readonly artifactCount: number;
  readonly childProcesses: number;
}

export interface RunOutcome {
  readonly runId: RunId;
  /** Exactly one terminal execution disposition. */
  readonly status: RunTerminalStatus;
  readonly failureKind: RuntimeFailureKind | null;
  readonly exitCode: number | null;
  readonly timing: RunTiming;
  readonly resourceSummary: ResourceSummary;
  readonly artifactRefs: readonly RuntimeArtifactRef[];
  readonly evidenceRefs: readonly string[];
  /** Independent cleanup status — cleanup failure never hides the primary result. */
  readonly cleanupStatus: "cleaned" | "partial" | "failed" | "skipped" | "not_applicable";
}

export function createRunOutcome(input: {
  readonly runId: RunId;
  readonly status: RunTerminalStatus;
  readonly failureKind?: RuntimeFailureKind | null;
  readonly exitCode?: number | null;
  readonly timing: RunTiming;
  readonly resourceSummary: ResourceSummary;
  readonly artifactRefs?: readonly RuntimeArtifactRef[];
  readonly evidenceRefs?: readonly string[];
  readonly cleanupStatus?: RunOutcome["cleanupStatus"];
}): RunOutcome {
  if (input.status === "failure" && input.failureKind === null) {
    throw new Error("A failure outcome requires a typed failure kind.");
  }
  if (input.status === "failure" && input.failureKind === "cleanup_failed") {
    throw new Error(
      "cleanup_failed is an independent cleanup status, never a terminal execution disposition.",
    );
  }
  return {
    runId: input.runId,
    status: input.status,
    failureKind: input.failureKind ?? null,
    exitCode: input.exitCode ?? null,
    timing: { ...input.timing },
    resourceSummary: { ...input.resourceSummary },
    artifactRefs: [...(input.artifactRefs ?? [])],
    evidenceRefs: [...(input.evidenceRefs ?? [])],
    cleanupStatus: input.cleanupStatus ?? "not_applicable",
  };
}

export function renderRunOutcome(outcome: RunOutcome): string {
  const failure = outcome.failureKind === null ? "" : ` (${outcome.failureKind})`;
  return `run ${outcome.runId} -> ${outcome.status}${failure} exit=${
    outcome.exitCode ?? "n/a"
  } cleanup=${outcome.cleanupStatus} artifacts=${outcome.artifactRefs.length}`;
}

// ---------------------------------------------------------------------------
// Process supervision contract
// ---------------------------------------------------------------------------

export type SupervisorState = "prepared" | "starting" | "running" | "terminating" | "terminal";

export type SupervisorObservation =
  | {
      readonly type: "startup_result";
      readonly ok: boolean;
      readonly failureKind: RuntimeFailureKind | null;
    }
  | { readonly type: "output_activity" }
  | {
      readonly type: "liveness";
      readonly kind: "process_alive" | "startup_completed" | "runtime_heartbeat";
    }
  | { readonly type: "idle_timeout" }
  | { readonly type: "hard_timeout" }
  | { readonly type: "resource_limit"; readonly kind: "output_limit" | "artifact_limit" }
  | { readonly type: "cancel_requested" }
  | { readonly type: "child_exit"; readonly exitCode: number | null }
  | { readonly type: "kill_result"; readonly ok: boolean }
  | { readonly type: "child_refused_termination" };

export interface SupervisorStateView {
  readonly state: SupervisorState;
  readonly startedAtMs: number | null;
  readonly terminatedAtMs: number | null;
  readonly terminalDisposition: RunTerminalStatus | null;
  readonly failureKind: RuntimeFailureKind | null;
}

export const INITIAL_SUPERVISOR_STATE: SupervisorStateView = Object.freeze({
  state: "prepared",
  startedAtMs: null,
  terminatedAtMs: null,
  terminalDisposition: null,
  failureKind: null,
});

/**
 * Pure deterministic supervisor transition: state + typed observation
 * (+ controlled clock) → next state. Completion ordering never decides
 * semantics; the transition table is the single source of truth.
 */
export function transitionSupervisor(
  current: SupervisorStateView,
  observation: SupervisorObservation,
  nowMs: number,
): SupervisorStateView {
  if (current.state === "terminal") {
    return current; // terminal is absorbing; repeated observations change nothing
  }
  switch (current.state) {
    case "prepared":
      if (observation.type === "startup_result" && observation.ok) {
        return { ...current, state: "running", startedAtMs: nowMs };
      }
      if (observation.type === "startup_result" && !observation.ok) {
        return {
          ...current,
          state: "terminal",
          terminatedAtMs: nowMs,
          terminalDisposition: "failure",
          failureKind: observation.failureKind ?? "spawn_failed",
        };
      }
      if (observation.type === "cancel_requested") {
        return {
          ...current,
          state: "terminal",
          terminatedAtMs: nowMs,
          terminalDisposition: "cancelled",
          failureKind: "cancelled",
        };
      }
      return current;
    case "starting":
      if (observation.type === "startup_result" && observation.ok) {
        return { ...current, state: "running", startedAtMs: nowMs };
      }
      if (observation.type === "startup_result" && !observation.ok) {
        return {
          ...current,
          state: "terminal",
          terminatedAtMs: nowMs,
          terminalDisposition: "failure",
          failureKind: observation.failureKind ?? "spawn_failed",
        };
      }
      return current;
    case "running":
      if (observation.type === "startup_result" && !observation.ok) {
        // Startup never completed within the window: the host timeout
        // arrives as a failed startup result.
        return {
          ...current,
          state: "terminal",
          terminatedAtMs: nowMs,
          terminalDisposition: "failure",
          failureKind: observation.failureKind ?? "startup_timeout",
        };
      }
      if (observation.type === "child_exit") {
        return {
          ...current,
          state: "terminal",
          terminatedAtMs: nowMs,
          terminalDisposition: observation.exitCode === 0 ? "success" : "failure",
          failureKind: observation.exitCode === 0 ? null : "process_crashed",
        };
      }
      if (observation.type === "cancel_requested") {
        return { ...current, state: "terminating" };
      }
      if (observation.type === "idle_timeout") {
        return {
          ...current,
          state: "terminal",
          terminatedAtMs: nowMs,
          terminalDisposition: "failure",
          failureKind: "idle_timeout",
        };
      }
      if (observation.type === "hard_timeout") {
        return {
          ...current,
          state: "terminal",
          terminatedAtMs: nowMs,
          terminalDisposition: "resource_limit",
          failureKind: "hard_timeout",
        };
      }
      if (observation.type === "resource_limit") {
        return {
          ...current,
          state: "terminal",
          terminatedAtMs: nowMs,
          terminalDisposition: "resource_limit",
          failureKind: observation.kind,
        };
      }
      return current;
    case "terminating":
      if (observation.type === "kill_result" && observation.ok) {
        return {
          ...current,
          state: "terminal",
          terminatedAtMs: nowMs,
          terminalDisposition: "cancelled",
          failureKind: "cancelled",
        };
      }
      if (observation.type === "child_exit") {
        return {
          ...current,
          state: "terminal",
          terminatedAtMs: nowMs,
          terminalDisposition: "cancelled",
          failureKind: "cancelled",
        };
      }
      if (observation.type === "kill_result" && !observation.ok) {
        return {
          ...current,
          state: "terminal",
          terminatedAtMs: nowMs,
          terminalDisposition: "failure",
          failureKind: "kill_failed",
        };
      }
      if (observation.type === "child_refused_termination") {
        return {
          ...current,
          state: "terminal",
          terminatedAtMs: nowMs,
          terminalDisposition: "failure",
          failureKind: "kill_failed",
        };
      }
      return current;
    default:
      return current;
  }
}

/**
 * Terminal reconciliation invariant: a terminal disposition can only be
 * produced by the pure transition above, so "process exited but
 * supervisor remains running" is structurally impossible.
 */
export function isSupervisorTerminal(state: SupervisorStateView): boolean {
  return state.state === "terminal" && state.terminalDisposition !== null;
}

/** Bounded run-scoped activity log (lifecycle observations). */
export type RunActivityEvent =
  | { readonly type: "RunPrepared"; readonly runId: string; readonly atMs: number }
  | {
      readonly type: "ReadinessEvaluated";
      readonly runId: string;
      readonly atMs: number;
      readonly blocked: boolean;
    }
  | { readonly type: "RunStarting"; readonly runId: string; readonly atMs: number }
  | { readonly type: "RunStarted"; readonly runId: string; readonly atMs: number }
  | { readonly type: "RunCancellationRequested"; readonly runId: string; readonly atMs: number }
  | { readonly type: "RunTerminating"; readonly runId: string; readonly atMs: number }
  | {
      readonly type: "RunTerminal";
      readonly runId: string;
      readonly atMs: number;
      readonly disposition: RunTerminalStatus;
    }
  | {
      readonly type: "ArtifactCaptured";
      readonly runId: string;
      readonly atMs: number;
      readonly artifactId: string;
    }
  | {
      readonly type: "ArtifactLimitReached";
      readonly runId: string;
      readonly atMs: number;
      readonly reason: string;
    }
  | { readonly type: "CleanupCompleted"; readonly runId: string; readonly atMs: number }
  | {
      readonly type: "CleanupFailed";
      readonly runId: string;
      readonly atMs: number;
      readonly message: string;
    };

export function createRunActivityLog(): {
  readonly events: readonly RunActivityEvent[];
  /** Records with an explicit timestamp (host passes the controlled clock). */
  readonly record: (event: Omit<RunActivityEvent, "atMs"> & { readonly atMs: number }) => void;
} {
  const events: RunActivityEvent[] = [];
  return {
    events,
    record(event) {
      events.push({ ...event, atMs: event.atMs } as RunActivityEvent);
    },
  };
}

/** Deterministic digest over a run's activity events. */
export function digestRunActivity(events: readonly RunActivityEvent[]): string {
  const ordered = createOrderingPolicy().stableSort(
    events.map((event) => ({ ...event })),
    (event) => `${event.type}:${event.atMs}`,
  );
  return computeArtifactDigest({
    artifactType: "RunActivity",
    schemaVersion: 1,
    payload: { events: ordered },
  }).value;
}
