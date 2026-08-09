import { afterEach, describe, expect, it } from "vitest";
import {
  createDefaultPolicy,
  createEvidenceProjector,
  createProjectionService,
  createRevisionGuard,
  createSolarisApplication,
  createTaskContract,
  createToolProjector,
  createToolRegistry,
  DEVELOP_OFFLINE_PROFILE,
  awaitCurrent,
  type ConversationItem,
  type ModelEvent,
  type ModelProvider,
  type ModelRequest,
  type ProjectionService,
  type Capability,
  type RegisteredToolInfo,
  type TaskRuntime,
  type TaskRuntimeSnapshot,
  type Tool,
  type ToolExecutionResult,
} from "@solaris/core";
import { createProviderChangeReviewer } from "@solaris/adapters";
import {
  createBehaviorLoopHarness,
  createBehaviorRuntime,
  createRecordingProvider,
  makeSnapshot,
  FIXTURE_PATH,
  type BehaviorLoopHarness,
} from "./behavior-harness.js";

/**
 * Projection behavior fixtures (Stage 3 milestone 2 §36), verified at the
 * final observable boundary: the actual fake-provider request, the actual
 * provider invocation (or its absence), and the actual task state.
 */

const CAPACITY = {
  advertisedMaximum: null,
  verifiedMaximum: null,
  workingMaximum: 32_768,
  maxOutputTokens: 4096,
};

interface TaskFixture {
  readonly runtime: TaskRuntime;
  readonly snapshot: TaskRuntimeSnapshot;
  readonly service: ProjectionService;
  readonly taskId: string;
}

/** Runtime + task + projection service wired to that task. */
function taskFixture(request = "Add a health component to the player"): TaskFixture {
  const { runtime, sources, now } = createBehaviorRuntime();
  const snapshot = makeSnapshot(sources, now);
  const contract = createTaskContract({
    id: "task-proj",
    request,
    acceptanceCriteria: [{ id: "c1", description: "done", verificationKind: "deterministic" }],
    pausePolicy: "none",
  });
  const handle = runtime.createTask({ contract, snapshot });
  handle.transitionPhase("working");
  const service = createProjectionService({
    policy: createDefaultPolicy("develop-offline"),
    profile: DEVELOP_OFFLINE_PROFILE,
    capacity: CAPACITY,
    getTaskSnapshot: () => runtime.latestTask()?.snapshot() ?? null,
    getTaskRequest: () => runtime.latestTask()?.contract().request ?? null,
  });
  return { runtime, snapshot, service, taskId: handle.taskId };
}

/** A plain executable tool for registries (not a prepared tool). */
function plainTool(
  name: string,
  capability: Capability,
  execute: () => Promise<ToolExecutionResult> = () =>
    Promise.resolve({ status: "success", output: {}, summary: `${name} executed` }),
): Tool {
  return {
    definition: { name, description: `${name} tool`, inputSchema: { type: "object" } },
    capability,
    execute,
  };
}

/** RegisteredToolInfo view of a plain tool for projector inputs. */
function toInfo(tool: Tool): RegisteredToolInfo {
  return { definition: tool.definition, capability: tool.capability ?? "workspace.read" };
}

function jsonTurn(json: string): readonly ModelEvent[] {
  return [{ type: "text_delta", text: json }, { type: "completed" }];
}

function scriptedTextProvider(
  turns: readonly (readonly ModelEvent[])[],
  observe?: (request: ModelRequest) => void,
): ModelProvider {
  let index = 0;
  return {
    id: "scripted-projection",
    async *stream(request: ModelRequest): AsyncIterable<ModelEvent> {
      observe?.(request);
      const events = turns[index] ?? [];
      index += 1;
      for (const event of events) {
        yield event;
        await Promise.resolve();
      }
    },
  };
}

