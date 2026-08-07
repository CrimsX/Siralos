import { describe, expect, it } from "vitest";
import {
  createDefaultPolicy,
  createSolarisApplication,
  createToolRegistry,
  DEVELOP_OFFLINE_PROFILE,
  INSPECT_PROFILE,
  type ApprovalDecision,
  type ApprovalRequest,
  type ApprovalReviewer,
  type ChangePreview,
  type ModelEvent,
  type ModelProvider,
  type PreparedMutation,
  type PreparedMutationTool,
  type RegisteredTool,
  type SandboxProfile,
  type ToolDefinition,
  type ToolExecutionContext,
  type ToolExecutionResult,
  type ToolPreparationResult,
} from "../index.js";

function createScriptedProvider(turns: readonly (readonly ModelEvent[])[]): {
  provider: ModelProvider;
  requests: unknown[];
} {
  const requests: unknown[] = [];
  let index = 0;
  const provider: ModelProvider = {
    id: "scripted-stub",
    async *stream(request): AsyncIterable<ModelEvent> {
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

function createStubMutationTool(
  name: string,
  options: {
    prepare?: (input: unknown) => Promise<ToolPreparationResult>;
    applyResult?: ToolExecutionResult;
    applyError?: Error;
  } = {},
): {
  tool: PreparedMutationTool;
  applyCalls: () => number;
  prepareCalls: () => number;
} {
  let applyCount = 0;
  let prepareCount = 0;
  const definition: ToolDefinition = { name, description: `Stub ${name}`, inputSchema: {} };
  const preview: ChangePreview = {
    files: [
      {
        path: "file.txt",
        operation: "update",
        beforeSha256: "before",
        afterSha256: "after",
        addedLines: 1,
        removedLines: 1,
        unifiedDiff: "--- file.txt\n+++ file.txt\n@@\n-old\n+new\n",
      },
    ],
    totalAddedLines: 1,
    totalRemovedLines: 1,
    truncated: false,
  };
  const tool: PreparedMutationTool = {
    kind: "prepared_mutation",
    definition,
    capability: "workspace.write",
    async prepare(input: unknown, _context: ToolExecutionContext) {
      prepareCount += 1;
      if (options.prepare !== undefined) {
        return options.prepare(input);
      }
      return { status: "ready", mutation: {} as PreparedMutation, preview };
    },
    apply(): Promise<ToolExecutionResult> {
      applyCount += 1;
      if (options.applyError !== undefined) {
        return Promise.reject(options.applyError);
      }
      return Promise.resolve(
        options.applyResult ?? { status: "success", output: { ok: true }, summary: "applied" },
      );
    },
  };
  return { tool, applyCalls: () => applyCount, prepareCalls: () => prepareCount };
}

function createScriptedReviewer(decisions: readonly ApprovalDecision[]): {
  reviewer: ApprovalReviewer;
  requests: () => ApprovalRequest[];
} {
  const requests: ApprovalRequest[] = [];
  let index = 0;
  const reviewer: ApprovalReviewer = {
    review(request: ApprovalRequest): Promise<ApprovalDecision> {
      requests.push(request);
      const decision = decisions[index] ?? { type: "deny", reason: "No scripted decision." };
      index += 1;
      return Promise.resolve(decision);
    },
  };
  return { reviewer, requests: () => requests };
}

function createFailingReviewer(): ApprovalReviewer {
  return {
    review(): Promise<ApprovalDecision> {
      return Promise.reject(new Error("reviewer exploded"));
    },
  };
}

function createApplication(options: {
  tools?: readonly RegisteredTool[];
  reviewer?: ApprovalReviewer;
  profile?: SandboxProfile;
  policy?: ReturnType<typeof createDefaultPolicy>;
  turns?: readonly (readonly ModelEvent[])[];
}) {
  const { provider, requests } = createScriptedProvider(
    options.turns ?? [
      [{ type: "tool_call", callId: "c1", toolName: "write.tool", input: {} }],
      [{ type: "text_delta", text: "done" }, { type: "completed" }],
    ],
  );
  const { tool } = createStubMutationTool("write.tool");
  const application = createSolarisApplication({
    provider,
    tools: createToolRegistry(options.tools ?? [tool]),
    policy: options.policy ?? createDefaultPolicy(options.profile?.id ?? "inspect"),
    profile: options.profile ?? INSPECT_PROFILE,
    ...(options.reviewer === undefined ? {} : { reviewer: options.reviewer }),
  });
  return { application, requests };
}

async function collectEvents(events: AsyncIterable<unknown>): Promise<unknown[]> {
  const collected: unknown[] = [];
  for await (const event of events) {
    collected.push(event);
  }
  return collected;
}

describe("write tool exposure", () => {
  it("does not expose write tools to the provider under inspect", async () => {
    const { application, requests } = createApplication({
      profile: INSPECT_PROFILE,
      turns: [[{ type: "completed" }]],
    });
    await collectEvents(application.sendPrompt("hello"));
    expect(requests).toHaveLength(1);
    const request = requests[0] as { tools: readonly ToolDefinition[] };
    expect(request.tools.map((definition) => definition.name)).not.toContain("write.tool");
  });

  it("exposes write tools to the provider under develop-offline", async () => {
    const { application, requests } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      turns: [[{ type: "completed" }]],
    });
    await collectEvents(application.sendPrompt("hello"));
    const request = requests[0] as { tools: readonly ToolDefinition[] };
    expect(request.tools.map((definition) => definition.name)).toContain("write.tool");
  });

  it("denies write tools under inspect without preparing or approving", async () => {
    const { tool, prepareCalls } = createStubMutationTool("write.tool");
    const { application } = createApplication({
      profile: INSPECT_PROFILE,
      tools: [tool],
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(prepareCalls()).toBe(0);
    const failed = events.find((event) => (event as { type?: string }).type === "tool_failed");
    expect(failed).toMatchObject({ toolName: "write.tool" });
  });
});

describe("approval flow", () => {
  it("requires approval for workspace writes under develop-offline", async () => {
    const { reviewer, requests } = createScriptedReviewer([{ type: "approve_once" }]);
    const { tool } = createStubMutationTool("write.tool");
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
      tools: [tool],
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(requests()).toHaveLength(1);
    expect(requests()[0]).toMatchObject({
      capability: "workspace.write",
      toolName: "write.tool",
      paths: ["file.txt"],
    });
    const resolved = events.find(
      (event) => (event as { type?: string }).type === "approval_resolved",
    );
    expect(resolved).toMatchObject({ decision: "approved" });
  });

  it("applies the mutation exactly once after approval", async () => {
    const { reviewer } = createScriptedReviewer([{ type: "approve_once" }]);
    const { tool, applyCalls } = createStubMutationTool("write.tool");
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
      tools: [tool],
    });
    await collectEvents(application.sendPrompt("hello"));
    expect(applyCalls()).toBe(1);
  });

  it("denies without applying when the user denies", async () => {
    const { reviewer } = createScriptedReviewer([{ type: "deny", reason: "Not now." }]);
    const { tool, applyCalls } = createStubMutationTool("write.tool");
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
      tools: [tool],
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(applyCalls()).toBe(0);
    expect(events.some((event) => (event as { type?: string }).type === "tool_completed")).toBe(
      false,
    );
    const result = events.at(-2);
    expect(result).toBeDefined();
  });

  it("defaults to denial when no reviewer is configured", async () => {
    const { tool, applyCalls } = createStubMutationTool("write.tool");
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      tools: [tool],
    });
    await collectEvents(application.sendPrompt("hello"));
    expect(applyCalls()).toBe(0);
  });

  it("defaults to denial when the reviewer fails", async () => {
    const { tool, applyCalls } = createStubMutationTool("write.tool");
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer: createFailingReviewer(),
      tools: [tool],
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(applyCalls()).toBe(0);
    expect(events.some((event) => (event as { type?: string }).type === "tool_completed")).toBe(
      false,
    );
  });

  it("cancels without applying when approval is cancelled", async () => {
    const { reviewer } = createScriptedReviewer([{ type: "cancelled" }]);
    const { tool, applyCalls } = createStubMutationTool("write.tool");
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
      tools: [tool],
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(applyCalls()).toBe(0);
    expect(events.some((event) => (event as { type?: string }).type === "tool_cancelled")).toBe(
      true,
    );
  });

  it("does not apply when the preview is truncated", async () => {
    const { reviewer } = createScriptedReviewer([{ type: "approve_once" }]);
    const { tool, applyCalls } = createStubMutationTool("write.tool", {
      prepare() {
        return Promise.resolve({
          status: "ready" as const,
          mutation: {} as PreparedMutation,
          preview: {
            files: [],
            totalAddedLines: 0,
            totalRemovedLines: 0,
            truncated: true,
          },
        });
      },
    });
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
      tools: [tool],
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(applyCalls()).toBe(0);
    expect(events.some((event) => (event as { type?: string }).type === "approval_requested")).toBe(
      false,
    );
  });

  it("surfaces preparation failures without approval", async () => {
    const { reviewer, requests } = createScriptedReviewer([{ type: "approve_once" }]);
    const { tool } = createStubMutationTool("write.tool", {
      prepare() {
        return Promise.resolve({
          status: "conflict" as const,
          message: "The file changed; reread it.",
        });
      },
    });
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
      tools: [tool],
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(requests()).toHaveLength(0);
    const failed = events.find((event) => (event as { type?: string }).type === "tool_failed");
    expect(failed).toMatchObject({ message: "The file changed; reread it." });
  });

  it("reports pending approval state during review", async () => {
    const statusHolder: {
      current: { getStatus(): { readonly pendingApproval: boolean } } | undefined;
    } = { current: undefined };
    const reviewer: ApprovalReviewer = {
      review(): Promise<ApprovalDecision> {
        expect(statusHolder.current?.getStatus().pendingApproval).toBe(true);
        return Promise.resolve({ type: "approve_once" });
      },
    };
    const { tool } = createStubMutationTool("write.tool");
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
      tools: [tool],
    });
    statusHolder.current = application;
    await collectEvents(application.sendPrompt("hello"));
    expect(application.getStatus().pendingApproval).toBe(false);
  });
});
