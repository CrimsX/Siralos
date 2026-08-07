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
  applyDigests: () => string[];
  prepareCalls: () => number;
} {
  let applyCount = 0;
  let prepareCount = 0;
  const applyDigests: string[] = [];
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
      return {
        status: "ready",
        mutation: {} as PreparedMutation,
        preview,
        digest: "digest-plan-1",
      };
    },
    apply(_input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      applyCount += 1;
      applyDigests.push(context.approvedDigest ?? "<missing>");
      if (options.applyError !== undefined) {
        return Promise.reject(options.applyError);
      }
      if (context.approvedDigest !== "digest-plan-1") {
        return Promise.resolve({
          status: "denied",
          message: "The prepared plan does not match the approved plan; the mutation was denied.",
        });
      }
      return Promise.resolve(
        options.applyResult ?? { status: "success", output: { ok: true }, summary: "applied" },
      );
    },
  };
  return {
    tool,
    applyCalls: () => applyCount,
    applyDigests: () => [...applyDigests],
    prepareCalls: () => prepareCount,
  };
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
      [
        { type: "tool_call", callId: "c1", toolName: "write.tool", input: {} },
        { type: "completed" },
      ],
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
          digest: "digest-plan-1",
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

  it("binds the approval to the prepared-plan digest", async () => {
    const { reviewer, requests } = createScriptedReviewer([{ type: "approve_once" }]);
    const { tool, applyDigests } = createStubMutationTool("write.tool");
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
      tools: [tool],
    });
    await collectEvents(application.sendPrompt("hello"));
    expect(requests()[0]).toMatchObject({ digest: "digest-plan-1" });
    expect(applyDigests()).toEqual(["digest-plan-1"]);
  });
});