describe("Behavior 1 — the stable projection is unchanged by volatile task changes", () => {
  it("new volatile evidence never alters the stable fingerprint or prefix bytes", () => {
    const fixture = taskFixture();
    const first = fixture.service.projectRequest({
      mode: "development",
      messages: [{ type: "user_message", content: "proceed" }],
      tools: [],
      providerToolCalling: true,
    });
    // The host observes a new parser result: volatile evidence appears.
    const handle = fixture.runtime.getTask(fixture.taskId);
    expect(handle).not.toBeNull();
    handle?.attachEvidence({
      id: "ev-parser-live",
      kind: "parser_result",
      source: { type: "parser", checkedFiles: 1, validFiles: 1, errors: 0 },
    });
    const second = fixture.service.projectRequest({
      mode: "development",
      messages: [{ type: "user_message", content: "proceed" }],
      tools: [],
      providerToolCalling: true,
    });
    expect(second.contextProjection.stableFingerprint).toBe(
      first.contextProjection.stableFingerprint,
    );
    expect(second.contextProjection.stableBytes).toBe(first.contextProjection.stableBytes);
    // The volatile segment changed (a new evidence record appeared).
    expect(second.contextProjection.volatileSegments.length).toBeGreaterThan(
      first.contextProjection.volatileSegments.length,
    );
    expect(second.system?.slice(0, second.contextProjection.stableBytes)).toBe(
      first.system?.slice(0, first.contextProjection.stableBytes),
    );
  });
});

describe("Behavior 2 — contextual state changes only the contextual portion", () => {
  it("changing the task phase rewrites contextual segments, not stable ones", () => {
    const fixture = taskFixture();
    const before = fixture.service.projectRequest({
      mode: "development",
      messages: [{ type: "user_message", content: "go" }],
      tools: [],
      providerToolCalling: true,
    });
    fixture.runtime.getTask(fixture.taskId)?.transitionPhase("validating");
    const after = fixture.service.projectRequest({
      mode: "development",
      messages: [{ type: "user_message", content: "go" }],
      tools: [],
      providerToolCalling: true,
    });
    expect(before.system).not.toBe(after.system);
    expect(before.system).toContain("Phase: working");
    expect(after.system).toContain("Phase: validating");
    // The stable segments are byte-identical in both serializations.
    expect(after.system?.slice(0, after.contextProjection.stableBytes)).toBe(
      before.system?.slice(0, before.contextProjection.stableBytes),
    );
  });
});

describe("Behavior 3 — hard context pressure blocks the provider call", () => {
  it("an over-budget projection never reaches the provider", async () => {
    const recorded = createRecordingProvider(scriptedTextProvider([jsonTurn("ok")]));
    const service = createProjectionService({
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      capacity: { ...CAPACITY, workingMaximum: 200 },
    });
    const application = createSolarisApplication({
      provider: recorded.provider,
      tools: createToolRegistry([]),
      projection: service,
    });
    const events: string[] = [];
    for await (const event of application.sendPrompt("x".repeat(4000))) {
      events.push(event.type);
    }
    expect(events).toContain("response_failed");
    expect(events).not.toContain("response_completed");
    expect(recorded.requests).toHaveLength(0); // provider never invoked
    // The pressure event precedes the block, so hard pressure is observable.
    expect(events.indexOf("context_pressure")).toBeLessThan(events.indexOf("response_failed"));
    expect(events.indexOf("context_pressure")).toBeGreaterThanOrEqual(0);
  });
});

