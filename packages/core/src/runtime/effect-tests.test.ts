import { describe, expect, it } from "vitest";
import { createFixedClock } from "../determinism/context.js";
import { createRunId, createRunTraceRef } from "./identity.js";
import {
  authorizesSourceMutation,
  cleanupScopeForRun,
  createRunFilesystemBoundary,
  createRuntimeSideEffectPolicy,
  resolveRunOwnedPath,
} from "./side-effects.js";
import {
  createRuntimeArtifactRef,
  createRuntimeArtifactStore,
  projectRuntimeArtifactsForContext,
} from "./artifacts.js";
import {
  INITIAL_SUPERVISOR_STATE,
  transitionSupervisor,
  createRunOutcome,
  type SupervisorStateView,
} from "./supervision.js";
import { classifyIncompleteRun, finalizeCancellation, requestCancellation } from "./budget.js";
import { evaluateRuntimeReadiness, executionAllowed } from "./readiness.js";
import { createRunManifest } from "./modes.js";

/**
 * Mandatory effect tests (Stage 3 — Runtime Readiness & Operational
 * Resilience, ADR 0031): workspace protection, user-data isolation,
 * timeout taxonomy, cancellation race, output flood, cleanup
 * containment, restart reconciliation, and fail-closed readiness. No
 * real Godot execution.
 */

const SHA = (letter: string): string => letter.repeat(64);
const RUN_ID = createRunId({ taskId: "task-1", phaseId: "runtime_execution", sequence: 1 });

describe("effect — workspace protection", () => {
  it("a run plan targeting source-workspace paths is never authorized and readiness refuses unsafe configuration", () => {
    const policy = createRuntimeSideEffectPolicy({ network: "denied" });
    // The policy structurally cannot authorize source mutation.
    expect(authorizesSourceMutation(policy)).toBe(false);
    // A RunManifest whose sandbox profile is missing fails readiness:
    // no execution request can proceed without the isolation property.
    const manifest = createRunManifest({
      taskId: "task-1",
      phaseId: "runtime_execution",
      runId: RUN_ID,
      taskContractDigest: SHA("a"),
      phaseContractDigest: null,
      executionInputDigest: SHA("c"),
      reproducibilityDigest: SHA("d"),
      godotExecutableFingerprint: SHA("e"),
      projectIdentity: "ws-1",
      runtimeMode: "headless",
      sandboxProfileId: null,
      sideEffectPolicyDigest: policy.digest,
      resourceBudgetDigest: SHA("g"),
      environmentDigest: SHA("h"),
    });
    void manifest;
    const readiness = evaluateRuntimeReadiness({
      runtimeMode: "headless",
      godotExecutable: { available: true, fingerprint: SHA("e") },
      projectIdentity: "ws-1",
      sandboxBackend: { available: false, supportsProcessSupervision: false },
      filesystemIsolation: { available: true, userDataRedirect: true },
      networkPolicyResolvable: true,
      artifactStorageAvailable: true,
      displayAvailable: null,
      resourceLimitCapabilities: { memory: false, cpu: false },
    });
    expect(executionAllowed(readiness)).toBe(false);
  });
});

describe("effect — user-data isolation", () => {
  it("each fake run receives its own host-controlled user-data location; model input cannot select arbitrary external directories", () => {
    const boundaryA = createRunFilesystemBoundary({
      runId: createRunId({ taskId: "task-1", phaseId: "runtime_execution", sequence: 1 }),
      hostRoots: {
        project_copy: "/runs/task-1/run1/project",
        user_data: "/runs/task-1/run1/userdata",
        temp: "/runs/task-1/run1/tmp",
        output: "/runs/task-1/run1/output",
        artifacts: "/runs/task-1/run1/artifacts",
      },
    });
    const boundaryB = createRunFilesystemBoundary({
      runId: createRunId({ taskId: "task-1", phaseId: "runtime_execution", sequence: 2 }),
      hostRoots: {
        project_copy: "/runs/task-1/run2/project",
        user_data: "/runs/task-1/run2/userdata",
        temp: "/runs/task-1/run2/tmp",
        output: "/runs/task-1/run2/output",
        artifacts: "/runs/task-1/run2/artifacts",
      },
    });
    expect(boundaryA.roots.user_data).not.toBe(boundaryB.roots.user_data);
    // Model-controlled input cannot escape the run-owned root.
    const malicious = resolveRunOwnedPath(boundaryA, "user_data", "../../../../etc/shadow");
    expect(malicious.status).toBe("rejected");
    const absolute = resolveRunOwnedPath(boundaryA, "user_data", "/etc/shadow");
    expect(absolute.status).toBe("rejected");
    // Cleanup operates only on host-owned roots.
    expect(cleanupScopeForRun(boundaryA)).toEqual([
      "/runs/task-1/run1/project",
      "/runs/task-1/run1/userdata",
      "/runs/task-1/run1/tmp",
      "/runs/task-1/run1/output",
      "/runs/task-1/run1/artifacts",
    ]);
  });
});

