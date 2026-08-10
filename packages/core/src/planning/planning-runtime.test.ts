import { describe, expect, it } from "vitest";
import { createTaskContract, type TaskContract } from "../tasks/task-contract.js";
import { createTaskRuntime, type TaskHandle } from "../tasks/task-runtime.js";
import { createTaskRuntimeSnapshot, TASK_RUNTIME_VERSION } from "../tasks/task-snapshot.js";
import type { TaskPlanContent } from "./planning-model.js";
import { createTaskPlan } from "./planning-model.js";
import { createPlanningFlow, type PlannerPort } from "./planning-flow.js";
import type { PlanningDecisionInput } from "./planning-policy.js";

function makeContract(id = "task-plan-1"): TaskContract {
  return createTaskContract({
    id,
    request: "Add health regeneration",
    acceptanceCriteria: [
      { id: "parses", description: "Parses cleanly", verificationKind: "deterministic" },
      { id: "tests", description: "Tests pass", verificationKind: "review" },
    ],
  });
}

function makeHandle(contract: TaskContract): TaskHandle {
  const runtime = createTaskRuntime({ now: () => 1000 });
  const handle = runtime.createTask({
    contract,
    snapshot: createTaskRuntimeSnapshot({
      runtimeVersion: TASK_RUNTIME_VERSION,
      provider: { profileId: "deterministic-fake", route: null },
      sandboxProfileId: null,
      capabilityPolicyRevision: null,
      workspaceIdentity: "<test>",
      godotEngineFingerprint: null,
      workflow: null,
    }),
  });
  handle.transitionPhase("working");
  return handle;
}

function makeContent(objective = "Add health regeneration after 5 seconds."): TaskPlanContent {
  return {
    objective,
    scope: { inScope: ["player health"], outOfScope: [] },
    nonGoals: [],
    touchpoints: [],
    constraints: [],
    risks: [],
    steps: [{ id: "step-1", title: "Update health timing", expectedTouchpoints: [] }],
    validation: { checks: ["check-only parse"] },
  };
}

function planInput(overrides: Partial<PlanningDecisionInput> = {}): PlanningDecisionInput {
  return {
    request: "Add health regeneration",
    explicitPlanRequest: true,
    inspectionOnly: false,
    expectedMutation: true,
    acceptanceCriterionCount: 2,
    protectedConfigInvolved: false,
    spansMultipleSubsystems: false,
    researchRequired: false,
    capabilityUncertainty: false,
    narrowRepair: false,
    knownTouchpoints: 0,
    ...overrides,
  };
}

function fakePlanner(content: TaskPlanContent): PlannerPort {
  return {
    plan() {
      return Promise.resolve({ status: "ready", content });
    },
  };
}

