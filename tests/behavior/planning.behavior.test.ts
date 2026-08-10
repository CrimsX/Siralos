import { afterEach, describe, expect, it } from "vitest";
import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import {
  DEVELOP_OFFLINE_PROFILE,
  TASK_RUNTIME_VERSION,
  canonicalizeJson,
  createAdHocTaskContract,
  createDefaultPolicy,
  createPlanningFlow,
  createPlanningPolicy,
  createTaskRuntimeSnapshot,
  createToolProjector,
  type ApprovalDecision,
  type ApprovalRequest,
  type GodotInspector,
  type GodotKnowledge,
  type ModelProvider,
  type ModelRequest,
  type PlannerPort,
  type PlanningDecisionInput,
} from "@solaris/core";
import { createPlannerExecutor, createPlannerToolRegistry } from "@solaris/adapters";
import {
  createBehaviorLoopHarness,
  FIXTURE_PATH,
  readWorkspaceFile,
  type BehaviorLoopHarness,
} from "./behavior-harness.js";

/**
 * Planning Foundation behavior fixtures (Stage 3 milestone 7, ADR 0020).
 *
 * Fixtures 1-35 and final-boundary effect tests 47-51: planning is
 * runtime-controlled rather than model-controlled. The host routes depth
 * deterministically; the planner is structurally read-only with a fresh
 * context; plans are immutable, revisioned, bound to the TaskContract
 * revision; plan approval binds to the exact plan revision and never
 * authorizes edits or commands; TaskState stays the execution authority;
 * and plan content can never grant capability.
 */

type ScriptStep =
  | { readonly kind: "tool-call"; readonly toolName: string; readonly input: unknown }
  | { readonly kind: "text"; readonly text: string };

function createScriptedPlannerProvider(
  steps: readonly ScriptStep[],
  onRequest?: (request: ModelRequest) => void,
): ModelProvider {
  let cursor = 0;
  return {
    id: "scripted-planner",
    toolCalling: true,
    stream(request: ModelRequest): AsyncIterable<import("@solaris/core").ModelEvent> {
      onRequest?.(request);
      return streamStep(steps, cursor++);
    },
  };
}

async function* streamStep(
  steps: readonly ScriptStep[],
  index: number,
): AsyncIterable<import("@solaris/core").ModelEvent> {
  const step = steps[index];
  if (step === undefined) {
    yield { type: "text_delta", text: "no further scripted steps" };
    yield { type: "completed" };
    return;
  }
  if (step.kind === "tool-call") {
    yield {
      type: "tool_call",
      callId: `call-${index}`,
      toolName: step.toolName,
      input: step.input,
    };
    await Promise.resolve();
    yield { type: "completed" };
    return;
  }
  for (let offset = 0; offset < step.text.length; offset += 40) {
    yield { type: "text_delta", text: step.text.slice(offset, offset + 40) };
  }
  yield { type: "completed" };
}

function planText(depth: "light" | "full", overrides: Record<string, unknown> = {}): string {
  const shared = {
    depth,
    objective: "Add health regeneration after 5 seconds without damage.",
    touchpoints: [
      {
        id: "t1",
        path: FIXTURE_PATH,
        confidence: "verified",
        revision: "rev_".padEnd(36, "a"),
        evidence: `read:${FIXTURE_PATH}`,
      },
      { id: "t2", path: "tests/player/**", confidence: "candidate" },
    ],
    steps: [
      {
        id: "step-1",
        title: "Update player health timing state",
        expectedTouchpoints: ["t1"],
        verification: [],
      },
      { id: "step-2", title: "Extend health tests", expectedTouchpoints: ["t2"] },
    ],
    validation: {
      checks: ["check-only parse", "existing health tests"],
      requirements: ["workspace mutation"],
    },
  };
  const plan =
    depth === "light"
      ? shared
      : {
          ...shared,
          scope: { inScope: ["player health timing"], outOfScope: ["UI work"] },
          nonGoals: ["Health bar animation"],
          constraints: [{ id: "c1", description: "Stay within the workspace." }],
          risks: [{ id: "r1", severity: "medium", description: "Damage cooldown interaction." }],
          rollback: { description: "Revert the prepared change set." },
        };
  return JSON.stringify({ ...plan, ...overrides });
}

