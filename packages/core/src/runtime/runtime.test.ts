import { describe, expect, it } from "vitest";
import { createFixedClock } from "../determinism/context.js";
import {
  createOperationId,
  createRunId,
  createRunTraceRef,
  formatRunTraceRef,
} from "./identity.js";
import { createRunManifest, evaluateRuntimeModeCapability, renderRunManifest } from "./modes.js";
import {
  authorizesSourceMutation,
  cleanupScopeForRun,
  createRunFilesystemBoundary,
  createRuntimeSideEffectPolicy,
  isPathWithinRunRoot,
  resolveRunOwnedPath,
} from "./side-effects.js";
import {
  createRuntimeArtifactRef,
  createRuntimeArtifactStore,
  enforceArtifactBudget,
  planRunCleanup,
  projectRuntimeArtifactsForContext,
  renderCleanupOutcome,
} from "./artifacts.js";
import {
  createRunOutcome,
  digestRunActivity,
  INITIAL_SUPERVISOR_STATE,
  isSupervisorTerminal,
  renderRunOutcome,
  transitionSupervisor,
} from "./supervision.js";
import {
  classifyIncompleteRun,
  createRuntimeBudget,
  finalizeCancellation,
  requestCancellation,
  renderRuntimeBudget,
} from "./budget.js";
import { evaluateRuntimeReadiness, executionAllowed, renderRuntimeReadiness } from "./readiness.js";

const SHA = (letter: string): string => letter.repeat(64);

function runId(sequence = 1) {
  return createRunId({ taskId: "task-1", phaseId: "runtime_execution", sequence, kind: "runtime" });
}

describe("run identity", () => {
  it("is deterministic from host inputs and traceable", () => {
    const a = createRunId({ taskId: "task-1", phaseId: "runtime", sequence: 1 });
    const b = createRunId({ taskId: "task-1", phaseId: "runtime", sequence: 1 });
    expect(b).toBe(a);
    expect(createRunId({ taskId: "task-1", phaseId: "runtime", sequence: 2 })).not.toBe(a);
    expect(a.startsWith("run_runtime_")).toBe(true);
    const operation = createOperationId(a, "spawn");
    expect(operation.startsWith("op_")).toBe(true);
    const trace = createRunTraceRef({
      taskId: "task-1",
      phaseId: "runtime",
      runId: a,
      operationId: operation,
      producer: "supervisor",
    });
    expect(formatRunTraceRef(trace)).toContain("run=");
    expect(formatRunTraceRef(trace)).toContain("producer=supervisor");
  });
});

describe("runtime modes", () => {
  it("headless and visual capabilities are distinct; visual never assumed from Godot", () => {
    const headless = evaluateRuntimeModeCapability({
      mode: "headless",
      godotAvailable: true,
      sandboxSupportsMode: true,
      displayAvailable: null,
      platform: "win32",
    });
    expect(headless.state).toBe("available");
    const visualNoDisplay = evaluateRuntimeModeCapability({
      mode: "visual",
      godotAvailable: true,
      sandboxSupportsMode: true,
      displayAvailable: false,
      platform: "win32",
    });
    expect(visualNoDisplay.state).toBe("blocked");
    const visualUnknown = evaluateRuntimeModeCapability({
      mode: "visual",
      godotAvailable: true,
      sandboxSupportsMode: true,
      displayAvailable: null,
      platform: "win32",
    });
    expect(visualUnknown.state).toBe("degraded");
    const noGodot = evaluateRuntimeModeCapability({
      mode: "headless",
      godotAvailable: false,
      sandboxSupportsMode: true,
      displayAvailable: null,
      platform: "win32",
    });
    expect(noGodot.state).toBe("blocked");
  });
});