describe("Behavior 4 — auto pressure reduces the projected context", () => {
  it("drops whole tool pairs and keeps the active request and task state", () => {
    const service = createProjectionService({
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      capacity: { ...CAPACITY, workingMaximum: 1000 },
      getTaskSnapshot: () => null,
      getTaskRequest: () => "Add a health component",
    });
    const items: ConversationItem[] = [{ type: "user_message", content: "original request" }];
    for (let index = 0; index < 6; index += 1) {
      items.push({
        type: "assistant_tool_call",
        callId: `call-${index}`,
        toolName: "workspace.read",
        input: { path: `file-${index}.gd` },
      });
      items.push({
        type: "tool_result",
        callId: `call-${index}`,
        toolName: "workspace.read",
        result: { status: "success", output: {}, summary: "x".repeat(400) },
      });
    }
    const projected = service.projectRequest({
      mode: "development",
      messages: items,
      tools: [],
      providerToolCalling: true,
    });
    expect(projected.pressure.state).not.toBe("hard");
    expect(projected.messages.length).toBeLessThan(items.length);
    // Task contract survives reduction.
    expect(projected.system).toContain("Add a health component");
    // Tool pairs stay whole.
    const callIds = projected.messages.filter((item) => item.type === "assistant_tool_call");
    const resultIds = projected.messages.filter((item) => item.type === "tool_result");
    expect(callIds.length).toBe(resultIds.length);
    // The active request survives.
    expect(projected.messages[0]?.type).toBe("user_message");
  });
});

describe("Behavior 5 — TaskContract survives conversational compaction", () => {
  it("the task contract request remains in the projected system after reduction", () => {
    const service = createProjectionService({
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      capacity: { ...CAPACITY, workingMaximum: 600 },
      getTaskSnapshot: () => null,
      getTaskRequest: () => "Fix the player movement bug",
    });
    const items: ConversationItem[] = [{ type: "user_message", content: "continue" }];
    for (let index = 0; index < 4; index += 1) {
      items.push({
        type: "assistant_tool_call",
        callId: `c-${index}`,
        toolName: "workspace.read",
        input: { path: "a.gd" },
      });
      items.push({
        type: "tool_result",
        callId: `c-${index}`,
        toolName: "workspace.read",
        result: { status: "success", output: {}, summary: "y".repeat(500) },
      });
    }
    const projected = service.projectRequest({
      mode: "development",
      messages: items,
      tools: [],
      providerToolCalling: true,
    });
    expect(projected.system).toContain("Fix the player movement bug");
    expect(projected.pressure.state).not.toBe("hard");
  });
});

describe("Behavior 6 — hidden tools are absent from the actual provider request", () => {
  it("review mode hides mutation tools from the request the provider receives", async () => {
    const recorded = createRecordingProvider(scriptedTextProvider([jsonTurn("ok")]));
    const service = createProjectionService({
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      capacity: CAPACITY,
    });
    const application = createSolarisApplication({
      provider: recorded.provider,
      tools: createToolRegistry([
        plainTool("workspace.read", "workspace.read"),
        plainTool("workspace.edit_file", "workspace.write"),
        plainTool("workspace.create_file", "workspace.write"),
      ]),
      projection: service,
    });
    for await (const _event of application.sendPrompt("review", undefined, { mode: "review" })) {
      // drain
    }
    expect(recorded.requests).toHaveLength(1);
    const names = (recorded.requests[0] as ModelRequest).tools.map((tool) => tool.name);
    expect(names).toContain("workspace.read");
    expect(names).not.toContain("workspace.edit_file");
    expect(names).not.toContain("workspace.create_file");
  });

  it("a provider calling a hidden tool is denied before execution (schema boundary)", async () => {
    let executed = false;
    const recorded = createRecordingProvider(
      scriptedTextProvider([
        [
          {
            type: "tool_call",
            callId: "call-hidden",
            toolName: "workspace.edit_file",
            input: { path: "a.gd" },
          },
          { type: "completed" },
        ],
      ]),
    );
    const service = createProjectionService({
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      capacity: CAPACITY,
    });
    const application = createSolarisApplication({
      provider: recorded.provider,
      tools: createToolRegistry([
        plainTool("workspace.read", "workspace.read"),
        plainTool("workspace.edit_file", "workspace.write", () => {
          executed = true;
          return Promise.resolve({ status: "success", output: {}, summary: "edited" });
        }),
      ]),
      projection: service,
    });
    const events: string[] = [];
    for await (const event of application.sendPrompt("review", undefined, { mode: "review" })) {
      events.push(event.type);
    }
    expect(executed).toBe(false);
    expect(events).toContain("tool_failed");
  });
});