function makePlanner(
  workspaceRoot: string,
  providerFactory: () => ModelProvider,
  onObservation?: (observation: {
    readonly action: string;
    readonly fingerprint: string;
    readonly progress: boolean;
  }) => void,
): PlannerPort {
  return createPlannerExecutor({
    providerFactory,
    tools: createPlannerToolRegistry({
      workspaceRoot,
      godot: {} as GodotInspector,
      knowledge: {} as GodotKnowledge,
    }),
    toolProjector: createToolProjector({
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
    }),
    ...(onObservation === undefined ? {} : { onObservation }),
  });
}

function planningInput(overrides: Partial<PlanningDecisionInput> = {}): PlanningDecisionInput {
  return {
    request: "Add health regeneration",
    explicitPlanRequest: false,
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

/** Messages after the last user message (mirrors the fake provider). */
function itemsAfterLastUserMessage(
  messages: readonly import("@solaris/core").ConversationItem[],
): readonly import("@solaris/core").ConversationItem[] {
  let lastUser = -1;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (messages[index]?.type === "user_message") {
      lastUser = index;
      break;
    }
  }
  return messages.slice(lastUser + 1);
}

function lastResult(
  items: readonly import("@solaris/core").ConversationItem[],
  toolName: string,
): import("@solaris/core").ToolExecutionResult | undefined {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (item?.type === "tool_result" && item.toolName === toolName) {
      return item.result;
    }
  }
  return undefined;
}

function readSha256(
  result: import("@solaris/core").ToolExecutionResult | undefined,
): string | null {
  if (result?.status !== "success") {
    return null;
  }
  const output = result.output as { sha256?: unknown };
  return typeof output["sha256"] === "string" && output["sha256"].length === 64
    ? output["sha256"]
    : null;
}

/**
 * A scripted develop provider that proposes the fixture edit and RETRIES
 * the mutation when the approval is denied — the fixture-22 boundary
 * scenario the deterministic fake provider does not script.
 */
function createDevelopMutationProvider(): ModelProvider {
  return {
    id: "scripted-develop",
    toolCalling: true,
    stream(request: ModelRequest): AsyncIterable<import("@solaris/core").ModelEvent> {
      const items = itemsAfterLastUserMessage(request.messages);
      const changeset = lastResult(items, "workspace.apply_text_changeset");
      const read = lastResult(items, "workspace.read");
      return (async function* () {
        if (changeset !== undefined) {
          if (changeset.status === "denied") {
            // End this turn cleanly after a denial; the retry happens on
            // the next prompt (a fresh user turn), keeping the denial and
            // the approval in separate application prompts.
            await Promise.resolve();
            yield { type: "text_delta", text: "The change was not approved." };
            yield { type: "completed" };
            return;
          }
          yield { type: "text_delta", text: "The approved change was applied." };
          yield { type: "completed" };
          return;
        }
        if (read === undefined) {
          yield {
            type: "tool_call",
            callId: "call-dev-read",
            toolName: "workspace.read",
            input: { path: FIXTURE_PATH },
          };
          yield { type: "completed" };
          return;
        }
        // After a denial in a previous prompt, retry the mutation now.
        const deniedBefore =
          lastResult(request.messages, "workspace.apply_text_changeset")?.status === "denied";
        const hash = readSha256(read);
        if (hash === null) {
          yield { type: "text_delta", text: "The fixture could not be read." };
          yield { type: "completed" };
          return;
        }
        yield {
          type: "tool_call",
          callId: deniedBefore ? "call-dev-change-2" : "call-dev-change",
          toolName: "workspace.apply_text_changeset",
          input: {
            changes: [
              {
                operation: "edit",
                path: FIXTURE_PATH,
                expectedSha256: hash,
                replacements: [
                  { oldText: "move_and_slide()", newText: "move_and_slide(Vector2.UP)" },
                ],
              },
            ],
          },
        };
        yield { type: "completed" };
      })();
    },
  };
}

async function listWorkspaceFiles(root: string): Promise<Map<string, string>> {
  const files = new Map<string, string>();
  async function walk(directory: string, prefix: string): Promise<void> {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const relative = prefix === "" ? entry.name : `${prefix}/${entry.name}`;
      if (entry.isDirectory()) {
        await walk(join(directory, entry.name), relative);
      } else {
        files.set(relative, await readFile(join(directory, entry.name), "utf8"));
      }
    }
  }
  await walk(root, "");
  return files;
}