describe("RunManifest", () => {
  it("binds H1/H2 identities into a deterministic digest", () => {
    const manifest = createRunManifest({
      taskId: "task-1",
      phaseId: "runtime_execution",
      runId: runId(),
      taskContractDigest: SHA("a"),
      phaseContractDigest: SHA("b"),
      executionInputDigest: SHA("c"),
      reproducibilityDigest: SHA("d"),
      godotExecutableFingerprint: SHA("e"),
      projectIdentity: "ws-1",
      runtimeMode: "headless",
      sandboxProfileId: "runtime-offline",
      sideEffectPolicyDigest: SHA("f"),
      resourceBudgetDigest: SHA("g"),
      environmentDigest: SHA("h"),
    });
    expect(manifest.digest).toMatch(/^[0-9a-f]{64}$/);
    const again = createRunManifest({
      taskId: "task-1",
      phaseId: "runtime_execution",
      runId: runId(),
      taskContractDigest: SHA("a"),
      phaseContractDigest: SHA("b"),
      executionInputDigest: SHA("c"),
      reproducibilityDigest: SHA("d"),
      godotExecutableFingerprint: SHA("e"),
      projectIdentity: "ws-1",
      runtimeMode: "headless",
      sandboxProfileId: "runtime-offline",
      sideEffectPolicyDigest: SHA("f"),
      resourceBudgetDigest: SHA("g"),
      environmentDigest: SHA("h"),
    });
    expect(again.digest).toBe(manifest.digest);
    expect(renderRunManifest(manifest)).toContain("mode=headless");
  });
});

describe("side-effect policy and run-owned boundaries", () => {
  it("policy is host-owned, digest-bound, and never authorizes source mutation", () => {
    const policy = createRuntimeSideEffectPolicy({ network: "loopback" });
    expect(policy.sourceWorkspace).toBe("protected");
    expect(policy.userData).toBe("redirected_run_owned");
    expect(policy.childProcesses).toBe("supervised");
    expect(authorizesSourceMutation(policy)).toBe(false);
    expect(() => createRuntimeSideEffectPolicy({ network: "open" as never })).toThrow(
      /network policy/,
    );
  });

  it("resolves run-owned paths with containment and rejects escapes", () => {
    const boundary = createRunFilesystemBoundary({
      runId: runId(),
      hostRoots: {
        project_copy: "/runs/run1/project",
        user_data: "/runs/run1/userdata",
        temp: "/runs/run1/tmp",
        output: "/runs/run1/output",
        artifacts: "/runs/run1/artifacts",
      },
    });
    const ok = resolveRunOwnedPath(boundary, "user_data", "logs/save.json");
    expect(ok.status).toBe("ok");
    if (ok.status === "ok") {
      expect(ok.absolutePath).toBe("/runs/run1/userdata/logs/save.json");
    }
    expect(resolveRunOwnedPath(boundary, "user_data", "../etc/passwd").status).toBe("rejected");
    expect(resolveRunOwnedPath(boundary, "user_data", "/etc/passwd").status).toBe("rejected");
    expect(resolveRunOwnedPath(boundary, "user_data", "C:\\windows\\x").status).toBe("rejected");
    expect(isPathWithinRunRoot(boundary, "user_data", "/runs/run1/userdata/logs/save.json")).toBe(
      true,
    );
    expect(isPathWithinRunRoot(boundary, "user_data", "/etc/passwd")).toBe(false);
    // Cleanup scope is host-owned run roots only.
    expect(cleanupScopeForRun(boundary)).toEqual([
      "/runs/run1/project",
      "/runs/run1/userdata",
      "/runs/run1/tmp",
      "/runs/run1/output",
      "/runs/run1/artifacts",
    ]);
  });
});

