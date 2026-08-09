import { describe, expect, it } from "vitest";
import {
  TASK_ACTIVITY_EVENT_KEYS,
  TASK_ACTIVITY_EVENT_TYPES,
  capabilityPolicyFingerprint,
  createDefaultPolicy,
  createSolarisApplication,
  createTaskContract,
  createTaskRuntime,
  createTaskRuntimeSnapshot,
  createToolRegistry,
  type TaskContract,
  type TaskRuntimeSnapshot,
  type TaskRuntimeSnapshotSources,
  type TaskStepSpec,
} from "@solaris/core";
import { createScriptedProvider } from "@solaris/core";
import { createBehaviorRuntime, makeSnapshot } from "./behavior-harness.js";

/**
 * Behavior fixtures 1-12 (Stage 3 milestone 1 §20).
 *
 * These verify user-observable runtime behavior at the final observable
 * boundary — the TaskRuntime handle API and, where the provider is
 * involved, the application tool loop — not implementation internals.
 */

function contract(id = "task-1"): TaskContract {
  return createTaskContract({
    id,
    request: "Implement the requested change",
    acceptanceCriteria: [
      {
        id: "criterion-1",
        description: "The change is complete.",
        verificationKind: "deterministic",
      },
    ],
    pausePolicy: "none",
  });
}

function steps(): readonly TaskStepSpec[] {
  return [
    {
      id: "implement",
      description: "Implement the change",
      kind: "implementation",
      accepts: ["mutation_receipt"],
    },
    {
      id: "research",
      description: "Inspect the workspace",
      kind: "research",
      accepts: ["workspace_read", "api_lookup"],
    },
  ];
}

interface Fixture {
  readonly runtime: ReturnType<typeof createTaskRuntime>;
  readonly snapshot: TaskRuntimeSnapshot;
  readonly sources: TaskRuntimeSnapshotSources;
  readonly now: () => number;
}

function fixture(): Fixture {
  const { runtime, sources, now } = createBehaviorRuntime();
  return { runtime, snapshot: makeSnapshot(sources, now), sources, now };
}

function createTask(
  f: Fixture,
  options: { readonly criteriaSatisfied?: boolean; readonly steps?: readonly TaskStepSpec[] } = {},
) {
  const handle = f.runtime.createTask({
    contract: contract(),
    snapshot: f.snapshot,
    steps: options.steps ?? steps(),
  });
  handle.transitionPhase("working");
  if (options.criteriaSatisfied === true) {
    handle.verifyCriterion("criterion-1", null);
  }
  return handle;
}

/** Complete the research step with valid workspace-read evidence. */
function completeResearch(handle: ReturnType<typeof createTask>): void {
  handle.beginStep("research");
  const attached = handle.attachEvidence({
    id: "ev-read-1",
    kind: "workspace_read",
    source: { type: "workspace_read", paths: ["scripts/player/player.gd"] },
  });
  expect(attached.status).toBe("attached");
  const completed = handle.completeStep("research", [
    { evidenceId: "ev-read-1", kind: "workspace_read" },
  ]);
  expect(completed).toEqual({ status: "ok" });
}

describe("Behavior 1 — the model cannot declare success without evidence", () => {
  it("a model-issued complete disposition is rejected while the step has no evidence", () => {
    const f = fixture();
    const handle = createTask(f);
    handle.beginStep("implement");
    const result = handle.submitDisposition({ type: "complete" }, "model");
    expect(result.accepted).toBe(false);
    expect(result.evaluation?.missing).toContain("acceptance criterion not satisfied: criterion-1");
    expect(result.reason).toBe(result.evaluation?.missing[0]);
    expect(handle.snapshot().phase).not.toBe("completed");
    expect(handle.snapshot().steps[0]?.status).toBe("active");
    const log = handle.activityLog();
    expect(
      log.some((event) => event.type === "disposition_submitted" && event.accepted === false),
    ).toBe(true);
    expect(log.some((event) => event.type === "task_completed")).toBe(false);
  });

  it("provider text claiming completion never reaches the runtime (application boundary)", async () => {
    const f = fixture();
    const handle = createTask(f);
    handle.beginStep("implement");
    const { provider } = createScriptedProvider([
      [{ type: "text_delta", text: "The task is complete!" }, { type: "completed" }],
    ]);
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([]),
      policy: createDefaultPolicy("inspect"),
    });
    for await (const _event of application.sendPrompt("finish the task")) {
      // drain
    }
    const snapshot = handle.snapshot();
    expect(snapshot.phase).toBe("working");
    expect(snapshot.steps[0]?.status).toBe("active");
    expect(snapshot.steps[0]?.evidenceRefs).toEqual([]);
  });
});

