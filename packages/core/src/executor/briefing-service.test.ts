import { describe, expect, it } from "vitest";
import { createExecutorBriefing } from "./briefing-service.js";
import { DEFAULT_EXECUTION_CONTRACT, reviseExecutionContract } from "./execution-contract.js";
import { S3M8_MILESTONE_MANIFEST } from "./s3m8-manifest.js";
import { createTaskRuntime, type TaskRuntime } from "../tasks/task-runtime.js";
import { createTaskContract, type TaskContract } from "../tasks/task-contract.js";
import {
  createTaskRuntimeSnapshot,
  TASK_RUNTIME_VERSION,
  type TaskRuntimeSnapshot,
} from "../tasks/task-snapshot.js";
import { createTaskPlan, type TaskPlan } from "../planning/planning-model.js";
import { createWorkspaceScope, promoteCandidateFile } from "./workspace-scope.js";

function makeRuntime(): TaskRuntime {
  return createTaskRuntime({ now: () => 1000 });
}

function startTask(runtime: TaskRuntime, request: string): TaskContract {
  const contract = createTaskContract({
    id: "task-1",
    request,
    acceptanceCriteria: [{ id: "c1", description: "works", verificationKind: "deterministic" }],
  });
  runtime.createTask({
    contract,
    snapshot: createTaskRuntimeSnapshot({
      runtimeVersion: TASK_RUNTIME_VERSION,
      provider: { profileId: "fake", route: null },
      sandboxProfileId: "develop-offline",
      capabilityPolicyRevision: "policy",
      workspaceIdentity: "/tmp/w",
      godotEngineFingerprint: null,
      workflow: null,
    }),
    steps: [],
  });
  return contract;
}

function planFor(contract: TaskContract): TaskPlan {
  return createTaskPlan({
    id: "plan-1",
    taskId: contract.id,
    taskContractRevision: contract.revision,
    depth: "light",
    content: {
      objective: contract.request,
      scope: { inScope: [], outOfScope: [] },
      nonGoals: [],
      touchpoints: [],
      constraints: [],
      risks: [],
      steps: [{ id: "s1", title: "Do", expectedTouchpoints: [] }],
      validation: { checks: ["tests"] },
    },
    createdAt: 1,
  });
}