describe("artifact model and budgets", () => {
  it("preserves producer/run/digest metadata and enforces budgets deterministically", () => {
    const ref = createRuntimeArtifactRef({
      id: "art-1",
      runId: runId(),
      kind: "stdout",
      mediaType: "text/plain",
      size: 1024,
      producer: "process-supervisor",
      createdAtMs: 1_000,
      retentionClass: "task",
      location: "/runs/run1/artifacts/art-1.log",
      digest: SHA("1"),
    });
    expect(ref.runId).toBe(runId());
    expect(ref.truncated).toBe(false);
    const admission = enforceArtifactBudget({
      budget: {
        maxArtifactBytes: 512,
        maxArtifactsPerRun: 10,
        maxAggregateBytesPerRun: 100_000,
        maxRetainedBytesPerTask: 1_000_000,
      },
      state: { artifactCount: 0, aggregateBytes: 0 },
      incomingSize: 1024,
      incomingCount: 1,
    });
    expect(admission).toEqual({ status: "admit", truncated: true });
    const limited = enforceArtifactBudget({
      budget: {
        maxArtifactBytes: 512,
        maxArtifactsPerRun: 1,
        maxAggregateBytesPerRun: 100_000,
        maxRetainedBytesPerTask: 1_000_000,
      },
      state: { artifactCount: 1, aggregateBytes: 10 },
      incomingSize: 10,
      incomingCount: 1,
    });
    expect(limited.status).toBe("artifact_limit");
  });

  it("the store never silently drops evidence: limits are explicit outcomes", () => {
    const store = createRuntimeArtifactStore({
      budget: {
        maxArtifactBytes: 1024,
        maxArtifactsPerRun: 1,
        maxAggregateBytesPerRun: 100_000,
        maxRetainedBytesPerTask: 1_000_000,
      },
    });
    const first = createRuntimeArtifactRef({
      id: "art-1",
      runId: runId(),
      kind: "stdout",
      mediaType: "text/plain",
      size: 10,
      producer: "p",
      createdAtMs: 1,
      location: "/runs/run1/artifacts/a",
      digest: SHA("1"),
    });
    const second = createRuntimeArtifactRef({
      id: "art-2",
      runId: runId(),
      kind: "stdout",
      mediaType: "text/plain",
      size: 10,
      producer: "p",
      createdAtMs: 2,
      location: "/runs/run1/artifacts/b",
      digest: SHA("2"),
    });
    expect(store.register(first).status).toBe("registered");
    expect(store.register(second).status).toBe("limit");
    expect(store.list(runId())).toHaveLength(1);
    expect(store.digest()).toMatch(/^[0-9a-f]{64}$/);
  });

  it("plans cleanup idempotently and projects artifacts bounded for context", () => {
    const cleanup = planRunCleanup({
      hostRoots: ["/runs/run1/project", "/runs/run1/tmp"],
      pathsAlreadyCleaned: new Set(["/runs/run1/tmp"]),
    });
    expect(cleanup.toClean).toEqual(["/runs/run1/project"]);
    expect(cleanup.alreadyClean).toEqual(["/runs/run1/tmp"]);
    expect(
      renderCleanupOutcome({
        status: "cleaned",
        cleanedPaths: ["/runs/run1/project"],
        failedPaths: [],
        message: null,
      }),
    ).toContain("cleanup cleaned");
    const refs = [1, 2, 3, 4, 5].map((index) =>
      createRuntimeArtifactRef({
        id: `art-${index}`,
        runId: runId(),
        kind: "stdout",
        mediaType: "text/plain",
        size: 100,
        producer: "p",
        createdAtMs: index,
        location: `/runs/run1/artifacts/${index}`,
        digest: SHA(String(index)),
      }),
    );
    const projection = projectRuntimeArtifactsForContext(refs, 3);
    expect(projection).toContain("[2 further artifact reference(s) not projected]");
  });
});