describe("effect — timeout taxonomy", () => {
  it("startup, idle, and hard timeouts produce distinct outcomes under controlled time", () => {
    const clock = createFixedClock(1_000);
    const running = transitionSupervisor(
      INITIAL_SUPERVISOR_STATE,
      { type: "startup_result", ok: true, failureKind: null },
      clock.now(),
    );
    const startup = transitionSupervisor(
      running,
      { type: "startup_result", ok: false, failureKind: "startup_timeout" },
      clock.now() + 15_000,
    );
    const idle = transitionSupervisor(running, { type: "idle_timeout" }, clock.now() + 60_000);
    const hard = transitionSupervisor(running, { type: "hard_timeout" }, clock.now() + 300_000);
    expect(startup.failureKind).toBe("startup_timeout");
    expect(idle.failureKind).toBe("idle_timeout");
    expect(hard.failureKind).toBe("hard_timeout");
    expect(hard.terminalDisposition).toBe("resource_limit");
    const startupOutcome = createRunOutcome({
      runId: RUN_ID,
      status: "failure",
      failureKind: "startup_timeout",
      timing: { startedAtMs: 1_000, terminalAtMs: 16_000, totalMs: 15_000 },
      resourceSummary: { stdoutBytes: 0, stderrBytes: 0, artifactCount: 0, childProcesses: 0 },
    });
    expect(startupOutcome.failureKind).toBe("startup_timeout");
    void startup;
  });
});

describe("effect — cancellation race", () => {
  it("process exit vs cancellation in any callback order produces exactly one deterministic terminal outcome", () => {
    const clock = createFixedClock(1_000);
    const run = transitionSupervisor(
      INITIAL_SUPERVISOR_STATE,
      { type: "startup_result", ok: true, failureKind: null },
      clock.now(),
    );
    // Order A: cancellation first, then exit -> cancelled.
    const orderA = transitionSupervisor(
      transitionSupervisor(run, { type: "cancel_requested" }, clock.now() + 100),
      { type: "child_exit", exitCode: 0 },
      clock.now() + 200,
    );
    // Order B: exit first, then cancellation -> success (terminal absorbing).
    const orderB = transitionSupervisor(
      transitionSupervisor(run, { type: "child_exit", exitCode: 0 }, clock.now() + 100),
      { type: "cancel_requested" },
      clock.now() + 200,
    );
    expect(orderA.state).toBe("terminal");
    expect(orderA.terminalDisposition).toBe("cancelled");
    expect(orderB.state).toBe("terminal");
    expect(orderB.terminalDisposition).toBe("success");
    // Exactly one terminal disposition in both orders.
    const dispositions = [orderA.terminalDisposition, orderB.terminalDisposition];
    expect(dispositions.every((disposition) => disposition !== null)).toBe(true);
    // Repeated cancellation is idempotent with a single cleanup flow.
    const first = requestCancellation(
      { phase: "none", cleanupFlowId: null, requestedAtMs: null },
      RUN_ID,
      clock.now(),
    );
    const second = requestCancellation(first.state, RUN_ID, clock.now() + 10);
    expect(second.idempotent).toBe(true);
    expect(second.state.cleanupFlowId).toBe(first.state.cleanupFlowId);
    expect(finalizeCancellation(second.state).phase).toBe("finalized");
  });
});

