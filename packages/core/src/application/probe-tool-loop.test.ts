import { describe, expect, it } from "vitest";
import {
  createDefaultPolicy,
  createPreparedGodotProbe,
  createSolarisApplication,
  createToolRegistry,
  DEVELOP_OFFLINE_PROFILE,
  evaluatePermission,
  INSPECT_PROFILE,
  VALIDATION_OFFLINE_PROFILE,
  type ApprovalDecision,
  type ApprovalRequest,
  type ApprovalReviewer,
  type GodotProbePreview,
  type GodotProbeToolPreparationResult,
  type ModelEvent,
  type ModelProvider,
  type ModelRequest,
  type PreparedGodotProbe,
  type PreparedProjectProbeTool,
  type ToolDefinition,
  type ToolExecutionContext,
  type ToolExecutionResult,
} from "../index.js";

function samplePreview(): GodotProbePreview {
  return {
    projectName: "Fixture",
    engineVersion: "4.7.1.stable.official",
    installationId: "path-1",
    engineEdition: "standard",
    support: "verified",
    compatibility: "compatible",
    risks: {
      toolScripts: 1,
      enabledEditorPlugins: 0,
      gdextensions: 0,
      autoloads: 0,
      dotnetProjects: 0,
    },
    mirror: { estimatedFileCount: 3, estimatedBytes: 42 },
    isolation: {
      sourceWorkspace: "not-used-as-project",
      disposableMirror: true,
      recoveryMode: true,
      headless: true,
      network: "denied",
      environment: "minimal",
      stdin: "closed",
    },
    manifestDigest: "m".repeat(64),
  };
}

function createScriptedProvider(turns: readonly (readonly ModelEvent[])[]): {
  provider: ModelProvider;
  requests: ModelRequest[];
} {
  const requests: ModelRequest[] = [];
  let index = 0;
  const provider: ModelProvider = {
    id: "scripted-stub",
    async *stream(request: ModelRequest): AsyncIterable<ModelEvent> {
      requests.push(request);
      const events = turns[index] ?? [];
      index += 1;
      for (const event of events) {
        yield event;
        await Promise.resolve();
      }
    },
  };
  return { provider, requests };
}

function createScriptedReviewer(decisions: readonly ApprovalDecision[]): {
  reviewer: ApprovalReviewer;
  requests: ApprovalRequest[];
} {
  const requests: ApprovalRequest[] = [];
  let index = 0;
  return {
    reviewer: {
      review(request: ApprovalRequest): Promise<ApprovalDecision> {
        requests.push(request);
        const decision = decisions[index] ?? { type: "deny", reason: "No scripted decision." };
        index += 1;
        return Promise.resolve(decision);
      },
    },
    requests,
  };
}

function createStubProbeTool(
  options: {
    prepareResult?: GodotProbeToolPreparationResult;
    executionResult?: ToolExecutionResult;
    executionError?: Error;
  } = {},
): {
  tool: PreparedProjectProbeTool;
  executed: () => { digest: string | null; probes: PreparedGodotProbe[] }[];
  prepared: () => number;
} {
  const executions: { digest: string | null; probes: PreparedGodotProbe[] }[] = [];
  let prepareCount = 0;
  const definition: ToolDefinition = {
    name: "godot.probe_project",
    description: "Recovery-mode project probe",
    inputSchema: { type: "object", additionalProperties: false },
  };
  const tool: PreparedProjectProbeTool = {
    kind: "prepared_probe",
    definition,
    capability: "godot.probe_project",
    prepare(_input: unknown, _context: ToolExecutionContext) {
      prepareCount += 1;
      return Promise.resolve(
        options.prepareResult ?? {
          status: "ready",
          probe: createPreparedGodotProbe(),
          preview: samplePreview(),
          digest: "prepared-digest-1",
        },
      );
    },
    executePrepared(
      probe: PreparedGodotProbe,
      context: ToolExecutionContext,
    ): Promise<ToolExecutionResult> {
      executions.push({ digest: context.approvedDigest ?? null, probes: [probe] });
      if (options.executionError !== undefined) {
        return Promise.reject(options.executionError);
      }
      if (context.approvedDigest !== "prepared-digest-1") {
        return Promise.resolve({
          status: "conflict",
          message: "The prepared probe does not match the approved digest.",
        });
      }
      return Promise.resolve(
        options.executionResult ?? {
          status: "success",
          output: { status: "completed" },
          summary: "probe completed",
        },
      );
    },
  };
  return { tool, executed: () => executions, prepared: () => prepareCount };
}

function toolCall(callId: string, toolName: string, input: unknown): ModelEvent {
  return { type: "tool_call", callId, toolName, input };
}

async function collectEvents(events: AsyncIterable<unknown>): Promise<unknown[]> {
  const collected: unknown[] = [];
  for await (const event of events) {
    collected.push(event);
  }
  return collected;
}