describe("Behavior 7 — gated tools stay visible but runtime enforcement still applies", () => {
  it("a gated tool appears in the request but its invocation is denied without execution", async () => {
    let executed = false;
    const gatedTool = plainTool("workspace.tag_file", "workspace.write", () => {
      executed = true;
      return Promise.resolve({ status: "success", output: {}, summary: "tagged" });
    });
    const recorded = createRecordingProvider(
      scriptedTextProvider([
        [
          {
            type: "tool_call",
            callId: "call-1",
            toolName: "workspace.tag_file",
            input: { path: "a.gd" },
          },
          { type: "completed" },
        ],
      ]),
    );
    const service = createProjectionService({
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      capacity: CAPACITY,
    });
    const application = createSolarisApplication({
      provider: recorded.provider,
      tools: createToolRegistry([gatedTool]),
      projection: service,
    });
    const events: string[] = [];
    for await (const event of application.sendPrompt("tag a.gd")) {
      events.push(event.type);
    }
    // Visible (gated) in the projected request...
    expect((recorded.requests[0] as ModelRequest).tools.map((tool) => tool.name)).toContain(
      "workspace.tag_file",
    );
    // ...but the runtime denied the invocation without executing it.
    expect(executed).toBe(false);
    expect(events).toContain("tool_failed");
  });
});

describe("Behavior 8 — the read-only reviewer has no write tools in the final provider request", () => {
  it("projects the reviewer registry through review mode at the request boundary", async () => {
    const requests: ModelRequest[] = [];
    const reviewer = createProviderChangeReviewer({
      providerFactory: () =>
        scriptedTextProvider([jsonTurn('{"findings":[]}')], (request) => requests.push(request)),
      tools: createToolRegistry([
        plainTool("workspace.read", "workspace.read"),
        plainTool("workspace.edit_file", "workspace.write"),
        plainTool("workspace.delete_file", "workspace.write"),
      ]),
      toolProjector: createToolProjector({
        policy: createDefaultPolicy("develop-offline"),
        profile: DEVELOP_OFFLINE_PROFILE,
      }),
      timeoutMs: 2000,
    });
    const outcome = await reviewer.review({
      developmentId: "wf-review",
      request: "Review the change",
      engineVersion: null,
      changedPaths: [FIXTURE_PATH],
      files: [{ path: FIXTURE_PATH, unifiedDiff: "--- a/player.gd\n+++ b/player.gd" }],
      metrics: {
        filesChanged: 1,
        linesAdded: 1,
        linesRemoved: 0,
        filesCreated: 0,
        filesDeleted: 0,
        functionsTouched: 1,
      },
      evidenceSummary: [],
      repositoryGuidance: null,
      previousFindingIds: [],
      reviewRound: 1,
    });
    expect(outcome.status).toBe("completed");
    expect(requests).toHaveLength(1);
    const names = (requests[0] as ModelRequest).tools.map((tool) => tool.name);
    expect(names).toContain("workspace.read");
    expect(names).not.toContain("workspace.edit_file");
    expect(names).not.toContain("workspace.delete_file");
  });
});

describe("Behavior 9 — an unsupported non-tool model fails clearly for /develop", () => {
  it("never silently degrades a development task into a text-only session", async () => {
    const recorded = createRecordingProvider(scriptedTextProvider([jsonTurn("ok")]));
    (recorded.provider as { toolCalling?: boolean }).toolCalling = false;
    const service = createProjectionService({
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      capacity: CAPACITY,
    });
    const application = createSolarisApplication({
      provider: recorded.provider,
      tools: createToolRegistry([]),
      projection: service,
    });
    const events: string[] = [];
    for await (const event of application.sendPrompt("develop fixture", undefined, {
      mode: "development",
    })) {
      events.push(event.type);
    }
    expect(events).toContain("response_failed");
    expect(recorded.requests).toHaveLength(0);
  });
});