describe("supervision contract", () => {
  it("transitions deterministically and terminal is absorbing", () => {
    const clock = createFixedClock(1_000);
    const started = transitionSupervisor(
      INITIAL_SUPERVISOR_STATE,
      { type: "startup_result", ok: true, failureKind: null },
      clock.now(),
    );
    expect(started.state).toBe("running");
    const exited = transitionSupervisor(
      started,
      { type: "child_exit", exitCode: 0 },
      clock.now() + 500,
    );
    expect(exited.state).toBe("terminal");
    expect(exited.terminalDisposition).toBe("success");
    // Terminal is absorbing: further observations cannot revive the run.
    const revived = transitionSupervisor(
      exited,
      { type: "liveness", kind: "process_alive" },
      clock.now() + 1_000,
    );
    expect(revived).toBe(exited);
    expect(isSupervisorTerminal(exited)).toBe(true);
  });

  it("startup failure, timeouts, and cancellation produce distinct dispositions", () => {
    const clock = createFixedClock(1_000);
    const spawn = transitionSupervisor(
      INITIAL_SUPERVISOR_STATE,
      { type: "startup_result", ok: false, failureKind: "spawn_failed" },
      clock.now(),
    );
    expect(spawn.terminalDisposition).toBe("failure");
    expect(spawn.failureKind).toBe("spawn_failed");
    const running = transitionSupervisor(
      INITIAL_SUPERVISOR_STATE,
      { type: "startup_result", ok: true, failureKind: null },
      clock.now(),
    );
    const idle = transitionSupervisor(running, { type: "idle_timeout" }, clock.now() + 1_000);
    expect(idle.failureKind).toBe("idle_timeout");
    const hard = transitionSupervisor(running, { type: "hard_timeout" }, clock.now() + 2_000);
    expect(hard.failureKind).toBe("hard_timeout");
    expect(hard.terminalDisposition).toBe("resource_limit");
    const terminating = transitionSupervisor(
      running,
      { type: "cancel_requested" },
      clock.now() + 3_000,
    );
    expect(terminating.state).toBe("terminating");
    const cancelled = transitionSupervisor(
      terminating,
      { type: "kill_result", ok: true },
      clock.now() + 3_100,
    );
    expect(cancelled.terminalDisposition).toBe("cancelled");
    expect(cancelled.failureKind).toBe("cancelled");
    const killFailed = transitionSupervisor(
      terminating,
      { type: "child_refused_termination" },
      clock.now() + 3_200,
    );
    expect(killFailed.failureKind).toBe("kill_failed");
  });

  it("outcome has one terminal disposition and independent cleanup status", () => {
    const outcome = createRunOutcome({
      runId: runId(),
      status: "failure",
      failureKind: "process_crashed",
      exitCode: 1,
      timing: { startedAtMs: 1_000, terminalAtMs: 3_000, totalMs: 2_000 },
      resourceSummary: { stdoutBytes: 10, stderrBytes: 5, artifactCount: 1, childProcesses: 1 },
      cleanupStatus: "failed",
    });
    expect(renderRunOutcome(outcome)).toContain("-> failure (process_crashed)");
    expect(() =>
      createRunOutcome({
        runId: runId(),
        status: "failure",
        failureKind: null,
        timing: { startedAtMs: null, terminalAtMs: null, totalMs: null },
        resourceSummary: { stdoutBytes: 0, stderrBytes: 0, artifactCount: 0, childProcesses: 0 },
      }),
    ).toThrow(/typed failure kind/);
    expect(() =>
      createRunOutcome({
        runId: runId(),
        status: "failure",
        failureKind: "cleanup_failed",
        timing: { startedAtMs: null, terminalAtMs: null, totalMs: null },
        resourceSummary: { stdoutBytes: 0, stderrBytes: 0, artifactCount: 0, childProcesses: 0 },
      }),
    ).toThrow(/independent cleanup status/);
  });

  it("run activity digest is deterministic", () => {
    const events = [
      { type: "RunPrepared" as const, runId: runId(), atMs: 1_000 },
      { type: "RunStarting" as const, runId: runId(), atMs: 1_100 },
      {
        type: "RunTerminal" as const,
        runId: runId(),
        atMs: 2_000,
        disposition: "success" as const,
      },
    ];
    expect(digestRunActivity(events)).toBe(digestRunActivity([...events].reverse()));
  });
});