describe("Behavior 2 — valid existing evidence permits step completion", () => {
  it("completes a step with a valid task-scoped evidence reference", () => {
    const f = fixture();
    const handle = createTask(f);
    handle.beginStep("implement");
    const attached = handle.attachEvidence({
      id: "ev-mutation-1",
      kind: "mutation_receipt",
      source: { type: "mutation", changeSetId: "cs-1", checkpointId: "cp-1" },
    });
    expect(attached.status).toBe("attached");
    const completed = handle.completeStep("implement", [
      { evidenceId: "ev-mutation-1", kind: "mutation_receipt" },
    ]);
    expect(completed).toEqual({ status: "ok" });
    const snapshot = handle.snapshot();
    expect(snapshot.steps[0]?.status).toBe("completed");
    expect(snapshot.steps[0]?.evidenceRefs).toEqual([
      { evidenceId: "ev-mutation-1", kind: "mutation_receipt" },
    ]);
    expect(
      handle
        .activityLog()
        .some((event) => event.type === "step_completed" && event.stepId === "implement"),
    ).toBe(true);
  });
});

describe("Behavior 3 — invalid or missing evidence is rejected (no silent acceptance)", () => {
  it("rejects unknown evidence ids", () => {
    const f = fixture();
    const handle = createTask(f);
    handle.beginStep("implement");
    const result = handle.completeStep("implement", [
      { evidenceId: "ev-missing", kind: "mutation_receipt" },
    ]);
    expect(result.status).toBe("rejected");
    if (result.status === "rejected") {
      expect(result.reason).toContain("Unknown evidence reference");
    }
    expect(handle.snapshot().steps[0]?.status).toBe("active");
  });

  it("rejects evidence of the wrong kind for the step", () => {
    const f = fixture();
    const handle = createTask(f);
    handle.beginStep("implement");
    handle.attachEvidence({
      id: "ev-read-1",
      kind: "workspace_read",
      source: { type: "workspace_read", paths: ["a.gd"] },
    });
    const result = handle.completeStep("implement", [
      { evidenceId: "ev-read-1", kind: "workspace_read" },
    ]);
    expect(result.status).toBe("rejected");
    if (result.status === "rejected") {
      expect(result.reason).toContain("does not accept evidence kind workspace_read");
    }
  });

  it("rejects evidence attached to another task", () => {
    const f = fixture();
    const other = f.runtime.createTask({
      contract: contract("task-other"),
      snapshot: f.snapshot,
      steps: steps(),
    });
    other.beginStep("implement");
    other.attachEvidence({
      id: "ev-foreign",
      kind: "mutation_receipt",
      source: { type: "mutation", changeSetId: "cs-9", checkpointId: null },
    });
    const handle = createTask(f);
    handle.beginStep("implement");
    const result = handle.completeStep("implement", [
      { evidenceId: "ev-foreign", kind: "mutation_receipt" },
    ]);
    expect(result.status).toBe("rejected");
    if (result.status === "rejected") {
      expect(result.reason).toContain("Unknown evidence reference");
    }
  });

  it("rejects completion with no evidence references at all", () => {
    const f = fixture();
    const handle = createTask(f);
    handle.beginStep("implement");
    const result = handle.completeStep("implement", []);
    expect(result.status).toBe("rejected");
    if (result.status === "rejected") {
      expect(result.reason).toContain("requires at least one evidence reference");
    }
  });

  it("rejects completing a step that was never started", () => {
    const f = fixture();
    const handle = createTask(f);
    handle.attachEvidence({
      id: "ev-mutation-1",
      kind: "mutation_receipt",
      source: { type: "mutation", changeSetId: "cs-1", checkpointId: null },
    });
    const result = handle.completeStep("implement", [
      { evidenceId: "ev-mutation-1", kind: "mutation_receipt" },
    ]);
    expect(result.status).toBe("rejected");
    if (result.status === "rejected") {
      expect(result.reason).toContain("is not active");
    }
    expect(handle.activityLog().some((event) => event.type === "step_completed")).toBe(false);
  });

  it("rejects duplicate evidence references", () => {
    const f = fixture();
    const handle = createTask(f);
    handle.beginStep("implement");
    handle.attachEvidence({
      id: "ev-mutation-1",
      kind: "mutation_receipt",
      source: { type: "mutation", changeSetId: "cs-1", checkpointId: null },
    });
    const result = handle.completeStep("implement", [
      { evidenceId: "ev-mutation-1", kind: "mutation_receipt" },
      { evidenceId: "ev-mutation-1", kind: "mutation_receipt" },
    ]);
    expect(result.status).toBe("rejected");
    if (result.status === "rejected") {
      expect(result.reason).toContain("Duplicate evidence reference");
    }
  });
});