describe("Behaviors 10-13 — evidence projection boundaries", () => {
  it("model views differ safely from raw evidence; raw stays authoritative", () => {
    const service = createProjectionService({
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      capacity: CAPACITY,
      evidence: { secrets: ["sk-test-1234"], maxTotalBytes: 4096 },
    });
    const raw = {
      status: "success" as const,
      output: { noisy: true },
      summary:
        "\u001B[32mok\u001B[0m line\nprogress\nprogress\nprogress\nsecret sk-test-1234 inside",
    };
    const projected = service.projectToolResult({ toolName: "process.run", result: raw });
    expect(projected.status).toBe("success");
    if (projected.status === "success") {
      expect(projected.summary).not.toContain("sk-test-1234");
      expect(projected.summary).not.toContain("\u001B");
      expect(projected.summary).toContain("ok line");
      expect(projected.summary).toContain("\u00D73");
    }
    // Raw evidence is untouched.
    expect(raw.summary).toContain("sk-test-1234");
    expect(raw.summary).toContain("\u001B");
  });

  it("truncation is explicit and the projection never exceeds the source under the reduction path", () => {
    const projector = createEvidenceProjector({ maxTotalBytes: 128, secrets: ["secret-1"] });
    const view = projector.projectForModel({
      rawText: "secret-1 " + "z".repeat(5000),
    });
    expect(view.truncated).toBe(true);
    expect(view.text).toContain("[truncated]");
    expect(view.text).not.toContain("secret-1");
    expect(view.shownBytes).toBeLessThanOrEqual(128);
    expect(view.shownBytes).toBeLessThan(view.originalBytes);
  });
});

describe("Behaviors 14-15 — watermark cleanup never touches durable evidence", () => {
  it("the model-evidence cache evicts to the low watermark while task evidence survives", () => {
    const fixture = taskFixture();
    const handle = fixture.runtime.getTask(fixture.taskId);
    expect(handle).not.toBeNull();
    // Attach durable evidence records to the authoritative task.
    for (let index = 0; index < 5; index += 1) {
      handle?.attachEvidence({
        id: `ev-durable-${index}`,
        kind: "parser_result",
        source: { type: "parser", checkedFiles: 1, validFiles: 1, errors: 0 },
      });
    }
    // Project many distinct tool results so the disposable cache evicts.
    for (let index = 0; index < 80; index += 1) {
      fixture.service.projectToolResult({
        toolName: "process.run",
        result: { status: "success", output: {}, summary: `result ${index} ` + "q".repeat(50) },
      });
    }
    expect(fixture.service.evidenceCacheSize()).toBeLessThanOrEqual(64);
    // Durable task evidence is untouched by model-context eviction.
    const snapshot = fixture.runtime.getTask(fixture.taskId)?.snapshot();
    expect(snapshot?.evidence).toHaveLength(5);
    expect(snapshot?.evidence.map((entry) => entry.id)).toContain("ev-durable-4");
  });

  it("a contract revision advance invalidates the disposable evidence cache (stale guard consumer)", () => {
    const fixture = taskFixture();
    const handle = fixture.runtime.getTask(fixture.taskId);
    expect(handle).not.toBeNull();
    for (let index = 0; index < 10; index += 1) {
      fixture.service.projectToolResult({
        toolName: "process.run",
        result: { status: "success", output: {}, summary: `result ${index}` },
      });
    }
    expect(fixture.service.evidenceCacheSize()).toBeGreaterThan(0);
    // The task contract advances to revision 2.
    handle?.reviseContract({
      id: fixture.taskId,
      request: "Add a health component to the player (revised)",
    });
    // The next projection observes the new revision and clears stale views.
    fixture.service.projectToolResult({
      toolName: "process.run",
      result: { status: "success", output: {}, summary: "fresh result" },
    });
    expect(fixture.service.evidenceCacheSize()).toBe(1);
    // Durable task evidence is still intact.
    expect(fixture.runtime.getTask(fixture.taskId)?.snapshot().contractRevision).toBe(2);
  });
});

