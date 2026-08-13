import { describe, expect, it } from "vitest";
import { createTaskPlan, type TaskPlan } from "../planning/planning-model.js";
import type { EvidenceKind } from "./task-model.js";
import { createTaskContract } from "./task-contract.js";
import {
  createTaskRuntime,
  MAX_TASK_EVIDENCE_RECORDS,
  MAX_TASK_FINDINGS,
  type TaskHandle,
} from "./task-runtime.js";
import { createTaskRuntimeSnapshot, TASK_RUNTIME_VERSION } from "./task-snapshot.js";

function createInput(id = "task-runtime-invariant") {
  return {
    contract: createTaskContract({
      id,
      request: "Verify runtime invariants",
      acceptanceCriteria: [
        {
          id: "verified",
          description: "The host verified the task.",
          verificationKind: "deterministic" as const,
        },
      ],
    }),
    snapshot: createTaskRuntimeSnapshot({
      runtimeVersion: TASK_RUNTIME_VERSION,
      provider: null,
      sandboxProfileId: null,
      capabilityPolicyRevision: null,
      workspaceIdentity: null,
      godotEngineFingerprint: null,
      workflow: null,
    }),
    steps: [],
  };
}

function complete(handle: TaskHandle): void {
  expect(handle.transitionPhase("working").status).toBe("ok");
  expect(
    handle.attachEvidence({
      id: "evidence-1",
      kind: "workspace_read",
      source: { type: "workspace_read", paths: ["project.godot"] },
      verification: {
        checkId: "verified",
        criterionId: "verified",
        milestone: null,
        outcome: "passed",
      },
    }).status,
  ).toBe("attached");
  expect(handle.verifyCriterion("verified", "evidence-1").status).toBe("verified");
  handle.setValidationStatus("clean");
  handle.setReviewStatus("clean");
  expect(handle.completeTask().status).toBe("completed");
}

function createMutablePlan(taskId: string): TaskPlan {
  return structuredClone(
    createTaskPlan({
      id: `plan-${taskId}`,
      taskId,
      taskContractDigest: "a".repeat(64),
      taskContractRevision: 1,
      depth: "full",
      content: {
        objective: "Keep authoritative task plans detached.",
        scope: { inScope: ["task runtime"], outOfScope: [] },
        nonGoals: [],
        touchpoints: [],
        constraints: [],
        risks: [],
        steps: [
          {
            id: "store-plan",
            title: "Store the validated plan",
            expectedTouchpoints: [],
            verification: ["verified"],
          },
        ],
        validation: { checks: ["task runtime tests"] },
      },
      createdAt: 1000,
    }),
  );
}