describe("Behavior 4 — a completion request does not bypass acceptance criteria", () => {
  it("task remains non-complete while required criteria are unsatisfied", () => {
    const f = fixture();
    const handle = createTask(f);
    handle.beginStep("implement");
    handle.attachEvidence({
      id: "ev-mutation-1",
      kind: "mutation_receipt",
      source: { type: "mutation", changeSetId: "cs-1", checkpointId: "cp-1" },
    });
    handle.completeStep("implement", [{ evidenceId: "ev-mutation-1", kind: "mutation_receipt" }]);
    // All steps complete, but the acceptance criterion was never verified.
    const result = handle.submitDisposition({ type: "complete" }, "model");
    expect(result.accepted).toBe(false);
    expect(result.evaluation?.allowed).toBe(false);
    expect(result.evaluation?.missing).toContain("acceptance criterion not satisfied: criterion-1");
    expect(handle.snapshot().phase).toBe("working");
    expect(handle.snapshot().completedAtMs).toBeNull();
  });

  it("completion stays blocked while validation or review is not clean", () => {
    const f = fixture();
    const handle = createTask(f);
    handle.verifyCriterion("criterion-1", null);
    completeResearch(handle);
    handle.beginStep("implement");
    handle.attachEvidence({
      id: "ev-mutation-1",
      kind: "mutation_receipt",
      source: { type: "mutation", changeSetId: "cs-1", checkpointId: "cp-1" },
    });
    handle.completeStep("implement", [{ evidenceId: "ev-mutation-1", kind: "mutation_receipt" }]);
    handle.setValidationStatus("failed");
    handle.setReviewStatus("clean");
    const result = handle.submitDisposition({ type: "complete" });
    expect(result.accepted).toBe(false);
    expect(result.evaluation?.missing).toContain("validation is failed (clean required)");
    expect(handle.snapshot().phase).toBe("working");
  });
});

describe("Behavior 5 — read-only work does not require mutation evidence", () => {
  it("a research-style task completes on read evidence alone", () => {
    const f = fixture();
    const handle = f.runtime.createTask({
      contract: contract(),
      snapshot: f.snapshot,
      steps: [
        {
          id: "research",
          description: "Inspect the workspace",
          kind: "research",
          accepts: ["workspace_read", "api_lookup"],
        },
      ],
    });
    handle.transitionPhase("working");
    completeResearch(handle);
    handle.verifyCriterion("criterion-1", "ev-read-1");
    handle.setValidationStatus("clean");
    handle.setReviewStatus("clean");
    const evaluation = handle.evaluateCompletion();
    expect(evaluation.allowed).toBe(true);
    const completed = handle.completeTask();
    expect(completed).toEqual({ status: "completed" });
    expect(handle.snapshot().phase).toBe("completed");
    // No mutation evidence was ever attached.
    expect(handle.snapshot().evidence.every((entry) => entry.kind !== "mutation_receipt")).toBe(
      true,
    );
  });
});

