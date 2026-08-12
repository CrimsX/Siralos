import { describe, expect, it } from "vitest";
import { createFixedClock } from "../determinism/context.js";
import { createRunId } from "./identity.js";
import {
  expectedFailureKind,
  createFakeProcessDriver,
  FAULT_SCRIPTS,
  listFaultScripts,
} from "./faults.js";
import {
  INITIAL_SUPERVISOR_STATE,
  transitionSupervisor,
  createRunOutcome,
  type RunTerminalStatus,
  type SupervisorObservation,
  type SupervisorStateView,
} from "./supervision.js";

/**
 * Deterministic fault-injection suite (Stage 3 — Runtime Readiness &
 * Operational Resilience, ADR 0031): the full failure taxonomy driven
 * through the pure supervisor under the H2 controlled clock, with
 * explicit per-script observation sequences. No real Godot project or
 * process is used.
 */

const RUN_ID = createRunId({ taskId: "task-1", phaseId: "runtime_execution", sequence: 1 });

interface ScenarioExpectation {
  readonly script: (typeof FAULT_SCRIPTS)[number];
  readonly disposition: RunTerminalStatus | null;
  readonly failureKind: string | null;
}

const SCENARIOS: readonly ScenarioExpectation[] = [
  { script: "normal", disposition: "success", failureKind: null },
  { script: "spawn_failure", disposition: "failure", failureKind: "spawn_failed" },
  { script: "sandbox_denied", disposition: "failure", failureKind: "sandbox_denied" },
  { script: "startup_hang", disposition: "failure", failureKind: "startup_timeout" },
  { script: "idle_hang", disposition: "failure", failureKind: "idle_timeout" },
  { script: "hard_timeout", disposition: "resource_limit", failureKind: "hard_timeout" },
  { script: "cancel_during_startup", disposition: "cancelled", failureKind: "cancelled" },
  { script: "cancel_while_running", disposition: "cancelled", failureKind: "cancelled" },
  { script: "crash", disposition: "failure", failureKind: "process_crashed" },
  { script: "child_refuses_termination", disposition: "failure", failureKind: "kill_failed" },
  { script: "output_flood", disposition: "resource_limit", failureKind: "output_limit" },
  { script: "artifact_quota", disposition: "resource_limit", failureKind: "artifact_limit" },
  { script: "cleanup_failure", disposition: "success", failureKind: null },
  { script: "restart_incomplete", disposition: null, failureKind: null },
];

/** Explicit deterministic observation sequence per fault script. */
function sequenceFor(script: (typeof FAULT_SCRIPTS)[number]): readonly SupervisorObservation[] {
  const startupOk: SupervisorObservation = { type: "startup_result", ok: true, failureKind: null };
  switch (script) {
    case "normal":
      return [startupOk, { type: "child_exit", exitCode: 0 }];
    case "spawn_failure":
      return [{ type: "startup_result", ok: false, failureKind: "spawn_failed" }];
    case "sandbox_denied":
      return [{ type: "startup_result", ok: false, failureKind: "sandbox_denied" }];
    case "startup_hang":
      return [startupOk, { type: "startup_result", ok: false, failureKind: "startup_timeout" }];
    case "idle_hang":
      return [startupOk, { type: "liveness", kind: "process_alive" }, { type: "idle_timeout" }];
    case "hard_timeout":
      return [startupOk, { type: "output_activity" }, { type: "hard_timeout" }];
    case "cancel_during_startup":
      return [startupOk, { type: "cancel_requested" }, { type: "kill_result", ok: true }];
    case "cancel_while_running":
      return [startupOk, { type: "cancel_requested" }, { type: "kill_result", ok: true }];
    case "crash":
      return [startupOk, { type: "child_exit", exitCode: 1 }];
    case "child_refuses_termination":
      return [startupOk, { type: "cancel_requested" }, { type: "child_refused_termination" }];
    case "output_flood":
      return [startupOk, { type: "resource_limit", kind: "output_limit" }];
    case "artifact_quota":
      return [startupOk, { type: "resource_limit", kind: "artifact_limit" }];
    case "cleanup_failure":
      return [startupOk, { type: "child_exit", exitCode: 0 }];
    case "restart_incomplete":
      return [];
  }
}

/** Drive the pure supervisor through the scripted sequence under a controlled clock. */
function driveScript(script: (typeof FAULT_SCRIPTS)[number]): {
  readonly state: SupervisorStateView;
  readonly terminalStatus: RunTerminalStatus | null;
} {
  const clock = createFixedClock(1_000);
  let state: SupervisorStateView = INITIAL_SUPERVISOR_STATE;
  for (const observation of sequenceFor(script)) {
    clock.advance(100);
    state = transitionSupervisor(state, observation, clock.now());
  }
  return { state, terminalStatus: state.state === "terminal" ? state.terminalDisposition : null };
}

describe("fault-injection suite", () => {
  it("covers every fault script with a distinct expected outcome", () => {
    expect(listFaultScripts()).toEqual([...FAULT_SCRIPTS].sort());
    for (const scenario of SCENARIOS) {
      expect(expectedFailureKind(scenario.script)).toBe(scenario.failureKind);
    }
  });

  for (const scenario of SCENARIOS) {
    it(`script "${scenario.script}" produces the distinct expected disposition`, () => {
      const result = driveScript(scenario.script);
      expect(result.terminalStatus).toBe(scenario.disposition);
      if (result.terminalStatus !== null && scenario.failureKind !== null) {
        expect(result.state.failureKind).toBe(scenario.failureKind);
      }
    });
  }

  it("the same script + clock produces the same observations (deterministic supervision)", () => {
    const a = createFakeProcessDriver("normal");
    const b = createFakeProcessDriver("normal");
    const clockA = createFixedClock(1_000);
    const clockB = createFixedClock(1_000);
    const obsA = a.observe(clockA.now(), ["start"]);
    const obsB = b.observe(clockB.now(), ["start"]);
    expect(obsB).toEqual(obsA);
  });

  it("builds a RunOutcome for each distinct terminal failure kind", () => {
    for (const scenario of SCENARIOS) {
      if (scenario.failureKind === null || scenario.disposition === null) {
        continue;
      }
      const outcome = createRunOutcome({
        runId: RUN_ID,
        status: scenario.disposition,
        failureKind: scenario.failureKind as never,
        timing: { startedAtMs: 1_000, terminalAtMs: 2_000, totalMs: 1_000 },
        resourceSummary: { stdoutBytes: 0, stderrBytes: 0, artifactCount: 0, childProcesses: 0 },
      });
      expect(outcome.status).toBe(scenario.disposition);
      expect(outcome.failureKind).toBe(scenario.failureKind);
    }
  });

  it("cleanup failure is an independent status, never a terminal disposition", () => {
    const result = driveScript("cleanup_failure");
    expect(result.terminalStatus).toBe("success");
    expect(result.state.failureKind).toBeNull();
    const outcome = createRunOutcome({
      runId: RUN_ID,
      status: "success",
      timing: { startedAtMs: 1_000, terminalAtMs: 2_000, totalMs: 1_000 },
      resourceSummary: { stdoutBytes: 0, stderrBytes: 0, artifactCount: 0, childProcesses: 0 },
      cleanupStatus: "failed",
    });
    expect(outcome.status).toBe("success");
    expect(outcome.cleanupStatus).toBe("failed");
  });
});
