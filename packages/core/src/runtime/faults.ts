import { createOrderingPolicy } from "../determinism/context.js";
import type { Clock } from "../determinism/context.js";
import type { RuntimeFailureKind } from "./supervision.js";
import type { SupervisorObservation } from "./supervision.js";

/**
 * Deterministic fault-injection harness (Stage 3 — Runtime Readiness &
 * Operational Resilience, ADR 0031).
 *
 * Fake process drivers simulate the full failure taxonomy under the H2
 * controlled clock; no real Godot project or process is needed. The
 * same FaultScript + clock always produces the same observation
 * sequence, so supervision outcomes are reproducible.
 */

export type FaultScript =
  | "normal"
  | "spawn_failure"
  | "sandbox_denied"
  | "startup_hang"
  | "idle_hang"
  | "hard_timeout"
  | "cancel_during_startup"
  | "cancel_while_running"
  | "crash"
  | "child_refuses_termination"
  | "output_flood"
  | "artifact_quota"
  | "cleanup_failure"
  | "restart_incomplete";

export const FAULT_SCRIPTS: readonly FaultScript[] = [
  "normal",
  "spawn_failure",
  "sandbox_denied",
  "startup_hang",
  "idle_hang",
  "hard_timeout",
  "cancel_during_startup",
  "cancel_while_running",
  "crash",
  "child_refuses_termination",
  "output_flood",
  "artifact_quota",
  "cleanup_failure",
  "restart_incomplete",
] as const;

export interface FakeProcessDriver {
  /** Deterministic observation at the given controlled time. */
  observe(nowMs: number, requested: readonly string[]): readonly SupervisorObservation[];
}

export interface FakeProcessOptions {
  /** Simulated output bytes per observation (flood scripts). */
  readonly outputBytesPerTick?: number;
}

/**
 * Deterministic fake process driver: observations are pure functions of
 * the script and the controlled clock.
 */
export function createFakeProcessDriver(
  script: FaultScript,
  options: FakeProcessOptions = {},
): FakeProcessDriver {
  const bytesPerTick = options.outputBytesPerTick ?? 64 * 1024;
  void bytesPerTick;
  return {
    observe(nowMs, requested) {
      const observations: SupervisorObservation[] = [];
      const tick = Math.floor(nowMs / 100);
      switch (script) {
        case "normal":
          if (requested.includes("start")) {
            observations.push({ type: "startup_result", ok: true, failureKind: null });
          }
          if (nowMs > 5_000) {
            observations.push({ type: "child_exit", exitCode: 0 });
          }
          break;
        case "spawn_failure":
          if (requested.includes("start")) {
            observations.push({ type: "startup_result", ok: false, failureKind: "spawn_failed" });
          }
          break;
        case "sandbox_denied":
          if (requested.includes("start")) {
            observations.push({ type: "startup_result", ok: false, failureKind: "sandbox_denied" });
          }
          break;
        case "startup_hang":
          if (requested.includes("start")) {
            observations.push({ type: "startup_result", ok: true, failureKind: null });
          }
          break;
        case "idle_hang":
          if (requested.includes("start")) {
            observations.push({ type: "startup_result", ok: true, failureKind: null });
          }
          if (requested.includes("liveness")) {
            observations.push({ type: "liveness", kind: "process_alive" });
          }
          break;
        case "hard_timeout":
          if (requested.includes("start")) {
            observations.push({ type: "startup_result", ok: true, failureKind: null });
          }
          if (tick % 2 === 0) {
            observations.push({ type: "output_activity" });
          }
          break;
        case "cancel_during_startup":
          if (requested.includes("start")) {
            observations.push({ type: "startup_result", ok: true, failureKind: null });
          }
          break;
        case "cancel_while_running":
          if (requested.includes("start")) {
            observations.push({ type: "startup_result", ok: true, failureKind: null });
          }
          if (nowMs > 2_000) {
            observations.push({ type: "child_exit", exitCode: 0 });
          }
          break;
        case "crash":
          if (requested.includes("start")) {
            observations.push({ type: "startup_result", ok: true, failureKind: null });
          }
          if (nowMs > 3_000) {
            observations.push({ type: "child_exit", exitCode: 1 });
          }
          break;
        case "child_refuses_termination":
          if (requested.includes("start")) {
            observations.push({ type: "startup_result", ok: true, failureKind: null });
          }
          if (requested.includes("terminate")) {
            observations.push({ type: "child_refused_termination" });
          }
          break;
        case "output_flood":
          if (requested.includes("start")) {
            observations.push({ type: "startup_result", ok: true, failureKind: null });
          }
          observations.push({ type: "output_activity" });
          break;
        case "artifact_quota":
          if (requested.includes("start")) {
            observations.push({ type: "startup_result", ok: true, failureKind: null });
          }
          if (nowMs > 1_000) {
            observations.push({ type: "child_exit", exitCode: 0 });
          }
          break;
        case "cleanup_failure":
          if (requested.includes("start")) {
            observations.push({ type: "startup_result", ok: true, failureKind: null });
          }
          if (nowMs > 4_000) {
            observations.push({ type: "child_exit", exitCode: 0 });
          }
          break;
        case "restart_incomplete":
          // No startup observation: the driver represents a run record
          // left non-terminal by a restart.
          break;
      }
      return observations;
    },
  };
}

export type { RuntimeFailureKind, SupervisorObservation };

/** Deterministic failure kind expected for a fault script. */
export function expectedFailureKind(script: FaultScript): RuntimeFailureKind | null {
  switch (script) {
    case "spawn_failure":
      return "spawn_failed";
    case "sandbox_denied":
      return "sandbox_denied";
    case "startup_hang":
      return "startup_timeout";
    case "idle_hang":
      return "idle_timeout";
    case "hard_timeout":
      return "hard_timeout";
    case "cancel_during_startup":
    case "cancel_while_running":
      return "cancelled";
    case "crash":
      return "process_crashed";
    case "child_refuses_termination":
      return "kill_failed";
    case "output_flood":
      return "output_limit";
    case "artifact_quota":
      return "artifact_limit";
    case "cleanup_failure":
      // cleanup_failed is the INDEPENDENT cleanup status, never a
      // terminal execution disposition.
      return null;
    case "normal":
    case "restart_incomplete":
      return null;
  }
}

/** Stable ordered fault-script listing (deterministic). */
export function listFaultScripts(): readonly FaultScript[] {
  return createOrderingPolicy().stableSort(FAULT_SCRIPTS, (script) => script);
}

export type { Clock };