describe("Behavior 6 — TaskState has one authoritative mutation path", () => {
  it("mutating a returned snapshot never affects the authoritative state", () => {
    const f = fixture();
    const handle = createTask(f);
    const snapshot = handle.snapshot();
    // A consumer could try to mutate the snapshot; it is a detached copy.
    const mutable = snapshot as unknown as {
      readonly steps: Array<{
        id: string;
        description: string;
        kind: string;
        status: string;
        evidenceRefs: unknown[];
        failedReason: null;
        blockedReason: null;
      }>;
      phase: string;
    };
    mutable.steps.push({
      id: "forged",
      description: "forged",
      kind: "research",
      status: "completed",
      evidenceRefs: [],
      failedReason: null,
      blockedReason: null,
    });
    mutable.phase = "completed";
    const fresh = handle.snapshot();
    expect(fresh.phase).toBe("working");
    expect(fresh.steps).toHaveLength(2);
    expect(fresh.steps.some((step) => step.id === "forged")).toBe(false);
  });

  it("runtime state only changes through the handle API", () => {
    const f = fixture();
    const handle = createTask(f);
    const before = handle.snapshot();
    // No operation was performed; the state is bit-identical.
    expect(handle.snapshot()).toEqual(before);
    // task_started + the host transition to working are the only records.
    expect(handle.activityLog()).toHaveLength(2);
  });

  it("completion is reachable only through the host completion gate", () => {
    const f = fixture();
    const handle = createTask(f);
    // Even a host cannot phase-transition into "completed": completion is
    // exclusively the gate path (completeTask / submitDisposition).
    const result = handle.transitionPhase("completed");
    expect(result.status).toBe("rejected");
    if (result.status === "rejected") {
      expect(result.reason).toContain("not allowed");
    }
    expect(handle.snapshot().phase).toBe("working");
    expect(handle.activityLog().some((event) => event.type === "task_completed")).toBe(false);
  });
});

describe("Behavior 7 — identical repeated actions do not repeatedly count as progress", () => {
  it("repeating the same action with the same result drives the lease to stalled without new progress", () => {
    const f = fixture();
    const handle = createTask(f);
    const observation = { action: "tool.workspace.search", fingerprint: "no-matches" };
    for (let index = 0; index < 6; index += 1) {
      handle.observe(observation);
    }
    const progress = handle.progress();
    expect(progress.state).toBe("stalled");
    // Only the first occurrence counted as useful (plus the task-started
    // observation the runtime records at creation).
    expect(progress.usefulObservations).toBe(2);
    expect(progress.repeatedActions).toBe(6);
  });

  it("identical failed commands do not renew progress", () => {
    const f = fixture();
    const handle = createTask(f);
    handle.observe({ action: "tool.process.run", fingerprint: "exit:1" });
    handle.observe({ action: "tool.process.run", fingerprint: "exit:1" });
    handle.observe({ action: "tool.process.run", fingerprint: "exit:1" });
    expect(handle.progress().state).toBe("degraded");
    expect(handle.progress().usefulObservations).toBe(2);
  });
});

describe("Behavior 8 — genuinely new useful evidence renews progress", () => {
  it("new evidence resets repetition and returns the lease to healthy", () => {
    const f = fixture();
    const handle = createTask(f);
    handle.observe({ action: "tool.workspace.read", fingerprint: "same-file" });
    handle.observe({ action: "tool.workspace.read", fingerprint: "same-file" });
    handle.observe({ action: "tool.workspace.read", fingerprint: "same-file" });
    expect(handle.progress().state).toBe("degraded");
    const progress = handle.observe({
      action: "evidence.attached",
      fingerprint: "parser_result",
      progress: true,
    });
    expect(progress.state).toBe("healthy");
    expect(progress.usefulObservations).toBe(3);
  });

  it("a genuinely different result also counts as new progress", () => {
    const f = fixture();
    const handle = createTask(f);
    handle.observe({ action: "tool.workspace.read", fingerprint: "file-a" });
    handle.observe({ action: "tool.workspace.read", fingerprint: "file-b" });
    expect(handle.progress().state).toBe("healthy");
    // task.started + file-a + file-b
    expect(handle.progress().usefulObservations).toBe(3);
  });

  it("an alternating loop of two identical actions eventually stalls within the bounded window", () => {
    const f = fixture();
    const handle = createTask(f);
    // A strictly alternating pair of actions produces no new useful state;
    // once the initially-fresh entries age out of the bounded window the
    // lease must degrade and then stall deterministically.
    for (let index = 0; index < 10; index += 1) {
      handle.observe({
        action: "tool.workspace.search",
        fingerprint: index % 2 === 0 ? "no-matches-a" : "no-matches-b",
      });
    }
    expect(handle.progress().state).toBe("stalled");
    // Only the first occurrence of each distinct key counted as useful.
    expect(handle.progress().usefulObservations).toBe(3);
  });
});