async function runPrompt(
  application: ReturnType<typeof createSolarisApplication>,
  text: string,
): Promise<unknown[]> {
  return collectEvents(application.sendPrompt(text));
}

describe("godot.probe_project capability policy", () => {
  it("resolves to ask in the inspect profile", () => {
    const evaluation = evaluatePermission(
      "godot.probe_project",
      createDefaultPolicy("inspect"),
      INSPECT_PROFILE,
    );
    expect(evaluation.decision).toBe("ask");
  });

  it("resolves to ask in the develop-offline profile", () => {
    const evaluation = evaluatePermission(
      "godot.probe_project",
      createDefaultPolicy("develop-offline"),
      DEVELOP_OFFLINE_PROFILE,
    );
    expect(evaluation.decision).toBe("ask");
  });

  it("fails closed (deny) in the internal validation and probe profiles", () => {
    expect(
      evaluatePermission(
        "godot.probe_project",
        createDefaultPolicy("validation-offline"),
        VALIDATION_OFFLINE_PROFILE,
      ).decision,
    ).toBe("deny");
    expect(
      evaluatePermission(
        "godot.probe_project",
        createDefaultPolicy("godot-probe-offline"),
        INSPECT_PROFILE,
      ).decision,
    ).toBe("deny");
    expect(
      evaluatePermission(
        "godot.probe_project",
        createDefaultPolicy("godot-recovery-probe-offline"),
        INSPECT_PROFILE,
      ).decision,
    ).toBe("deny");
  });

  it("fails closed when the policy has no rule", () => {
    const evaluation = evaluatePermission(
      "godot.probe_project",
      { rules: {} as never },
      INSPECT_PROFILE,
    );
    expect(evaluation.decision).toBe("deny");
  });

  it("hides the tool from the provider when the capability is denied", async () => {
    const { tool } = createStubProbeTool();
    const { provider, requests } = createScriptedProvider([
      [toolCall("call-1", "godot.probe_project", {}), { type: "completed" as const }],
    ]);
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
      policy: createDefaultPolicy("validation-offline"),
      profile: VALIDATION_OFFLINE_PROFILE,
    });
    await runPrompt(application, "probe");
    const toolDefinitions = requests[0]?.tools ?? [];
    expect(toolDefinitions.some((definition) => definition.name === "godot.probe_project")).toBe(
      false,
    );
  });
});

