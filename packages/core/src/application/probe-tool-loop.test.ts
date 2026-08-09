import { describe, expect, it } from "vitest";
import {
  createDefaultPolicy,
  createSolarisApplication,
  createToolRegistry,
  DEVELOP_OFFLINE_PROFILE,
  type ApprovalDecision,
  type ApprovalRequest,
  type ApprovalReviewer,
  type GodotProbePreview,
  type GodotProbeToolPreparationResult,
  type ModelEvent,
  type ModelProvider,
  type PreparedGodotProbe,
  type PreparedProjectProbeTool,
  type ToolDefinition,
  type ToolExecutionContext,
  type ToolExecutionResult,
} from "../index.js";

function probePreview(): GodotProbePreview {
  return {
    projectName: "Fixture",
    engineVersion: "4.7.1.stable.official",
    installationId: "test-install",
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
    mirror: { estimatedFileCount: 3, estimatedBytes: 99 },
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

function createScriptedProvider(turns: readonly (readonly ModelEvent[])[]): ModelProvider {
  let index = 0;
  return {
    id: "scripted-stub",
    async *stream(): AsyncIterable<ModelEvent> {
      const events = turns[index] ?? [];
      index += 1;
      for (const event of events) {
        yield event;
        await Promise.resolve();
      }
    },
  };
}

function createStubProbeTool(
  options: {
    prepareResult?: GodotProbeToolPreparationResult;
    executeResult?: ToolExecutionResult;
    prepareError?: Error;
  } = {},
): {
  tool: PreparedProjectProbeTool;
  prepareCalls: () => number;
  executeCalls: () => number;
  executedDigests: () => string[];
} {
  let prepareCount = 0;
  let executeCount = 0;
  const executedDigests: string[] = [];
  const definition: ToolDefinition = {
    name: "godot.probe_project",
    description: "Recovery-mode Godot project probe.",
    inputSchema: {},
  };
  const tool: PreparedProjectProbeTool = {
    kind: "prepared_probe",
    definition,
    capability: "godot.probe_project",
    prepare(_input: unknown, _context: ToolExecutionContext) {
      prepareCount += 1;
      if (options.prepareError !== undefined) {
        throw options.prepareError;
      }
      if (options.prepareResult !== undefined) {
        return Promise.resolve(options.prepareResult);
      }
      return Promise.resolve({
        status: "ready",
        probe: {} as PreparedGodotProbe,
        preview: probePreview(),
        digest: "probe-digest-1",
      });
    },
    executePrepared(_probe: PreparedGodotProbe, context: ToolExecutionContext) {
      executeCount += 1;
      executedDigests.push(context.approvedDigest ?? "<missing>");
      return Promise.resolve(
        options.executeResult ?? { status: "success", output: { ok: true }, summary: "probed" },
      );
    },
  };
  return {
    tool,
    prepareCalls: () => prepareCount,
    executeCalls: () => executeCount,
    executedDigests: () => [...executedDigests],
  };
}

function createScriptedReviewer(decisions: readonly ApprovalDecision[]): {
  reviewer: ApprovalReviewer;
  requests: () => ApprovalRequest[];
} {
  const requests: ApprovalRequest[] = [];
  let index = 0;
  return {
    reviewer: {
      review(request: ApprovalRequest): Promise<ApprovalDecision> {
        requests.push(request);
        const decision = decisions[index] ?? { type: "deny" };
        index += 1;
        return Promise.resolve(decision);
      },
    },
    requests: () => requests,
  };
}

interface ApplicationEventLike {
  readonly type?: string;
  readonly message?: string;
  readonly capability?: string;
  readonly summary?: string;
  readonly decision?: string;
}

function hasEvent(
  events: readonly unknown[],
  predicate: (event: ApplicationEventLike) => boolean,
): boolean {
  return events.some((event) => predicate(event as ApplicationEventLike));
}

async function runPrompt(provider: ModelProvider, reviewer: ApprovalReviewer | undefined) {
  const { tool, prepareCalls, executeCalls, executedDigests } = createStubProbeTool();
  const registry = createToolRegistry([tool]);
  const app = createSolarisApplication({
    provider,
    tools: registry,
    policy: createDefaultPolicy("develop-offline"),
    profile: DEVELOP_OFFLINE_PROFILE,
    ...(reviewer === undefined ? {} : { reviewer }),
  });
  const events: unknown[] = [];
  for await (const event of app.sendPrompt("probe the project")) {
    events.push(event);
  }
  return { events, prepareCalls, executeCalls, executedDigests, app };
}

describe("prepared probe tool loop", () => {
  it("requests one-time approval and executes with the bound digest", async () => {
    const provider = createScriptedProvider([
      [
        {
          type: "tool_call",
          callId: "call-1",
          toolName: "godot.probe_project",
          input: {},
        },
        { type: "completed" },
      ],
      [
        {
          type: "text_delta",
          text: "The recovery-mode probe finished.",
        },
        { type: "completed" },
      ],
    ]);
    const { reviewer, requests } = createScriptedReviewer([{ type: "approve_once" }]);
    const { events, executeCalls, executedDigests } = await runPrompt(provider, reviewer);
    expect(requests()).toHaveLength(1);
    const request = requests()[0] as Extract<
      ApprovalRequest,
      { capability: "godot.probe_project" }
    >;
    expect(request.capability).toBe("godot.probe_project");
    expect(request.digest).toBe("probe-digest-1");
    expect(request.preview.isolation.recoveryMode).toBe(true);
    expect(
      hasEvent(
        events,
        (event) =>
          event.type === "approval_requested" &&
          event.capability === "godot.probe_project" &&
          (event.summary ?? "").includes("recovery-mode project probe"),
      ),
    ).toBe(true);
    expect(
      hasEvent(
        events,
        (event) => event.type === "approval_resolved" && event.decision === "approved",
      ),
    ).toBe(true);
    expect(executeCalls()).toBe(1);
    expect(executedDigests()).toEqual(["probe-digest-1"]);
  });

  it("denies without executing when the reviewer denies", async () => {
    const provider = createScriptedProvider([
      [
        {
          type: "tool_call",
          callId: "call-1",
          toolName: "godot.probe_project",
          input: {},
        },
        { type: "completed" },
      ],
    ]);
    const { reviewer } = createScriptedReviewer([{ type: "deny", reason: "not now" }]);
    const { events, executeCalls } = await runPrompt(provider, reviewer);
    expect(executeCalls()).toBe(0);
    expect(
      hasEvent(
        events,
        (event) => event.type === "approval_resolved" && event.decision === "denied",
      ),
    ).toBe(true);
    expect(
      hasEvent(events, (event) => event.type === "tool_failed" && event.message === "not now"),
    ).toBe(true);
  });

  it("cancels without executing when the reviewer cancels", async () => {
    const provider = createScriptedProvider([
      [
        {
          type: "tool_call",
          callId: "call-1",
          toolName: "godot.probe_project",
          input: {},
        },
        { type: "completed" },
      ],
    ]);
    const { reviewer } = createScriptedReviewer([{ type: "cancelled" }]);
    const { events, executeCalls } = await runPrompt(provider, reviewer);
    expect(executeCalls()).toBe(0);
    expect(hasEvent(events, (event) => event.type === "tool_cancelled")).toBe(true);
  });

  it("refuses without approval when preparation reports unavailable", async () => {
    const provider = createScriptedProvider([
      [
        {
          type: "tool_call",
          callId: "call-1",
          toolName: "godot.probe_project",
          input: {},
        },
        { type: "completed" },
      ],
    ]);
    const { reviewer, requests } = createScriptedReviewer([{ type: "approve_once" }]);
    const { tool } = createStubProbeTool({
      prepareResult: {
        status: "unavailable",
        message: "Recovery-mode project probing is unavailable on this platform.",
      },
    });
    const registry = createToolRegistry([tool]);
    const app = createSolarisApplication({
      provider,
      tools: registry,
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
    });
    const events: unknown[] = [];
    for await (const event of app.sendPrompt("probe the project")) {
      events.push(event);
    }
    expect(requests()).toHaveLength(0);
    expect(
      hasEvent(
        events,
        (event) => event.type === "tool_failed" && (event.message ?? "").includes("unavailable"),
      ),
    ).toBe(true);
  });

  it("never executes outside the approval protocol when policy says ask", async () => {
    const provider = createScriptedProvider([
      [
        {
          type: "tool_call",
          callId: "call-1",
          toolName: "godot.probe_project",
          input: {},
        },
        { type: "completed" },
      ],
    ]);
    const { reviewer } = createScriptedReviewer([{ type: "deny" }]);
    const { events, executeCalls } = await runPrompt(provider, reviewer);
    expect(executeCalls()).toBe(0);
    expect(hasEvent(events, (event) => event.type === "tool_awaiting_approval")).toBe(true);
    expect(
      hasEvent(
        events,
        (event) => event.type === "tool_failed" && (event.message ?? "").includes("denied"),
      ),
    ).toBe(true);
  });

  it("denies the capability when policy denies godot.probe_project", async () => {
    const provider = createScriptedProvider([
      [
        {
          type: "tool_call",
          callId: "call-1",
          toolName: "godot.probe_project",
          input: {},
        },
        { type: "completed" },
      ],
    ]);
    const { tool, executeCalls } = createStubProbeTool();
    const registry = createToolRegistry([tool]);
    const app = createSolarisApplication({
      provider,
      tools: registry,
      policy: createDefaultPolicy("validation-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
    });
    const events: unknown[] = [];
    for await (const event of app.sendPrompt("probe the project")) {
      events.push(event);
    }
    expect(executeCalls()).toBe(0);
    expect(
      hasEvent(
        events,
        (event) =>
          event.type === "tool_failed" && (event.message ?? "").includes("denied by policy"),
      ),
    ).toBe(true);
  });
});