describe("task runtime plan integration", () => {
  it("stores an immutable plan revision and reflects it in TaskState", async () => {
    const handle = makeHandle(makeContract());
    const flow = createPlanningFlow({ handle, planner: fakePlanner(makeContent()) });
    flow.route(planInput());
    const result = await flow.run();
    expect(result.status).toBe("planned");
    if (result.status !== "planned") {
      return;
    }
    expect(result.plan.revision).toBe(1);
    expect(result.plan.taskContractRevision).toBe(1);
    const snapshot = handle.snapshot();
    expect(snapshot.plan.planId).toBe("plan-task-plan-1");
    expect(snapshot.plan.planRevision).toBe(1);
    expect(snapshot.plan.depth).toBe("full");
    expect(snapshot.plan.state).toBe("current");
    expect(snapshot.plan.approval).toBe("none");
    expect(handle.currentPlan()?.objective).toContain("health regeneration");
    expect(handle.planRevisions().length).toBe(1);
  });

  it("rejects a plan bound to a different contract revision", () => {
    const handle = makeHandle(makeContract());
    const plan = createTaskPlan({
      id: "plan-task-plan-1",
      taskId: "task-plan-1",
      taskContractRevision: 99,
      depth: "full",
      content: makeContent(),
      createdAt: 1000,
    });
    const result = handle.setPlan(plan);
    expect(result.status).toBe("rejected");
    expect(handle.currentPlan()).toBeNull();
  });

  it("creates rev 2 rather than mutating rev 1, and invalidates the old approval (fixture 12/20)", async () => {
    const handle = makeHandle(makeContract());
    let calls = 0;
    const flow = createPlanningFlow({
      handle,
      planner: {
        plan() {
          calls += 1;
          return Promise.resolve({
            status: "ready",
            content: makeContent(
              calls === 1 ? "Add health regeneration after 5 seconds." : "changed objective",
            ),
          });
        },
      },
    });
    flow.route(planInput());
    const first = await flow.run();
    if (first.status !== "planned") {
      throw new Error("first plan failed");
    }
    expect(flow.approve().status).toBe("ok");
    expect(handle.snapshot().plan.approval).toBe("approved");
    // A non-sequential revision is rejected (plans are immutable).
    const skip = createTaskPlan({
      id: "plan-task-plan-1",
      taskId: "task-plan-1",
      taskContractRevision: 1,
      depth: "full",
      content: makeContent("skipped"),
      createdAt: 1000,
    });
    expect(handle.setPlan(skip).status).toBe("rejected");
    // Rev 2 through the flow: prior approval becomes invalidated.
    const second = await flow.run();
    expect(second.status).toBe("planned");
    if (second.status !== "planned") {
      return;
    }
    expect(second.plan.revision).toBe(2);
    expect(handle.currentPlan()?.objective).toBe("changed objective");
    expect(handle.snapshot().plan.approval).toBe("invalidated");
    expect(handle.planRevisions().length).toBe(2);
    // The old revision stays inspectable and unchanged.
    expect(handle.planRevisions()[0]?.objective).toContain("health regeneration");
  });

  it("binds approval to the exact plan revision (fixture 19)", async () => {
    const handle = makeHandle(makeContract());
    const flow = createPlanningFlow({ handle, planner: fakePlanner(makeContent()) });
    flow.route(planInput());
    const result = await flow.run();
    if (result.status !== "planned") {
      throw new Error("plan failed");
    }
    expect(handle.approvePlan("plan-task-plan-1", 2).status).toBe("rejected");
    expect(handle.approvePlan("plan-task-plan-1", 1).status).toBe("ok");
    expect(handle.snapshot().plan.approval).toBe("approved");
  });

  it("blocks full-plan mutation execution until the exact current revision is approved", async () => {
    const handle = makeHandle(makeContract());
    const flow = createPlanningFlow({ handle, planner: fakePlanner(makeContent()) });
    flow.route(planInput());
    const result = await flow.run();
    expect(result.status).toBe("planned");

    expect(flow.mutationExecutionBlocked()).toContain("approval");
    expect(flow.approve().status).toBe("ok");
    expect(flow.mutationExecutionBlocked()).toBeNull();
  });

  it("does not invoke the planner after the task becomes terminal", async () => {
    const handle = makeHandle(makeContract());
    let calls = 0;
    const flow = createPlanningFlow({
      handle,
      planner: {
        plan() {
          calls += 1;
          return Promise.resolve({ status: "ready", content: makeContent() });
        },
      },
    });
    flow.route(planInput());
    handle.cancel("user cancelled");

    const result = await flow.run();

    expect(result.status).toBe("failed");
    expect(calls).toBe(0);
    expect(handle.currentPlan()).toBeNull();
  });

  it("marks the plan stale and invalidates approval when the contract changes (fixtures 14/21)", async () => {
    const handle = makeHandle(makeContract());
    const flow = createPlanningFlow({ handle, planner: fakePlanner(makeContent()) });
    flow.route(planInput());
    const result = await flow.run();
    if (result.status !== "planned") {
      throw new Error("plan failed");
    }
    expect(flow.approve().status).toBe("ok");
    handle.reviseContract({ id: "task-plan-1", request: "Add health regeneration and mana" });
    const snapshot = handle.snapshot();
    expect(snapshot.plan.state).toBe("stale");
    expect(snapshot.plan.approval).toBe("invalidated");
    expect(snapshot.plan.staleReason).toContain("TaskContract revision");
    expect(flow.mutationExecutionBlocked()).toContain("stale");
  });

  it("records planning activity events (fixture 34)", async () => {
    const handle = makeHandle(makeContract());
    const flow = createPlanningFlow({ handle, planner: fakePlanner(makeContent()) });
    flow.route(planInput({ spansMultipleSubsystems: true }));
    const result = await flow.run();
    if (result.status !== "planned") {
      throw new Error("plan failed");
    }
    flow.approve();
    handle.invalidatePlan("host decision");
    const types = handle.activityLog().map((event) => event.type);
    expect(types).toContain("planning_routed");
    expect(types).toContain("plan_created");
    expect(types).toContain("plan_approved");
    expect(types).toContain("plan_invalidated");
  });

  it("keeps planner private reasoning out of TaskState and activity (fixture 35)", async () => {
    const handle = makeHandle(makeContract());
    const flow = createPlanningFlow({ handle, planner: fakePlanner(makeContent()) });
    flow.route(planInput());
    const result = await flow.run();
    if (result.status !== "planned") {
      throw new Error("plan failed");
    }
    const serialized = JSON.stringify({ state: handle.snapshot(), activity: handle.activityLog() });
    expect(serialized).not.toContain("chain-of-thought");
    expect(serialized).not.toContain("private reasoning");
  });

  it("rejects invalid planner output and never creates a plan (fixture 11)", async () => {
    const handle = makeHandle(makeContract());
    const badPlanner: PlannerPort = {
      plan() {
        return Promise.resolve({ status: "ready", content: { ...makeContent(), steps: [] } });
      },
    };
    const flow = createPlanningFlow({ handle, planner: badPlanner });
    flow.route(planInput());
    const result = await flow.run();
    expect(result.status).toBe("failed");
    expect(handle.currentPlan()).toBeNull();
    expect(handle.snapshot().plan.state).toBe("none");
    expect(handle.activityLog().some((event) => event.type === "plan_rejected")).toBe(true);
  });
});