describe("executor briefing service", () => {
  it("compiles a brief for the current task with contract/manifest identity", () => {
    const runtime = makeRuntime();
    const contract = startTask(runtime, "Inspect scenes read-only.");
    const briefing = createExecutorBriefing({
      executionContract: DEFAULT_EXECUTION_CONTRACT,
      milestone: S3M8_MILESTONE_MANIFEST,
      getTaskContract: () => runtime.latestTask()?.contract() ?? null,
      getTaskSnapshot: () => runtime.latestTask()?.snapshot() ?? null,
      getCurrentPlan: () => runtime.latestTask()?.currentPlan() ?? null,
    });
    const brief = briefing.latestOrCompile();
    expect(brief).not.toBeNull();
    expect(brief?.taskId).toBe(contract.id);
    expect(brief?.executionContract.revision).toBe(1);
    expect(brief?.milestone?.id).toBe("S3M8");
    expect(briefing.fingerprint()).toMatch(/^[0-9a-f]{64}$/);
  });

  it("returns null when no task exists", () => {
    const briefing = createExecutorBriefing({
      executionContract: DEFAULT_EXECUTION_CONTRACT,
      milestone: S3M8_MILESTONE_MANIFEST,
      getTaskContract: () => null,
      getTaskSnapshot: () => null,
      getCurrentPlan: () => null,
    });
    expect(briefing.latestOrCompile()).toBeNull();
    expect(briefing.fingerprint()).toBeNull();
  });

  it("memoizes: unrelated volatile state does not rewrite the compiled brief", () => {
    const runtime = makeRuntime();
    const contract = startTask(runtime, "Inspect scenes read-only.");
    const handle = runtime.getTask(contract.id);
    const briefing = createExecutorBriefing({
      executionContract: DEFAULT_EXECUTION_CONTRACT,
      milestone: S3M8_MILESTONE_MANIFEST,
      getTaskContract: () => runtime.latestTask()?.contract() ?? null,
      getTaskSnapshot: () => runtime.latestTask()?.snapshot() ?? null,
      getCurrentPlan: () => null,
    });
    const first = briefing.latestOrCompile();
    const firstFingerprint = computeFingerprint(first!);
    // Volatile host state changes (evidence, findings) are NOT part of the
    // memo key: the compiled brief must not be rewritten by them.
    handle?.attachEvidence({
      id: "ev-1",
      kind: "workspace_read",
      source: { type: "workspace_read", paths: ["scene.ts"], revision: "rev_".padEnd(36, "a") },
    });
    handle?.setFindings([{ findingId: "f-1", severity: "high", source: "review" }]);
    const second = briefing.latestOrCompile();
    expect(computeFingerprint(second!)).toBe(firstFingerprint);
    // And the same brief object is returned (no recompilation churn).
    expect(second).toBe(first);
  });

  it("recompiles when the plan arrives and when the plan revision changes", () => {
    const runtime = makeRuntime();
    const contract = startTask(runtime, "Inspect scenes read-only.");
    const handle = runtime.getTask(contract.id);
    const briefing = createExecutorBriefing({
      executionContract: DEFAULT_EXECUTION_CONTRACT,
      milestone: S3M8_MILESTONE_MANIFEST,
      getTaskContract: () => runtime.latestTask()?.contract() ?? null,
      getTaskSnapshot: () => runtime.latestTask()?.snapshot() ?? null,
      getCurrentPlan: () => runtime.latestTask()?.currentPlan() ?? null,
    });
    const withoutPlan = briefing.latestOrCompile();
    expect(withoutPlan?.plan).toBeNull();

    const plan = planFor(contract);
    const setResult = handle?.setPlan(plan);
    expect(setResult?.status).toBe("ok");
    handle?.approvePlan(plan.id, plan.revision);
    const withPlan = briefing.latestOrCompile();
    expect(withPlan?.plan?.id).toBe("plan-1");
    expect(computeFingerprint(withoutPlan!)).not.toBe(computeFingerprint(withPlan!));

    // Same plan revision: memoized (no recompilation churn).
    const again = briefing.latestOrCompile();
    expect(computeFingerprint(again!)).toBe(computeFingerprint(withPlan!));
  });

  it("keeps the active task tied to its execution contract revision (snapshot semantics)", () => {
    const runtime = makeRuntime();
    const contract = startTask(runtime, "Inspect scenes read-only.");
    const handle = runtime.getTask(contract.id);
    const runtimeSnapshot = handle?.runtimeSnapshot() as TaskRuntimeSnapshot;
    expect(runtimeSnapshot.executionContract).toBeNull(); // sources did not carry identity

    const v2 = reviseExecutionContract(DEFAULT_EXECUTION_CONTRACT, {
      reportingRequirements: [
        ...DEFAULT_EXECUTION_CONTRACT.reportingRequirements,
        { id: "REPORT.EXTRA", requirement: "extra" },
      ],
    });
    const briefing = createExecutorBriefing({
      executionContract: v2,
      milestone: S3M8_MILESTONE_MANIFEST,
      getTaskContract: () => runtime.latestTask()?.contract() ?? null,
      getTaskSnapshot: () => runtime.latestTask()?.snapshot() ?? null,
      getCurrentPlan: () => null,
    });
    const brief = briefing.latestOrCompile();
    // The active task's snapshot stays contract-free while a NEW task
    // compiled under rev 2 would carry the newer identity:
    expect(brief?.executionContract.revision).toBe(2);
    expect(runtimeSnapshot.executionContract).toBeNull();
  });

  it("records brief identity in the task runtime snapshot when sources carry it", () => {
    const runtime = makeRuntime();
    const contract = createTaskContract({
      id: "task-2",
      request: "Inspect scenes read-only.",
      acceptanceCriteria: [{ id: "c1", description: "works", verificationKind: "deterministic" }],
    });
    runtime.createTask({
      contract,
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "fake", route: null },
        sandboxProfileId: "develop-offline",
        capabilityPolicyRevision: "policy",
        workspaceIdentity: "/tmp/w",
        godotEngineFingerprint: null,
        workflow: null,
        executionContract: { id: DEFAULT_EXECUTION_CONTRACT.id, revision: 1 },
        milestoneManifest: { id: "S3M8", version: 1 },
        executorBriefFingerprint: "fp-1",
      }),
      steps: [],
    });
    const state = runtime.latestTask()?.runtimeSnapshot();
    expect(state?.executionContract).toEqual({ id: "solaris-execution-contract", revision: 1 });
    expect(state?.milestoneManifest).toEqual({ id: "S3M8", version: 1 });
    expect(state?.executorBriefFingerprint).toBe("fp-1");
  });

  it("selects a milestone deterministically by request when a selector is provided", () => {
    const runtime = makeRuntime();
    startTask(runtime, "Add a new player ability.");
    const briefing = createExecutorBriefing({
      executionContract: DEFAULT_EXECUTION_CONTRACT,
      milestone: null,
      selectMilestone: (request) =>
        /scene|resource/i.test(request) ? S3M8_MILESTONE_MANIFEST : null,
      getTaskContract: () => runtime.latestTask()?.contract() ?? null,
      getTaskSnapshot: () => runtime.latestTask()?.snapshot() ?? null,
      getCurrentPlan: () => null,
    });
    expect(briefing.latestOrCompile()?.milestone).toBeNull();

    const sceneRuntime = makeRuntime();
    startTask(sceneRuntime, "Inspect the main scene read-only.");
    const sceneBriefing = createExecutorBriefing({
      executionContract: DEFAULT_EXECUTION_CONTRACT,
      milestone: null,
      selectMilestone: (request) =>
        /scene|resource/i.test(request) ? S3M8_MILESTONE_MANIFEST : null,
      getTaskContract: () => sceneRuntime.latestTask()?.contract() ?? null,
      getTaskSnapshot: () => sceneRuntime.latestTask()?.snapshot() ?? null,
      getCurrentPlan: () => null,
    });
    expect(sceneBriefing.latestOrCompile()?.milestone?.id).toBe("S3M8");
  });
});

