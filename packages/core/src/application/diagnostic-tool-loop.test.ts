import { describe, expect, it } from "vitest";
import {
  createDefaultPolicy,
  createSolarisApplication,
  createToolRegistry,
  DEVELOP_OFFLINE_PROFILE,
  type ApprovalDecision,
  type ApprovalRequest,
  type ApprovalReviewer,
  type GodotDiagnosticPreview,
  type GodotDiagnosticToolPreparationResult,
  type ModelEvent,
  type ModelProvider,
  type PreparedDiagnosticTool,
  type PreparedGDScriptCheck,
  type ToolDefinition,
  type ToolExecutionContext,
  type ToolExecutionResult,
} from "../index.js";

function diagnosticPreview(): GodotDiagnosticPreview {
  return {
    projectName: "Fixture",
    engineVersion: "4.7.1.stable.official",
    installationId: "test-install",
    engineEdition: "standard",
    support: "compatible-untested",
    compatibility: "compatible",
    scripts: { count: 1, paths: ["src/player/player.gd"], totalBytes: 64 },
    operation: "parse-only",
    isolation: {
      sourceWorkspace: "not-used-as-project",
      disposableMirror: true,
      checkOnly: true,
      headless: true,
      sceneExecution: "disabled",
      gameExecution: "disabled",
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

function createStubDiagnosticTool(
  options: {
    prepareResult?: GodotDiagnosticToolPreparationResult;
    executeResult?: ToolExecutionResult;
  } = {},
): {
  tool: PreparedDiagnosticTool;
  prepareCalls: () => number;
  executeCalls: () => number;
  executedDigests: () => string[];
} {
  let prepareCount = 0;
  let executeCount = 0;
  const executedDigests: string[] = [];
  const definition: ToolDefinition = {
    name: "godot.check_script",
    description: "GDScript check-only diagnostics.",
    inputSchema: {},
  };
  const tool: PreparedDiagnosticTool = {
    kind: "prepared_diagnostic",
    definition,
    capability: "godot.diagnose",
    prepare(_input: unknown, _context: ToolExecutionContext) {
      prepareCount += 1;
      if (options.prepareResult !== undefined) {
        return Promise.resolve(options.prepareResult);
      }
      return Promise.resolve({
        status: "ready",
        check: {} as PreparedGDScriptCheck,
        preview: diagnosticPreview(),
        digest: "check-digest-1",
      });
    },
    executePrepared(_check: PreparedGDScriptCheck, context: ToolExecutionContext) {
      executeCount += 1;
      executedDigests.push(context.approvedDigest ?? "<missing>");
      return Promise.resolve(
        options.executeResult ?? {
          status: "success",
          output: { engineVersion: "4.7.1.stable.official", valid: true, diagnostics: [] },
          summary: "checked",
        },
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
  let index = 0;
  const requests: ApprovalRequest[] = [];
  return {
    reviewer: {
      review(request: ApprovalRequest): Promise<ApprovalDecision> {
        requests.push(request);
        const decision = decisions[Math.min(index, decisions.length - 1)] ?? {
          type: "deny",
          reason: "no decision scripted",
        };
        index += 1;
        return Promise.resolve(decision);
      },
    },
    requests: () => [...requests],
  };
}

interface ApplicationEventLike {
  readonly type?: string;
  readonly capability?: string;
  readonly decision?: string;
}

function hasEvent(
  events: readonly unknown[],
  predicate: (event: ApplicationEventLike) => boolean,
): boolean {
  return events.some((event) => predicate(event as ApplicationEventLike));
}

function toolResultEvent(events: readonly unknown[]): ApplicationEventLike | undefined {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index] as ApplicationEventLike;
    if (event?.type === "tool_completed" || event?.type === "tool_failed") {
      return event;
    }
  }
  return undefined;
}

async function runPrompt(
  provider: ModelProvider,
  reviewer: ApprovalReviewer | undefined,
  prepareResult: GodotDiagnosticToolPreparationResult | undefined,
) {
  const { tool, prepareCalls, executeCalls, executedDigests } = createStubDiagnosticTool(
    prepareResult === undefined ? {} : { prepareResult },
  );
  const registry = createToolRegistry([tool]);
  const app = createSolarisApplication({
    provider,
    tools: registry,
    policy: createDefaultPolicy("develop-offline"),
    profile: DEVELOP_OFFLINE_PROFILE,
    ...(reviewer === undefined ? {} : { reviewer }),
  });
  const events: unknown[] = [];
  for await (const event of app.sendPrompt("check the script")) {
    events.push(event);
  }
  return { events, prepareCalls, executeCalls, executedDigests, app };
}

describe("prepared diagnostic tool loop", () => {
  it("requests one-time approval and executes only with the approved digest", async () => {
    const provider = createScriptedProvider([
      [
        {
          type: "tool_call",
          callId: "call-1",
          toolName: "godot.check_script",
          input: { path: "src/player/player.gd" },
        },
        { type: "completed" },
      ],
      [{ type: "text_delta", text: "The GDScript check finished." }, { type: "completed" }],
    ]);
    const { reviewer, requests } = createScriptedReviewer([{ type: "approve_once" }]);
    const { events, executeCalls, executedDigests } = await runPrompt(
      provider,
      reviewer,
      undefined,
    );
    expect(hasEvent(events, (event) => event.type === "approval_requested")).toBe(true);
    expect(
      hasEvent(
        events,
        (event) => event.type === "approval_resolved" && event.decision === "approved",
      ),
    ).toBe(true);
    expect(requests()).toHaveLength(1);
    expect(requests()[0]?.capability).toBe("godot.diagnose");
    expect(requests()[0]?.digest).toBe("check-digest-1");
    expect(executeCalls()).toBe(1);
    expect(executedDigests()).toEqual(["check-digest-1"]);
    expect(toolResultEvent(events)?.type).toBe("tool_completed");
  });

  it("denial launches nothing", async () => {
    const provider = createScriptedProvider([
      [
        {
          type: "tool_call",
          callId: "call-1",
          toolName: "godot.check_script",
          input: { path: "src/player/player.gd" },
        },
        { type: "completed" },
      ],
      [{ type: "text_delta", text: "The GDScript check finished." }, { type: "completed" }],
    ]);
    const { reviewer } = createScriptedReviewer([{ type: "deny", reason: "user said no" }]);
    const { events, executeCalls } = await runPrompt(provider, reviewer, undefined);
    expect(hasEvent(events, (event) => event.type === "tool_failed")).toBe(true);
    expect(executeCalls()).toBe(0);
  });

  it("reviewer failure denies without launching", async () => {
    const provider = createScriptedProvider([
      [
        {
          type: "tool_call",
          callId: "call-1",
          toolName: "godot.check_script",
          input: { path: "src/player/player.gd" },
        },
        { type: "completed" },
      ],
      [{ type: "text_delta", text: "The GDScript check finished." }, { type: "completed" }],
    ]);
    const failingReviewer: ApprovalReviewer = {
      review(): Promise<ApprovalDecision> {
        return Promise.reject(new Error("reviewer crashed"));
      },
    };
    const { events, executeCalls } = await runPrompt(provider, failingReviewer, undefined);
    expect(hasEvent(events, (event) => event.type === "tool_failed")).toBe(true);
    expect(executeCalls()).toBe(0);
  });

  it("maps unavailable preparation to a typed unavailable result without requesting approval", async () => {
    const provider = createScriptedProvider([
      [
        {
          type: "tool_call",
          callId: "call-1",
          toolName: "godot.check_script",
          input: { path: "src/player/player.gd" },
        },
        { type: "completed" },
      ],
      [{ type: "text_delta", text: "The GDScript check finished." }, { type: "completed" }],
    ]);
    const { reviewer, requests } = createScriptedReviewer([{ type: "approve_once" }]);
    const { events, executeCalls } = await runPrompt(provider, reviewer, {
      status: "unavailable",
      message: "GDScript check-only diagnostics are unavailable on this platform.",
    });
    expect(hasEvent(events, (event) => event.type === "approval_requested")).toBe(false);
    expect(requests()).toHaveLength(0);
    expect(executeCalls()).toBe(0);
    expect(hasEvent(events, (event) => event.type === "tool_failed")).toBe(true);
  });

  it("never exposes an unconditional allow for godot.diagnose in user-facing profiles", () => {
    for (const profileId of ["inspect", "develop-offline", "validation-offline"] as const) {
      const policy = createDefaultPolicy(profileId);
      expect(policy.rules["godot.diagnose"]).not.toBe("allow");
    }
    expect(createDefaultPolicy("inspect").rules["godot.api"]).toBe("allow");
  });
});