describe("Behavior 9 — TaskRuntimeSnapshot remains unchanged after an ordinary global config change", () => {
  it("later config changes do not alter the captured snapshot", () => {
    const f = fixture();
    const handle = createTask(f);
    const captured = handle.runtimeSnapshot();
    // Simulate an ordinary global configuration change after task start.
    void createDefaultPolicy("inspect");
    expect(handle.runtimeSnapshot()).toBe(captured);
    expect(captured.capabilityPolicyRevision).toBe(f.sources.capabilityPolicyRevision);
    expect(captured.sandboxProfileId).toBe("develop-offline");
  });

  it("the snapshot is frozen and rejects mutation", () => {
    const f = fixture();
    const handle = createTask(f);
    const captured = handle.runtimeSnapshot();
    expect(Object.isFrozen(captured)).toBe(true);
    expect(() => {
      (captured as { sandboxProfileId: string }).sandboxProfileId = "inspect";
    }).toThrow();
  });
});

describe("Behavior 10 — a new task receives the new configuration", () => {
  it("a task created after a config change captures the new values", () => {
    const f = fixture();
    const first = f.runtime.createTask({ contract: contract("task-a"), snapshot: f.snapshot });
    const changedFingerprint = capabilityPolicyFingerprint(createDefaultPolicy("inspect"));
    expect(changedFingerprint).not.toBe(f.sources.capabilityPolicyRevision);
    const secondSnapshot = createTaskRuntimeSnapshot(
      { ...f.sources, capabilityPolicyRevision: changedFingerprint },
      f.now,
    );
    const second = f.runtime.createTask({
      contract: contract("task-b"),
      snapshot: secondSnapshot,
    });
    expect(first.runtimeSnapshot().capabilityPolicyRevision).toBe(
      f.sources.capabilityPolicyRevision,
    );
    expect(second.runtimeSnapshot().capabilityPolicyRevision).toBe(changedFingerprint);
    expect(second.runtimeSnapshot().capturedAtMs).toBeGreaterThan(
      first.runtimeSnapshot().capturedAtMs,
    );
  });
});

describe("Behavior 11 — the task activity log is append-only", () => {
  it("returned copies cannot mutate the log, and records are immutable", () => {
    const f = fixture();
    const handle = createTask(f);
    const firstView = handle.activityLog();
    const lengthBefore = firstView.length;
    (firstView[0] as { phase: string }).phase = "forged";
    (firstView as { length: number }).length = 0;
    handle.beginStep("implement");
    const secondView = handle.activityLog();
    expect(secondView).toHaveLength(lengthBefore + 1);
    expect(secondView[0]?.type).toBe("task_started");
    expect(secondView.some((event) => event.type === "step_started")).toBe(true);
    // Mutating the second view does not affect subsequent reads either.
    (secondView as unknown[]).push({ type: "forged" });
    expect(handle.activityLog()).toHaveLength(lengthBefore + 1);
  });

  it("sequences are deterministic and strictly increasing per task", () => {
    const f = fixture();
    const handle = createTask(f);
    handle.beginStep("implement");
    const sequences = handle.activityLog().map((event) => event.sequence);
    expect(sequences).toEqual([1, 2, 3]);
    expect(handle.activityLog().every((event) => event.taskId === handle.taskId)).toBe(true);
  });
});

describe("Behavior 12 — activity events contain no provider-private continuation state", () => {
  it("every activity record uses only the typed allowlist of fields", () => {
    const f = fixture();
    const handle = createTask(f);
    handle.beginStep("implement");
    handle.attachEvidence({
      id: "ev-mutation-1",
      kind: "mutation_receipt",
      source: { type: "mutation", changeSetId: "cs-1", checkpointId: null },
    });
    handle.completeStep("implement", [{ evidenceId: "ev-mutation-1", kind: "mutation_receipt" }]);
    handle.submitDisposition({ type: "blocked", reason: "awaiting user input" }, "model");
    handle.submitDisposition({ type: "continue", nextAction: "propose repair" }, "model");
    handle.cancel("user cancelled");
    const allowedTypes = new Set<string>(TASK_ACTIVITY_EVENT_TYPES);
    const allowedKeys = new Set<string>(TASK_ACTIVITY_EVENT_KEYS);
    for (const event of handle.activityLog()) {
      expect(allowedTypes.has(event.type)).toBe(true);
      for (const key of Object.keys(event)) {
        expect(allowedKeys.has(key)).toBe(true);
      }
      // No provider continuation fields may appear anywhere in the record.
      const serialized = JSON.stringify(event);
      expect(serialized.includes("continuation")).toBe(false);
      expect(serialized.includes("providerState")).toBe(false);
      expect(serialized.includes("rawOutput")).toBe(false);
    }
  });
});