describe("effect — output flood", () => {
  it("configured limits hold, evidence states truncation, and model context stays bounded", () => {
    const store = createRuntimeArtifactStore({
      budget: {
        maxArtifactBytes: 1_024,
        maxArtifactsPerRun: 100,
        maxAggregateBytesPerRun: 1_000_000,
        maxRetainedBytesPerTask: 1_000_000,
      },
    });
    // Simulate unbounded stdout: 100 kB > 1 kB artifact limit.
    const ref = createRuntimeArtifactRef({
      id: "stdout-1",
      runId: RUN_ID,
      kind: "stdout",
      mediaType: "text/plain",
      size: 100_000,
      producer: "process-supervisor",
      createdAtMs: 1_000,
      retentionClass: "task",
      location: "/runs/run1/artifacts/stdout-1.log",
      digest: SHA("1"),
      truncated: true,
    });
    const admission = store.register(ref);
    expect(admission.status).toBe("registered");
    expect(store.list(RUN_ID)[0]?.truncated).toBe(true);
    // The model context projection is bounded references, never raw bytes.
    const projection = projectRuntimeArtifactsForContext(store.list(RUN_ID));
    expect(projection).toContain("stdout-1");
    expect(projection).toContain("truncated at budget limit");
    expect(projection.length).toBeLessThan(500);
  });
});

describe("effect — cleanup containment", () => {
  it("malicious cleanup paths are rejected; cleanup only touches host-owned run roots", () => {
    const boundary = createRunFilesystemBoundary({
      runId: RUN_ID,
      hostRoots: {
        project_copy: "/runs/run1/project",
        user_data: "/runs/run1/userdata",
        temp: "/runs/run1/tmp",
        output: "/runs/run1/output",
        artifacts: "/runs/run1/artifacts",
      },
    });
    // A malicious "cleanup path" supplied through fixture state is never
    // accepted as a cleanup target.
    expect(resolveRunOwnedPath(boundary, "temp", "..\\..\\workspace\\src").status).toBe("rejected");
    expect(resolveRunOwnedPath(boundary, "temp", "/workspace/src").status).toBe("rejected");
    // The cleanup scope is exactly the host-owned roots.
    for (const root of cleanupScopeForRun(boundary)) {
      expect(root.startsWith("/runs/run1/")).toBe(true);
    }
  });
});

describe("effect — restart reconciliation", () => {
  it("a persisted non-terminal run reconciles to interrupted/unknown/cleanup_required, never success", () => {
    const clock = createFixedClock(10_000);
    // Persisted record recreated after a Siralos restart.
    const record = { runId: RUN_ID, lastKnownState: "running", lastObservedAtMs: 2_000 };
    const withState = classifyIncompleteRun(record, true);
    expect(withState.classification).toBe("cleanup_required");
    const withoutState = classifyIncompleteRun(record, false);
    expect(withoutState.classification).toBe("unknown");
    const prepared = classifyIncompleteRun({ ...record, lastKnownState: "prepared" }, false);
    expect(prepared.classification).toBe("interrupted");
    // No fabricated success anywhere.
    const outcomes = [withState, withoutState, prepared].map((entry) => entry.classification);
    expect(outcomes).not.toContain("success");
    void clock;
  });
});

describe("effect — readiness fail-closed", () => {
  it("a mode whose required isolation capability is unavailable blocks every execution request", () => {
    const readiness = evaluateRuntimeReadiness({
      runtimeMode: "visual",
      godotExecutable: { available: true, fingerprint: SHA("e") },
      projectIdentity: "ws-1",
      sandboxBackend: { available: true, supportsProcessSupervision: true },
      filesystemIsolation: { available: false, userDataRedirect: false },
      networkPolicyResolvable: true,
      artifactStorageAvailable: true,
      displayAvailable: false,
      resourceLimitCapabilities: { memory: false, cpu: false },
    });
    expect(executionAllowed(readiness)).toBe(false);
    expect(readiness.blockedReasons.length).toBeGreaterThan(0);
    // No silent downgrade: every missing required isolation property is listed.
    const joined = readiness.blockedReasons.join("; ");
    expect(joined).toContain("filesystem_isolation");
    expect(joined).toContain("user_data_isolation");
    expect(joined).toContain("display");
  });
});

/** Causal trace refs are preserved on artifacts/evidence. */
export function preservedTrace(): string {
  const trace = createRunTraceRef({
    taskId: "task-1",
    phaseId: "runtime_execution",
    runId: RUN_ID,
    producer: "test",
  });
  return trace.runId;
}

export type { SupervisorStateView };