import { canonicalizeJson, sha256Hex } from "../godot/digest.js";
import type { ExecutorBrief } from "./brief-compiler.js";

function computeFingerprint(brief: ExecutorBrief): string {
  return sha256Hex(canonicalizeJson(brief));
}

describe("executor briefing service — evolving context memo", () => {
  it("recompiles when the workspace scope changes under a stable plan revision", () => {
    const runtime = makeRuntime();
    startTask(runtime, "Inspect scenes read-only.");
    const options = {
      executionContract: DEFAULT_EXECUTION_CONTRACT,
      milestone: S3M8_MILESTONE_MANIFEST,
      getTaskContract: () => runtime.latestTask()?.contract() ?? null,
      getTaskSnapshot: () => runtime.latestTask()?.snapshot() ?? null,
      getCurrentPlan: () => runtime.latestTask()?.currentPlan() ?? null,
      workspaceScope: createWorkspaceScope({
        candidateFiles: [
          {
            path: "packages/core/src/godot/scene/parser.ts",
            confidence: "candidate",
            view: "none",
          },
        ],
      }),
    };
    const briefing = createExecutorBriefing(options);
    const first = briefing.latestOrCompile();
    expect(first?.workspaceVerifiedFiles).toEqual([]);
    const firstFingerprint = briefing.fingerprint();
    // Scope evolves during the task (promotion); the same plan revision
    // must NOT serve the stale memoized brief.
    const promoted = promoteCandidateFile(
      options.workspaceScope,
      "packages/core/src/godot/scene/parser.ts",
      {
        evidence: "read:packages/core/src/godot/scene/parser.ts",
        revision: "rev_".padEnd(36, "a"),
        reason: "promoted by deterministic discovery",
      },
    );
    options.workspaceScope = promoted.scope;
    const second = briefing.latestOrCompile();
    expect(second?.workspaceVerifiedFiles).toEqual(["packages/core/src/godot/scene/parser.ts"]);
    expect(briefing.fingerprint()).not.toBe(firstFingerprint);
  });
});