describe("project probe approval flow", () => {
  it("asks for one-time approval and executes only after approval", async () => {
    const { tool, executed, prepared } = createStubProbeTool();
    const { reviewer, requests } = createScriptedReviewer([{ type: "approve_once" }]);
    const { provider } = createScriptedProvider([
      [toolCall("call-1", "godot.probe_project", {}), { type: "completed" as const }],
      [],
    ]);
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
    });
    const events = await runPrompt(application, "probe the godot project");
    const requested = events.find(
      (event) => (event as { type?: string }).type === "approval_requested",
    );
    expect(requested).toMatchObject({
      type: "approval_requested",
      capability: "godot.probe_project",
      toolName: "godot.probe_project",
    });
    expect(requests[0]?.capability).toBe("godot.probe_project");
    expect(requests[0]?.digest).toBe("prepared-digest-1");
    expect(prepared()).toBe(1);
    expect(executed().length).toBe(1);
    expect(executed()[0]?.digest).toBe("prepared-digest-1");
    expect(events.some((event) => (event as { type?: string }).type === "tool_completed")).toBe(
      true,
    );
  });

  it("denies without executing when the user denies", async () => {
    const { tool, executed, prepared } = createStubProbeTool();
    const { reviewer } = createScriptedReviewer([{ type: "deny", reason: "Not now." }]);
    const { provider } = createScriptedProvider([
      [toolCall("call-1", "godot.probe_project", {}), { type: "completed" as const }],
    ]);
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
    });
    const events = await runPrompt(application, "probe");
    expect(prepared()).toBe(1);
    expect(executed().length).toBe(0);
    const outcome = events.find((event) => (event as { type?: string }).type === "tool_failed") as
      { message?: string } | undefined;
    expect(outcome?.message).toBe("Not now.");
  });

  it("denies when the reviewer reports EOF-style denial", async () => {
    const { tool, executed } = createStubProbeTool();
    const { reviewer } = createScriptedReviewer([
      { type: "deny", reason: "The approval prompt was closed without an answer." },
    ]);
    const { provider } = createScriptedProvider([
      [toolCall("call-1", "godot.probe_project", {}), { type: "completed" as const }],
    ]);
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
    });
    await runPrompt(application, "probe");
    expect(executed().length).toBe(0);
  });

  it("denies when the reviewer fails", async () => {
    const { tool, executed } = createStubProbeTool();
    const reviewer: ApprovalReviewer = {
      review(): Promise<ApprovalDecision> {
        return Promise.reject(new Error("reviewer exploded"));
      },
    };
    const { provider } = createScriptedProvider([
      [toolCall("call-1", "godot.probe_project", {}), { type: "completed" as const }],
    ]);
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
    });
    const events = await runPrompt(application, "probe");
    expect(executed().length).toBe(0);
    expect(events.some((event) => (event as { type?: string }).type === "tool_failed")).toBe(true);
  });

  it("cancels without executing when the approval is cancelled", async () => {
    const { tool, executed } = createStubProbeTool();
    const { reviewer } = createScriptedReviewer([{ type: "cancelled" }]);
    const { provider } = createScriptedProvider([
      [toolCall("call-1", "godot.probe_project", {}), { type: "completed" as const }],
    ]);
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
    });
    const events = await runPrompt(application, "probe");
    expect(executed().length).toBe(0);
    expect(events.some((event) => (event as { type?: string }).type === "tool_cancelled")).toBe(
      true,
    );
  });

  it("denies without executing when no reviewer is available (provider cannot approve itself)", async () => {
    const { tool, executed } = createStubProbeTool();
    const { provider } = createScriptedProvider([
      [toolCall("call-1", "godot.probe_project", {}), { type: "completed" as const }],
    ]);
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
    });
    const events = await runPrompt(application, "probe");
    expect(executed().length).toBe(0);
    const outcome = events.find((event) => (event as { type?: string }).type === "tool_failed") as
      { message?: string } | undefined;
    expect(outcome?.message).toContain("No approval reviewer");
  });

  it("propagates a digest conflict as a conflict result", async () => {
    const { tool, executed } = createStubProbeTool({
      executionResult: {
        status: "conflict",
        message: "The project changed after approval; a new approval is required.",
      },
    });
    const { reviewer } = createScriptedReviewer([{ type: "approve_once" }]);
    const { provider } = createScriptedProvider([
      [toolCall("call-1", "godot.probe_project", {}), { type: "completed" as const }],
    ]);
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
    });
    const events = await runPrompt(application, "probe");
    expect(executed().length).toBe(1);
    const failure = events.find((event) => (event as { type?: string }).type === "tool_failed") as
      { message?: string } | undefined;
    expect(failure?.message).toContain("changed after approval");
  });

  it("requires a fresh prepare and fresh approval for every call (approval cannot be reused)", async () => {
    const { tool, prepared, executed } = createStubProbeTool();
    const { reviewer, requests } = createScriptedReviewer([
      { type: "approve_once" },
      { type: "approve_once" },
    ]);
    const { provider } = createScriptedProvider([
      [toolCall("call-1", "godot.probe_project", {}), { type: "completed" as const }],
      [],
      [toolCall("call-2", "godot.probe_project", {}), { type: "completed" as const }],
      [],
    ]);
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
    });
    await runPrompt(application, "probe one");
    await runPrompt(application, "probe two");
    expect(prepared()).toBe(2);
    expect(executed().length).toBe(2);
    expect(requests.length).toBe(2);
    expect(requests[0]?.digest).toBe("prepared-digest-1");
    expect(requests[1]?.digest).toBe("prepared-digest-1");
  });

  it("approval of the probe does not enable other capabilities", () => {
    const evaluation = evaluatePermission(
      "process.execute",
      createDefaultPolicy("develop-offline"),
      DEVELOP_OFFLINE_PROFILE,
    );
    expect(evaluation.decision).toBe("ask");
    expect(
      evaluatePermission(
        "workspace.write",
        createDefaultPolicy("develop-offline"),
        DEVELOP_OFFLINE_PROFILE,
      ).decision,
    ).toBe("ask");
  });

  it("does not approve when the input is invalid", async () => {
    const { tool, prepared } = createStubProbeTool({
      prepareResult: { status: "invalid_input", message: "The probe takes no input." },
    });
    const { reviewer } = createScriptedReviewer([{ type: "approve_once" }]);
    const { provider } = createScriptedProvider([
      [toolCall("call-1", "godot.probe_project", { path: "x" }), { type: "completed" as const }],
    ]);
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
    });
    const events = await runPrompt(application, "probe");
    expect(prepared()).toBe(1);
    expect(events.some((event) => (event as { type?: string }).type === "tool_failed")).toBe(true);
  });

  it("surfaces an execution failure truthfully", async () => {
    const { tool } = createStubProbeTool({
      executionResult: {
        status: "failed",
        message: "The Godot probe failed to start.",
      },
    });
    const { reviewer } = createScriptedReviewer([{ type: "approve_once" }]);
    const { provider } = createScriptedProvider([
      [toolCall("call-1", "godot.probe_project", {}), { type: "completed" as const }],
    ]);
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
    });
    const events = await runPrompt(application, "probe");
    const failure = events.find((event) => (event as { type?: string }).type === "tool_failed") as
      { message?: string } | undefined;
    expect(failure?.message).toBe("The Godot probe failed to start.");
  });
});