describe("RuntimeBudget and cancellation", () => {
  it("budget exposes only backend-enforceable limits with capability state", () => {
    const budget = createRuntimeBudget({ memoryMb: null, cpuPercent: null });
    expect(budget.memoryMb).toBeNull();
    expect(renderRuntimeBudget(budget)).toContain("startup=15000ms");
    expect(budget.digest).toMatch(/^[0-9a-f]{64}$/);
    expect(createRuntimeBudget({ memoryMb: 512 }).memoryMb).toBe(512);
  });

  it("cancellation is idempotent with a single cleanup flow", () => {
    const first = requestCancellation(
      { phase: "none", cleanupFlowId: null, requestedAtMs: null },
      runId(),
      1_000,
    );
    expect(first.idempotent).toBe(false);
    expect(first.state.phase).toBe("requested");
    const second = requestCancellation(first.state, runId(), 1_100);
    expect(second.idempotent).toBe(true);
    expect(second.state.cleanupFlowId).toBe(first.state.cleanupFlowId);
    const finalized = finalizeCancellation(second.state);
    expect(finalized.phase).toBe("finalized");
  });
});

describe("restart reconciliation", () => {
  it("classifies incomplete runs conservatively, never fabricating success", () => {
    const interrupted = classifyIncompleteRun(
      { runId: runId(), lastKnownState: "prepared", lastObservedAtMs: 1_000 },
      false,
    );
    expect(interrupted.classification).toBe("interrupted");
    const running = classifyIncompleteRun(
      { runId: runId(), lastKnownState: "running", lastObservedAtMs: 2_000 },
      true,
    );
    expect(running.classification).toBe("cleanup_required");
    const unknown = classifyIncompleteRun(
      { runId: runId(), lastKnownState: "running", lastObservedAtMs: 2_000 },
      false,
    );
    expect(unknown.classification).toBe("unknown");
  });
});

describe("readiness manifest", () => {
  const readyInput = {
    runtimeMode: "headless" as const,
    godotExecutable: { available: true, fingerprint: SHA("e") },
    projectIdentity: "ws-1",
    sandboxBackend: { available: true, supportsProcessSupervision: true },
    filesystemIsolation: { available: true, userDataRedirect: true },
    networkPolicyResolvable: true,
    artifactStorageAvailable: true,
    displayAvailable: null,
    resourceLimitCapabilities: { memory: false, cpu: false },
  };

  it("is deterministic and ready for a fully provisioned headless run", () => {
    const manifest = evaluateRuntimeReadiness(readyInput);
    expect(manifest.ready).toBe(true);
    expect(executionAllowed(manifest)).toBe(true);
    expect(evaluateRuntimeReadiness(readyInput).digest).toBe(manifest.digest);
    expect(renderRuntimeReadiness(manifest)).toContain("ready");
  });

  it("fails closed when required isolation is unavailable", () => {
    const blocked = evaluateRuntimeReadiness({
      ...readyInput,
      sandboxBackend: { available: false, supportsProcessSupervision: false },
    });
    expect(blocked.ready).toBe(false);
    expect(executionAllowed(blocked)).toBe(false);
    expect(blocked.blockedReasons.some((reason) => reason.includes("sandbox_backend"))).toBe(true);
    const visualNoDisplay = evaluateRuntimeReadiness({
      ...readyInput,
      runtimeMode: "visual",
      displayAvailable: false,
    });
    expect(visualNoDisplay.ready).toBe(false);
    expect(visualNoDisplay.blockedReasons.some((reason) => reason.includes("display"))).toBe(true);
  });

  it("never silently downgrades security: unsupported process supervision blocks", () => {
    const degraded = evaluateRuntimeReadiness({
      ...readyInput,
      sandboxBackend: { available: true, supportsProcessSupervision: false },
    });
    expect(degraded.ready).toBe(false);
    expect(degraded.blockedReasons.some((reason) => reason.includes("process_supervision"))).toBe(
      true,
    );
  });
});