describe("approval fail-closed behavior", () => {
  function createOrdinaryWriteTool(
    options: {
      name?: string;
      result?: ToolExecutionResult;
      throwsOnExecute?: boolean;
    } = {},
  ): {
    tool: RegisteredTool;
    executeCalls: () => number;
  } {
    let executeCount = 0;
    const definition: ToolDefinition = {
      name: options.name ?? "plain.write",
      description: "An ordinary tool that declares workspace.write.",
      inputSchema: {},
    };
    const tool: RegisteredTool = {
      definition,
      capability: "workspace.write",
      execute(): Promise<ToolExecutionResult> {
        executeCount += 1;
        if (options.throwsOnExecute === true) {
          return Promise.reject(new Error("executed unexpectedly"));
        }
        return Promise.resolve(
          options.result ?? { status: "success", output: { ok: true }, summary: "executed" },
        );
      },
    };
    return { tool, executeCalls: () => executeCount };
  }

  it("never executes an ordinary tool whose capability requires approval", async () => {
    const { reviewer, requests } = createScriptedReviewer([{ type: "approve_once" }]);
    const { tool, executeCalls } = createOrdinaryWriteTool();
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
      tools: [tool],
      turns: [
        [
          { type: "tool_call", callId: "c1", toolName: "plain.write", input: {} },
          { type: "completed" },
        ],
        [{ type: "text_delta", text: "done" }, { type: "completed" }],
      ],
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(executeCalls()).toBe(0);
    expect(requests()).toHaveLength(0);
    const failed = events.find((event) => (event as { type?: string }).type === "tool_failed");
    expect(failed).toMatchObject({ toolName: "plain.write" });
    expect(events.some((event) => (event as { type?: string }).type === "tool_completed")).toBe(
      false,
    );
  });

  it("fails closed when an ask tool cannot produce a reviewable plan", async () => {
    const { tool, executeCalls } = createOrdinaryWriteTool();
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      tools: [tool],
      turns: [
        [
          { type: "tool_call", callId: "c1", toolName: "plain.write", input: {} },
          { type: "completed" },
        ],
        [{ type: "text_delta", text: "done" }, { type: "completed" }],
      ],
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(executeCalls()).toBe(0);
    const failed = events.find((event) => (event as { type?: string }).type === "tool_failed") as
      { type?: string; message?: string; toolName?: string } | undefined;
    expect(failed?.toolName).toBe("plain.write");
    expect(failed?.message).toContain("does not support a reviewable preparation protocol");
  });

  it("denies without execution when the reviewer's decision times out", async () => {
    const { reviewer } = createScriptedReviewer([
      { type: "deny", reason: "The approval prompt timed out; the change was denied." },
    ]);
    const { tool, applyCalls } = createStubMutationTool("write.tool");
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
      tools: [tool],
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(applyCalls()).toBe(0);
    const resolved = events.find(
      (event) => (event as { type?: string }).type === "approval_resolved",
    );
    expect(resolved).toMatchObject({ decision: "denied" });
  });

  it("denies without execution when the reviewer's input reaches EOF", async () => {
    const { reviewer } = createScriptedReviewer([
      { type: "deny", reason: "The approval prompt was closed without an answer." },
    ]);
    const { tool, applyCalls } = createStubMutationTool("write.tool");
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
      tools: [tool],
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(applyCalls()).toBe(0);
    const resolved = events.find(
      (event) => (event as { type?: string }).type === "approval_resolved",
    );
    expect(resolved).toMatchObject({ decision: "denied" });
  });

  it("denies without execution when the reviewer throws", async () => {
    const { tool, applyCalls } = createStubMutationTool("write.tool");
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer: createFailingReviewer(),
      tools: [tool],
    });
    await collectEvents(application.sendPrompt("hello"));
    expect(applyCalls()).toBe(0);
  });

  it("denies a prepared plan that changes after approval", async () => {
    const { reviewer } = createScriptedReviewer([{ type: "approve_once" }]);
    const { tool, applyCalls } = createStubMutationTool("write.tool", {
      prepare() {
        return Promise.resolve({
          status: "ready" as const,
          mutation: {} as PreparedMutation,
          preview: {
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
          },
          digest: "digest-plan-1",
        });
      },
      applyResult: {
        status: "denied",
        message: "The prepared plan does not match the approved plan; the mutation was denied.",
      },
    });
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
      tools: [tool],
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(applyCalls()).toBe(1);
    const failed = events.find((event) => (event as { type?: string }).type === "tool_failed") as
      { type?: string; message?: string } | undefined;
    expect(failed?.message).toContain("does not match");
  });

  it("refuses an attempted approval replay through a second apply", async () => {
    const { reviewer } = createScriptedReviewer([
      { type: "approve_once" },
      { type: "approve_once" },
    ]);
    const { tool, applyCalls } = createStubMutationTool("write.tool", {
      applyResult: {
        status: "failed",
        message: "The prepared mutation is not valid for this tool or has already been used.",
      },
    });
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
      tools: [tool],
      turns: [
        [
          { type: "tool_call", callId: "c1", toolName: "write.tool", input: {} },
          { type: "completed" },
        ],
        [
          { type: "tool_call", callId: "c2", toolName: "write.tool", input: {} },
          { type: "completed" },
        ],
        [{ type: "text_delta", text: "done" }, { type: "completed" }],
      ],
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(applyCalls()).toBe(2);
    const failed = events.filter((event) => (event as { type?: string }).type === "tool_failed");
    expect(failed.length).toBeGreaterThan(0);
    expect(
      failed.some((event) =>
        (event as { message?: string }).message?.includes("already been used"),
      ),
    ).toBe(true);
  });

  it("does not let the provider approve its own action", async () => {
    const { reviewer, requests } = createScriptedReviewer([{ type: "approve_once" }]);
    const { tool, applyCalls } = createStubMutationTool("write.tool");
    const { application } = createApplication({
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer,
      tools: [tool],
    });
    await collectEvents(application.sendPrompt("hello"));
    expect(requests()).toHaveLength(1);
    expect(applyCalls()).toBe(1);
    const request = requests()[0] as { capability?: string };
    expect(request.capability).toBe("workspace.write");
  });
});