describe("task runtime invariants", () => {
  it("validates a supplied contract before it becomes authoritative state", () => {
    const input = createInput("task-invalid-contract");
    const invalid = {
      ...input.contract,
      acceptanceCriteria: [],
    };

    expect(() =>
      createTaskRuntime().createTask({
        ...input,
        contract: invalid,
      }),
    ).toThrow(/at least one acceptance criterion/);
  });

  it("rejects duplicate task ids instead of replacing authoritative history", () => {
    const runtime = createTaskRuntime();
    const input = createInput("task-duplicate");
    const first = runtime.createTask(input);

    expect(() => runtime.createTask(input)).toThrow(/already exists/);
    expect(runtime.getTask("task-duplicate")?.activityLog()).toEqual(first.activityLog());
    expect(runtime.listTasks()).toHaveLength(1);
  });

  it("bounds the number of evidence records retained by one task", () => {
    const handle = createTaskRuntime().createTask(createInput("task-evidence-bound"));
    handle.transitionPhase("working");
    for (let index = 0; index < MAX_TASK_EVIDENCE_RECORDS; index += 1) {
      const result = handle.attachEvidence({
        id: `evidence-${index}`,
        kind: "workspace_read",
        source: { type: "workspace_read", paths: [`file-${index}.gd`] },
      });
      expect(result.status).toBe("attached");
    }

    const overflow = handle.attachEvidence({
      id: "evidence-overflow",
      kind: "workspace_read",
      source: { type: "workspace_read", paths: ["overflow.gd"] },
    });

    expect(overflow.status).toBe("rejected");
    expect(overflow.reason).toContain(`${MAX_TASK_EVIDENCE_RECORDS}`);
    expect(handle.snapshot().evidence).toHaveLength(MAX_TASK_EVIDENCE_RECORDS);
  });

  it("binds an evidence kind to its matching source type", () => {
    const handle = createTaskRuntime().createTask(createInput("task-evidence-kind"));

    const result = handle.attachEvidence({
      id: "mismatched-evidence",
      kind: "review_result",
      source: { type: "workspace_read", paths: ["project.godot"] },
    });

    expect(result).toMatchObject({ status: "rejected" });
    expect(result.reason).toContain("requires source type review");
    expect(handle.snapshot().evidence).toEqual([]);
  });

  it("detaches task specs, evidence sources, and runtime snapshots from caller-owned data", () => {
    const accepts: EvidenceKind[] = ["workspace_read"];
    const input = createInput("task-detached-input");
    const snapshot = structuredClone(input.snapshot);
    const handle = createTaskRuntime().createTask({
      ...input,
      snapshot,
      steps: [
        {
          id: "inspect",
          description: "Inspect the project",
          kind: "research",
          accepts,
        },
      ],
    });
    accepts[0] = "review_result";
    (snapshot as { workspaceIdentity: string | null }).workspaceIdentity = "forged";
    const paths = ["project.godot"];
    expect(
      handle.attachEvidence({
        id: "detached-evidence",
        kind: "workspace_read",
        source: { type: "workspace_read", paths },
      }).status,
    ).toBe("attached");
    paths[0] = "forged.gd";
    expect(handle.beginStep("inspect").status).toBe("ok");
    expect(
      handle.completeStep("inspect", [{ evidenceId: "detached-evidence", kind: "workspace_read" }])
        .status,
    ).toBe("ok");

    expect(handle.runtimeSnapshot().workspaceIdentity).toBeNull();
    expect(handle.snapshot().evidence[0]?.source).toMatchObject({
      type: "workspace_read",
      paths: ["project.godot"],
    });
  });

  it("validates and detaches plan revisions at the runtime storage boundary", () => {
    const handle = createTaskRuntime().createTask(createInput("task-plan-detached"));
    const plan = createMutablePlan("task-plan-detached");

    expect(handle.setPlan(plan).status).toBe("ok");
    (plan as { objective: string }).objective = "forged objective";
    (plan.steps as unknown as Array<{ title: string }>)[0]!.title = "forged step";

    expect(handle.currentPlan()?.objective).toBe("Keep authoritative task plans detached.");
    expect(handle.currentPlan()?.steps[0]?.title).toBe("Store the validated plan");

    const oversized = createMutablePlan("task-plan-detached");
    (oversized as { id: string }).id = "plan-replacement";
    (oversized as { objective: string }).objective = "x".repeat(2049);
    expect(handle.setPlan(oversized)).toMatchObject({ status: "rejected" });
    expect(handle.planRevisions()).toHaveLength(1);
  });

  it("reconciles acceptance state when the TaskContract revision changes", () => {
    const handle = createTaskRuntime().createTask(createInput("task-revised-acceptance"));
    expect(
      handle.attachEvidence({
        id: "ev-verified",
        kind: "workspace_read",
        source: { type: "workspace_read", paths: ["project.godot"] },
        verification: {
          checkId: "verified",
          criterionId: "verified",
          milestone: null,
          outcome: "passed",
        },
      }).status,
    ).toBe("attached");
    expect(handle.verifyCriterion("verified", "ev-verified").status).toBe("verified");

    handle.reviseContract({
      id: "task-revised-acceptance",
      acceptanceCriteria: [
        {
          id: "verified",
          description: "The host verified the task.",
          verificationKind: "deterministic",
        },
        {
          id: "new-check",
          description: "The new contract requirement is verified.",
          verificationKind: "review",
        },
      ],
    });

    expect(handle.snapshot().acceptance).toMatchObject([
      { criterionId: "verified", status: "pending", verifiedBy: null },
      { criterionId: "new-check", status: "pending" },
    ]);
    expect(handle.evaluateCompletion().missing).toContain(
      "acceptance criterion not satisfied: new-check",
    );

    handle.reviseContract({
      id: "task-revised-acceptance",
      acceptanceCriteria: [
        {
          id: "verified",
          description: "The meaning of this criterion changed.",
          verificationKind: "deterministic",
        },
      ],
    });
    expect(handle.snapshot().acceptance).toMatchObject([
      { criterionId: "verified", status: "pending", verifiedBy: null },
    ]);
  });

  it("rejects null, stale, failed, and criterion-mismatched verification evidence", () => {
    const handle = createTaskRuntime().createTask(createInput("task-exact-verification"));
    expect(handle.verifyCriterion("verified", null)).toMatchObject({ status: "rejected" });

    expect(
      handle.attachEvidence({
        id: "ev-wrong",
        kind: "workspace_read",
        source: { type: "workspace_read", paths: ["project.godot"] },
        verification: {
          checkId: "other",
          criterionId: "other",
          milestone: null,
          outcome: "passed",
        },
      }).status,
    ).toBe("attached");
    expect(handle.verifyCriterion("verified", "ev-wrong")).toMatchObject({ status: "rejected" });

    expect(
      handle.attachEvidence({
        id: "ev-failed",
        kind: "parser_result",
        source: { type: "parser", checkedFiles: 1, validFiles: 0, errors: 1 },
        verification: {
          checkId: "verified",
          criterionId: "verified",
          milestone: null,
          outcome: "failed",
        },
      }).status,
    ).toBe("attached");
    expect(handle.verifyCriterion("verified", "ev-failed")).toMatchObject({ status: "rejected" });

    expect(
      handle.attachEvidence({
        id: "ev-current",
        kind: "workspace_read",
        source: { type: "workspace_read", paths: ["project.godot"] },
        verification: {
          checkId: "verified",
          criterionId: "verified",
          milestone: null,
          outcome: "passed",
        },
      }).status,
    ).toBe("attached");
    handle.reviseContract({ id: "task-exact-verification", context: "new task context" });
    expect(handle.verifyCriterion("verified", "ev-current")).toMatchObject({ status: "rejected" });
  });

  it("rejects an unbounded findings replacement without changing current findings", () => {
    const handle = createTaskRuntime().createTask(createInput("task-finding-bound"));
    expect(() =>
      handle.setFindings(
        Array.from({ length: MAX_TASK_FINDINGS + 1 }, (_, index) => ({
          findingId: `finding-${index}`,
          severity: "low" as const,
          source: "runtime-test",
        })),
      ),
    ).toThrow(/at most/);
    expect(handle.snapshot().currentFindings).toEqual([]);
  });

  it("keeps terminal task state and activity immutable", () => {
    const handle = createTaskRuntime({ now: () => 1000 }).createTask(createInput("task-terminal"));
    complete(handle);
    const before = handle.snapshot();
    const activityBefore = handle.activityLog();
    const progressBefore = handle.progress();

    expect(handle.beginStep("unknown").status).toBe("rejected");
    expect(
      handle.attachEvidence({
        id: "late-evidence",
        kind: "workspace_read",
        source: { type: "workspace_read", paths: ["late.gd"] },
      }).status,
    ).toBe("rejected");
    expect(handle.markCriterionFailed("verified").status).toBe("rejected");
    expect(handle.completeTask()).toMatchObject({ status: "rejected" });
    expect(
      handle.submitDisposition({ type: "continue", nextAction: "late continuation" }).accepted,
    ).toBe(false);
    expect(() => handle.reviseContract({ id: "task-terminal", request: "late revision" })).toThrow(
      /terminal/,
    );

    handle.setValidationStatus("failed");
    handle.setReviewStatus("findings");
    handle.setIteration(99);
    handle.setFindings([{ findingId: "late", severity: "high", source: "must-not-be-retained" }]);
    handle.routePlanning("full", "explicit-plan-request");
    handle.rejectPlan("late rejection");
    handle.observe({ action: "late", fingerprint: "late", progress: true });

    expect(handle.snapshot()).toEqual(before);
    expect(handle.activityLog()).toEqual(activityBefore);
    expect(handle.progress()).toEqual(progressBefore);
  });
});