describe("Behavior 16 — stale async projection results are discarded", () => {
  it("a result completed after a revision advance never enters the newer context", async () => {
    const guard = createRevisionGuard(1);
    let resolveLate!: (value: string) => void;
    const late = new Promise<string>((resolve) => {
      resolveLate = resolve;
    });
    const awaited = awaitCurrent(guard, late);
    guard.advance(); // task state moved to revision 2 while the helper ran
    resolveLate("stale projection payload");
    expect(await awaited).toBeNull();
  });
});

describe("Behaviors 17-18 — the /develop loop still works through the projection path", () => {
  let harness: BehaviorLoopHarness;
  afterEach(async () => {
    await harness.cleanup();
  });

  it("clean success completes with projection wired (regression)", async () => {
    harness = await createBehaviorLoopHarness({ projection: true });
    await harness.startWorkflow("develop fixture");
    await harness.runPrompt("develop fixture");
    const task = await harness.finalizeTask();
    expect(task?.phase).toBe("completed");
  });

  it("failed validation still refuses completion with projection wired (regression)", async () => {
    harness = await createBehaviorLoopHarness({ projection: true });
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
    await harness.startWorkflow("develop fixture");
    await harness.runPrompt("develop fixture");
    const task = await harness.finalizeTask();
    expect(task?.phase).toBe("failed");
    expect(task?.phase).not.toBe("completed");
  });
});

describe("Behavior 19 — the reviewer still blocks evidence-backed Critical/High findings", () => {
  let harness: BehaviorLoopHarness;
  afterEach(async () => {
    await harness.cleanup();
  });

  it("a high-severity review finding leaves the task failed, never completed", async () => {
    harness = await createBehaviorLoopHarness({
      projection: true,
      reviewerScenario: "high",
    });
    await harness.startWorkflow("develop fixture");
    await harness.runPrompt("develop fixture");
    const task = await harness.finalizeTask();
    expect(task?.phase).toBe("failed");
    expect(task?.phase).not.toBe("completed");
    expect(task?.reviewStatus).toBe("findings");
  });
});

describe("Behavior 20 — projectors cannot mutate TaskState", () => {
  it("projection calls leave the authoritative task state bit-identical", () => {
    const fixture = taskFixture();
    const before = fixture.runtime.getTask(fixture.taskId)?.snapshot();
    fixture.service.projectRequest({
      mode: "development",
      messages: [{ type: "user_message", content: "go" }],
      tools: [],
      providerToolCalling: true,
    });
    fixture.service.projectToolResult({
      toolName: "process.run",
      result: { status: "success", output: {}, summary: "ok" },
    });
    expect(fixture.runtime.getTask(fixture.taskId)?.snapshot()).toEqual(before);
    // Only task_started + the host phase transition exist.
    expect(fixture.runtime.getTask(fixture.taskId)?.activityLog()).toHaveLength(2);
  });
});

describe("Tool ABI stability (§16)", () => {
  it("ordinary task progress does not change the session tool fingerprint", () => {
    const projector = createToolProjector({
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
    });
    const tools = [
      toInfo(plainTool("workspace.read", "workspace.read")),
      toInfo(plainTool("workspace.search", "workspace.read")),
      toInfo(plainTool("workspace.apply_text_changeset", "workspace.write")),
    ];
    const first = projector.project({ mode: "development", registeredTools: tools });
    const second = projector.project({ mode: "development", registeredTools: tools });
    expect(first.fingerprint).toBe(second.fingerprint);
    expect(first.requestTools.map((tool) => tool.name).sort()).toEqual(
      ["workspace.read", "workspace.search", "workspace.apply_text_changeset"].sort(),
    );
  });
});