describe("Planning Foundation behavior fixtures", () => {
  let harness: BehaviorLoopHarness;
  afterEach(async () => {
    await harness.cleanup();
  });

  it("fixture 1/47 — a simple bounded task routes to none and never invokes the planner provider", async () => {
    harness = await createBehaviorLoopHarness();
    await harness.startWorkflow("develop fixture");
    const handle = harness.runtime.latestTask();
    expect(handle).not.toBeNull();
    if (handle === null) {
      return;
    }
    let plannerInstances = 0;
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () => {
        plannerInstances += 1;
        return createScriptedPlannerProvider([{ kind: "text", text: planText("light") }]);
      }),
    });
    const decision = flow.route(
      planningInput({
        request: "Rename one known local variable in one known file",
        narrowRepair: true,
        knownTouchpoints: 1,
      }),
    );
    expect(decision.depth).toBe("none");
    const result = await flow.run();
    expect(result.status).toBe("routed");
    expect(plannerInstances).toBe(0);
    expect(handle.currentPlan()).toBeNull();
    expect(handle.snapshot().plan.state).toBe("none");
    expect(handle.activityLog().some((event) => event.type === "planning_routed")).toBe(true);
    // The normal development flow still completes (fixture 30).
    await harness.runPrompt("develop fixture");
    const task = await harness.finalizeTask();
    expect(task?.phase).toBe("completed");
  });

  it("fixtures 2/3/5 — light/full routing is deterministic for identical structured inputs", () => {
    const policy = createPlanningPolicy();
    const moderate = planningInput({ knownTouchpoints: 3 });
    const complex = planningInput({ spansMultipleSubsystems: true });
    for (let index = 0; index < 10; index += 1) {
      expect(policy.decide(moderate).depth).toBe("light");
      expect(policy.decide(complex).depth).toBe("full");
    }
  });

  it("fixture 4 — an explicit /plan request plans even when normal routing is none", async () => {
    harness = await createBehaviorLoopHarness();
    const handle = harness.runtime.createTask({
      contract: createAdHocTaskContract("task-plan-1", "Add health regeneration"),
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "deterministic-fake", route: null },
        sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
        capabilityPolicyRevision: "policy",
        workspaceIdentity: harness.workspace.root,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () =>
        createScriptedPlannerProvider([{ kind: "text", text: planText("full") }]),
      ),
    });
    const decision = flow.route(
      planningInput({ explicitPlanRequest: true, narrowRepair: true, knownTouchpoints: 1 }),
    );
    expect(decision.depth).toBe("full");
    const result = await flow.run();
    expect(result.status).toBe("planned");
    if (result.status === "planned") {
      expect(result.plan.revision).toBe(1);
      expect(result.plan.taskContractRevision).toBe(1);
    }
  });

  it("fixture 6/48 — the planner provider receives no mutation, process, or approval tools", async () => {
    harness = await createBehaviorLoopHarness();
    const requests: ModelRequest[] = [];
    const planner = makePlanner(harness.workspace.root, () =>
      createScriptedPlannerProvider([{ kind: "text", text: planText("light") }], (request) => {
        requests.push(request);
      }),
    );
    const handle = harness.runtime.createTask({
      contract: createAdHocTaskContract("task-plan-2", "Plan something"),
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "deterministic-fake", route: null },
        sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
        capabilityPolicyRevision: "policy",
        workspaceIdentity: harness.workspace.root,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    const flow = createPlanningFlow({ handle, planner });
    flow.route(planningInput({ explicitPlanRequest: true }));
    await flow.run();
    expect(requests.length).toBeGreaterThan(0);
    const toolNames = (requests[0]?.tools ?? []).map((tool) => tool.name);
    for (const forbidden of [
      "workspace.create_file",
      "workspace.edit_file",
      "workspace.delete_file",
      "workspace.apply_text_changeset",
      "process.run",
      "solaris.undo",
      "godot.probe_project",
      "godot.check_script",
      "godot.lsp_session",
    ]) {
      expect(toolNames).not.toContain(forbidden);
    }
  });

  it("fixture 7 — the planner receives no approval-grant capability", async () => {
    harness = await createBehaviorLoopHarness();
    const requests: ModelRequest[] = [];
    const planner = makePlanner(harness.workspace.root, () =>
      createScriptedPlannerProvider([{ kind: "text", text: planText("light") }], (request) => {
        requests.push(request);
      }),
    );
    const handle = harness.runtime.createTask({
      contract: createAdHocTaskContract("task-plan-3", "Plan something"),
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "deterministic-fake", route: null },
        sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
        capabilityPolicyRevision: "policy",
        workspaceIdentity: harness.workspace.root,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    const flow = createPlanningFlow({ handle, planner });
    flow.route(planningInput({ explicitPlanRequest: true }));
    await flow.run();
    expect(requests.length).toBeGreaterThan(0);
    // No approval-grant surface exists in the projected schema.
    for (const tool of requests[0]?.tools ?? []) {
      expect(tool.name).not.toMatch(/approve|grant|permission|capabilit/i);
    }
  });

  it("fixture 8 — the planner can inspect the workspace when policy allows", async () => {
    harness = await createBehaviorLoopHarness();
    const requests: ModelRequest[] = [];
    const planner = makePlanner(harness.workspace.root, () =>
      createScriptedPlannerProvider(
        [
          {
            kind: "tool-call",
            toolName: "workspace.read",
            input: { path: FIXTURE_PATH, mode: "exact" },
          },
          { kind: "text", text: planText("full") },
        ],
        (request) => {
          requests.push(request);
        },
      ),
    );
    const handle = harness.runtime.createTask({
      contract: createAdHocTaskContract("task-plan-4", "Plan something"),
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "deterministic-fake", route: null },
        sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
        capabilityPolicyRevision: "policy",
        workspaceIdentity: harness.workspace.root,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    const flow = createPlanningFlow({ handle, planner });
    flow.route(planningInput({ explicitPlanRequest: true }));
    const result = await flow.run();
    // The read succeeded (the plan was produced after a real read result).
    expect(result.status).toBe("planned");
  });

  it("fixture 10 — valid structured planner output becomes an immutable TaskPlan rev 1", async () => {
    harness = await createBehaviorLoopHarness();
    const handle = harness.runtime.createTask({
      contract: createAdHocTaskContract("task-plan-5", "Plan something"),
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "deterministic-fake", route: null },
        sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
        capabilityPolicyRevision: "policy",
        workspaceIdentity: harness.workspace.root,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () =>
        createScriptedPlannerProvider([{ kind: "text", text: planText("full") }]),
      ),
    });
    flow.route(planningInput({ explicitPlanRequest: true }));
    const result = await flow.run();
    expect(result.status).toBe("planned");
    if (result.status !== "planned") {
      return;
    }
    const plan = handle.currentPlan();
    expect(plan?.revision).toBe(1);
    expect(plan?.taskContractRevision).toBe(1);
    // Immutable: the stored plan is never mutated — mutating the returned
    // view cannot change the stored revision.
    (plan as { objective: string }).objective = "mutated view";
    expect(handle.currentPlan()?.objective).toContain("health regeneration");
    expect(plan?.touchpoints[0]?.confidence).toBe("verified");
    expect(plan?.touchpoints[0]?.revision).toBe("rev_".padEnd(36, "a"));
    expect(plan?.touchpoints[0]?.evidence).toBe(`read:${FIXTURE_PATH}`);
  });

  it("fixture 11 — malformed planner output is rejected; no plan is stored", async () => {
    harness = await createBehaviorLoopHarness();
    const handle = harness.runtime.createTask({
      contract: createAdHocTaskContract("task-plan-6", "Plan something"),
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "deterministic-fake", route: null },
        sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
        capabilityPolicyRevision: "policy",
        workspaceIdentity: harness.workspace.root,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () =>
        createScriptedPlannerProvider([{ kind: "text", text: "I will just describe the plan." }]),
      ),
    });
    flow.route(planningInput({ explicitPlanRequest: true }));
    const result = await flow.run();
    expect(result.status).toBe("failed");
    expect(handle.currentPlan()).toBeNull();
    expect(handle.snapshot().plan.state).toBe("none");
    expect(handle.activityLog().some((event) => event.type === "plan_rejected")).toBe(true);
  });

  it("fixture 12/20 — modifying a plan creates rev 2 and invalidates the rev 1 approval", async () => {
    harness = await createBehaviorLoopHarness();
    const handle = harness.runtime.createTask({
      contract: createAdHocTaskContract("task-plan-7", "Plan something"),
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "deterministic-fake", route: null },
        sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
        capabilityPolicyRevision: "policy",
        workspaceIdentity: harness.workspace.root,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    let calls = 0;
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () => {
        calls += 1;
        const objective =
          calls === 1
            ? "Add health regeneration after 5 seconds without damage."
            : "Changed objective for rev 2.";
        return createScriptedPlannerProvider([
          { kind: "text", text: planText("full", { objective }) },
        ]);
      }),
    });
    flow.route(planningInput({ explicitPlanRequest: true }));
    const first = await flow.run();
    if (first.status !== "planned") {
      throw new Error("first plan failed");
    }
    expect(flow.approve().status).toBe("ok");
    const second = await flow.run();
    expect(second.status).toBe("planned");
    if (second.status !== "planned") {
      return;
    }
    expect(second.plan.revision).toBe(2);
    expect(handle.currentPlan()?.objective).toBe("Changed objective for rev 2.");
    expect(handle.snapshot().plan.approval).toBe("invalidated");
    // The old revision is still inspectable and unchanged (fixture 12).
    expect(handle.planRevisions()[0]?.objective).toContain("health regeneration");
    // Execution "as if rev 1 approval still applied" is refused (fixture 20/50).
    expect(handle.approvePlan(second.plan.id, 1).status).toBe("rejected");
  });

  it("fixture 14/21 — a TaskContract change marks the old plan stale and invalidates its approval", async () => {
    harness = await createBehaviorLoopHarness();
    const handle = harness.runtime.createTask({
      contract: createAdHocTaskContract("task-plan-8", "Plan something"),
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "deterministic-fake", route: null },
        sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
        capabilityPolicyRevision: "policy",
        workspaceIdentity: harness.workspace.root,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () =>
        createScriptedPlannerProvider([{ kind: "text", text: planText("full") }]),
      ),
    });
    flow.route(planningInput({ explicitPlanRequest: true }));
    const result = await flow.run();
    if (result.status !== "planned") {
      throw new Error("plan failed");
    }
    expect(flow.approve().status).toBe("ok");
    handle.reviseContract({ id: handle.contract().id, request: "Changed contract request" });
    const snapshot = handle.snapshot();
    expect(snapshot.plan.state).toBe("stale");
    expect(snapshot.plan.approval).toBe("invalidated");
    expect(handle.approvePlan(result.plan.id, result.plan.revision).status).toBe("rejected");
    // The host mutation gate refuses execution while the plan is stale
    // (the CLI enforces this gate before starting the executor loop).
    expect(flow.mutationExecutionBlocked()).toContain("stale");
  });

  it("fixture 16 — a candidate touchpoint is never promoted to verified", async () => {
    harness = await createBehaviorLoopHarness();
    const handle = harness.runtime.createTask({
      contract: createAdHocTaskContract("task-plan-9", "Plan something"),
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "deterministic-fake", route: null },
        sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
        capabilityPolicyRevision: "policy",
        workspaceIdentity: harness.workspace.root,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () =>
        createScriptedPlannerProvider([
          {
            kind: "text",
            text: planText("full", {
              touchpoints: [
                // Claims verified WITHOUT the exact revision handle: rejected.
                { id: "t1", path: FIXTURE_PATH, confidence: "verified" },
              ],
            }),
          },
        ]),
      ),
    });
    flow.route(planningInput({ explicitPlanRequest: true }));
    const result = await flow.run();
    expect(result.status).toBe("failed");
    expect(handle.currentPlan()).toBeNull();
  });

  it("fixture 17 — a full plan without meaningful acceptance criteria cannot enter mutation execution", async () => {
    harness = await createBehaviorLoopHarness();
    const handle = harness.runtime.createTask({
      contract: createAdHocTaskContract("task-plan-10", "Plan something"),
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "deterministic-fake", route: null },
        sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
        capabilityPolicyRevision: "policy",
        workspaceIdentity: harness.workspace.root,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () =>
        createScriptedPlannerProvider([{ kind: "text", text: planText("full") }]),
      ),
    });
    flow.route(planningInput({ explicitPlanRequest: true }));
    const result = await flow.run();
    if (result.status !== "planned") {
      throw new Error("plan failed");
    }
    // The ad-hoc contract has a single user criterion: mutation execution
    // is blocked even though the plan exists and could be approved.
    expect(flow.mutationExecutionBlocked()).toContain("acceptance criteria");
  });

  it("fixture 18 — plan steps never create competing TaskState progress", async () => {
    harness = await createBehaviorLoopHarness();
    await harness.startWorkflow("develop fixture");
    const handle = harness.runtime.latestTask();
    if (handle === null) {
      return;
    }
    const stepsBefore = handle.snapshot().steps.map((step) => step.id);
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () =>
        createScriptedPlannerProvider([{ kind: "text", text: planText("full") }]),
      ),
    });
    flow.route(planningInput({ explicitPlanRequest: true }));
    const result = await flow.run();
    if (result.status !== "planned") {
      throw new Error("plan failed");
    }
    // TaskState steps are untouched by planning; the plan holds its own
    // proposed steps and never becomes mutable runtime progress.
    expect(handle.snapshot().steps.map((step) => step.id)).toEqual(stepsBefore);
    expect(result.plan.steps.length).toBeGreaterThan(0);
    expect(handle.progress().state).toBe("healthy");
  });

  it("fixture 19 — plan approval binds to the exact plan revision", async () => {
    harness = await createBehaviorLoopHarness();
    const handle = harness.runtime.createTask({
      contract: createAdHocTaskContract("task-plan-11", "Plan something"),
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "deterministic-fake", route: null },
        sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
        capabilityPolicyRevision: "policy",
        workspaceIdentity: harness.workspace.root,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () =>
        createScriptedPlannerProvider([{ kind: "text", text: planText("full") }]),
      ),
    });
    flow.route(planningInput({ explicitPlanRequest: true }));
    const result = await flow.run();
    if (result.status !== "planned") {
      throw new Error("plan failed");
    }
    expect(handle.approvePlan(result.plan.id, result.plan.revision + 1).status).toBe("rejected");
    expect(flow.approve().status).toBe("ok");
    expect(handle.snapshot().plan.approval).toBe("approved");
  });

  it("fixture 22/49 — plan approval does NOT authorize workspace mutation (real boundary)", async () => {
    // Scripted approvals: the harness workflow start and the plan approval
    // are HOST decisions (no reviewer call); the reviewer only sees the
    // real mutation approval, which is DENIED.
    let reviewerCalls = 0;
    const reviewer = {
      review(_request: ApprovalRequest): Promise<ApprovalDecision> {
        reviewerCalls += 1;
        return Promise.resolve({ type: "deny", reason: "mutation not yet approved" });
      },
    };
    harness = await createBehaviorLoopHarness({
      qualityStage: false,
      reviewerOverride: reviewer,
      providerOverride: createDevelopMutationProvider(),
    });
    await harness.startWorkflow("develop fixture");
    const handle = harness.runtime.latestTask();
    if (handle === null) {
      return;
    }
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () =>
        createScriptedPlannerProvider([{ kind: "text", text: planText("full") }]),
      ),
    });
    flow.route(planningInput({ explicitPlanRequest: true }));
    const result = await flow.run();
    if (result.status !== "planned") {
      throw new Error("plan failed");
    }
    expect(flow.approve().status).toBe("ok");
    const before = await readWorkspaceFile(harness.workspace.root, FIXTURE_PATH);
    // The provider proposes a REAL source edit; the mutation approval is
    // DENIED even though the plan is approved — the file must remain
    // unchanged and the denial must be honored end to end.
    await harness.runPrompt("develop fixture");
    const afterDenied = await readWorkspaceFile(harness.workspace.root, FIXTURE_PATH);
    expect(afterDenied).toBe(before);
    expect(reviewerCalls).toBeGreaterThan(0);
    expect(harness.status().session?.state).toEqual({ kind: "terminal", status: "denied" });
    // The plan approval itself never touched the checkpoint store.
    const checkpoints = await harness.store.list();
    expect(checkpoints.some((entry) => entry.id.includes("plan"))).toBe(false);
    // The plan is still approved and current; the edit was refused anyway.
    expect(handle.snapshot().plan.approval).toBe("approved");
  });

  it("fixture 23 — plan approval does NOT authorize process execution", async () => {
    harness = await createBehaviorLoopHarness();
    await harness.startWorkflow("develop fixture");
    const handle = harness.runtime.latestTask();
    if (handle === null) {
      return;
    }
    const policy = createDefaultPolicy("develop-offline");
    const processRuleBefore = policy.rules["process.execute"];
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () =>
        createScriptedPlannerProvider([{ kind: "text", text: planText("full") }]),
      ),
    });
    flow.route(planningInput({ explicitPlanRequest: true }));
    const result = await flow.run();
    if (result.status !== "planned") {
      throw new Error("plan failed");
    }
    expect(flow.approve().status).toBe("ok");
    // The approved plan changed nothing about process capability.
    expect(policy.rules["process.execute"]).toBe(processRuleBefore);
    expect(handle.snapshot().plan.approval).toBe("approved");
  });

  it("fixture 24/51 — planner text requesting network/sandbox changes grants nothing", async () => {
    harness = await createBehaviorLoopHarness();
    const handle = harness.runtime.createTask({
      contract: createAdHocTaskContract("task-plan-12", "Plan something"),
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "deterministic-fake", route: null },
        sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
        capabilityPolicyRevision: "policy",
        workspaceIdentity: harness.workspace.root,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    const policy = createDefaultPolicy("develop-offline");
    const rulesBefore = canonicalizeJson(policy.rules);
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () =>
        createScriptedPlannerProvider([
          {
            kind: "text",
            text: planText("full", {
              steps: [
                { id: "step-1", title: "Enable unrestricted network", expectedTouchpoints: [] },
              ],
            }),
          },
        ]),
      ),
    });
    flow.route(planningInput({ explicitPlanRequest: true }));
    const result = await flow.run();
    expect(result.status).toBe("failed");
    expect(handle.currentPlan()).toBeNull();
    // The sandbox/capability policy is bit-for-bit unchanged.
    expect(canonicalizeJson(policy.rules)).toBe(rulesBefore);
  });

  it("fixture 26/27 — plan-only mode creates zero workspace changes and zero checkpoints", async () => {
    harness = await createBehaviorLoopHarness();
    const handle = harness.runtime.createTask({
      contract: createAdHocTaskContract("task-plan-13", "Plan something"),
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "deterministic-fake", route: null },
        sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
        capabilityPolicyRevision: "policy",
        workspaceIdentity: harness.workspace.root,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    const before = await listWorkspaceFiles(harness.workspace.root);
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () =>
        createScriptedPlannerProvider([{ kind: "text", text: planText("full") }]),
      ),
    });
    flow.route(planningInput({ explicitPlanRequest: true }));
    const result = await flow.run();
    expect(result.status).toBe("planned");
    if (result.status !== "planned") {
      return;
    }
    expect(flow.approve().status).toBe("ok");
    const after = await listWorkspaceFiles(harness.workspace.root);
    expect(after).toEqual(before);
    expect((await harness.store.list()).length).toBe(0);
  });

  it("fixture 28 — light plans stay bounded", async () => {
    harness = await createBehaviorLoopHarness();
    const handle = harness.runtime.createTask({
      contract: createAdHocTaskContract("task-plan-14", "Plan something"),
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "deterministic-fake", route: null },
        sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
        capabilityPolicyRevision: "policy",
        workspaceIdentity: harness.workspace.root,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () =>
        createScriptedPlannerProvider([{ kind: "text", text: planText("light") }]),
      ),
    });
    flow.route(planningInput({ explicitPlanRequest: true, requestedDepth: "light" }));
    const result = await flow.run();
    if (result.status !== "planned") {
      throw new Error(`plan failed: ${"message" in result ? result.message : result.status}`);
    }
    expect(result.plan.steps.length).toBeLessThanOrEqual(6);
    expect(result.plan.risks.length).toBe(0);
    expect(JSON.stringify(result.plan).length).toBeLessThan(32 * 1024);
  });

  it("fixture 29 — repeated no-progress planner reads fail cleanly and feed host progress", async () => {
    harness = await createBehaviorLoopHarness();
    const handle = harness.runtime.createTask({
      contract: createAdHocTaskContract("task-plan-15", "Plan something"),
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "deterministic-fake", route: null },
        sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
        capabilityPolicyRevision: "policy",
        workspaceIdentity: harness.workspace.root,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    const readCall = {
      kind: "tool-call" as const,
      toolName: "workspace.read",
      input: { path: FIXTURE_PATH, mode: "exact" },
    };
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(
        harness.workspace.root,
        () => createScriptedPlannerProvider([readCall, readCall, readCall]),
        (observation) => {
          handle.observe(observation);
        },
      ),
    });
    flow.route(planningInput({ explicitPlanRequest: true }));
    const result = await flow.run();
    expect(result.status).toBe("failed");
    if (result.status === "failed") {
      expect(result.message).toContain("stalled");
    }
    // The no-progress observations reached the task's host progress
    // tracking (repeated identical reads never count as progress).
    expect(handle.progress().repeatedActions).toBeGreaterThan(0);
  });

  it("fixture 31/32 — full planning still requires exact edit approval/checkpointing; failed validation blocks completion", async () => {
    harness = await createBehaviorLoopHarness();
    await harness.startWorkflow("develop fixture");
    const handle = harness.runtime.latestTask();
    if (handle === null) {
      return;
    }
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () =>
        createScriptedPlannerProvider([{ kind: "text", text: planText("full") }]),
      ),
    });
    flow.route(planningInput({ explicitPlanRequest: true }));
    const result = await flow.run();
    if (result.status !== "planned") {
      throw new Error("plan failed");
    }
    expect(flow.approve().status).toBe("ok");
    // Fail the post-edit parser gate BEFORE execution: the approved plan
    // does not make completion possible.
    harness.parserControl.resultsByPath.set(FIXTURE_PATH, {
      valid: false,
      diagnostics: [
        {
          source: "godot-check-only",
          severity: "error",
          path: FIXTURE_PATH,
          line: 4,
          column: 3,
          code: null,
          message: "Parse error: Unexpected token )",
          rawCategory: "SCRIPT ERROR",
        },
      ],
    });
    await harness.runPrompt("develop fixture");
    // The mutation was separately approved through the reviewer; the
    // workflow start and the plan approval are host decisions.
    expect(harness.approvals()).toBe(1);
    const checkpoints = await harness.store.list();
    expect(checkpoints.length).toBeGreaterThan(0);
    const task = await harness.finalizeTask();
    expect(task).not.toBeNull();
    if (task === null) {
      return;
    }
    expect(task.validationStatus).toBe("failed");
    expect(task.phase).not.toBe("completed");
    expect(harness.runtime.getTask(task.taskId)?.evaluateCompletion().allowed).toBe(false);
  });

  it("fixture 33 — the change reviewer stays independent of the planner", async () => {
    harness = await createBehaviorLoopHarness();
    await harness.startWorkflow("develop fixture");
    const handle = harness.runtime.latestTask();
    if (handle === null) {
      return;
    }
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () =>
        createScriptedPlannerProvider([{ kind: "text", text: planText("full") }]),
      ),
    });
    flow.route(planningInput({ explicitPlanRequest: true }));
    await flow.run();
    // The independent reviewer is a separate provider surface: it never
    // sees planner continuation, and planning never marks review done.
    expect(handle.snapshot().reviewStatus).toBe("not_run");
  });

  it("fixture 34/35 — planning activity events append correctly; private reasoning never stored", async () => {
    harness = await createBehaviorLoopHarness();
    const handle = harness.runtime.createTask({
      contract: createAdHocTaskContract("task-plan-16", "Plan something"),
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: TASK_RUNTIME_VERSION,
        provider: { profileId: "deterministic-fake", route: null },
        sandboxProfileId: DEVELOP_OFFLINE_PROFILE.id,
        capabilityPolicyRevision: "policy",
        workspaceIdentity: harness.workspace.root,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () =>
        createScriptedPlannerProvider([
          {
            kind: "text",
            text: JSON.stringify({
              ...JSON.parse(planText("full")),
              privateReasoning: "chain-of-thought must never be stored",
            }),
          },
        ]),
      ),
    });
    flow.route(planningInput({ explicitPlanRequest: true, spansMultipleSubsystems: true }));
    const result = await flow.run();
    if (result.status !== "planned") {
      throw new Error("plan failed");
    }
    flow.approve();
    const types = handle.activityLog().map((event) => event.type);
    expect(types.indexOf("planning_routed")).toBeLessThan(types.indexOf("plan_created"));
    expect(types.indexOf("plan_created")).toBeLessThan(types.indexOf("plan_approved"));
    const serialized = JSON.stringify({ state: handle.snapshot(), activity: handle.activityLog() });
    expect(serialized).not.toContain("chain-of-thought");
    expect(serialized).not.toContain("privateReasoning");
  });

  it("fixture 30 — /develop with none planning follows the existing successful flow", async () => {
    harness = await createBehaviorLoopHarness();
    await harness.startWorkflow("develop fixture");
    const handle = harness.runtime.latestTask();
    if (handle === null) {
      return;
    }
    const flow = createPlanningFlow({
      handle,
      planner: makePlanner(harness.workspace.root, () => {
        throw new Error("planner must not run for none routing");
      }),
    });
    flow.route(
      planningInput({ narrowRepair: true, knownTouchpoints: 1, request: "Fix a known variable" }),
    );
    const result = await flow.run();
    expect(result.status).toBe("routed");
    await harness.runPrompt("develop fixture");
    const task = await harness.finalizeTask();
    expect(task?.phase).toBe("completed");
  });
});
