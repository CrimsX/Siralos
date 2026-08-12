import { describe, expect, it } from "vitest";
import {
  createDefaultPolicy,
  createPreparedCommand,
  createSiralosApplication,
  createToolRegistry,
  DEVELOP_OFFLINE_PROFILE,
  INSPECT_PROFILE,
  type ApplicationEvent,
  type CommandToolPreparationResult,
  type CommandPreview,
  type ModelEvent,
  type ModelProvider,
  type ModelRequest,
  type PreparedCommandTool,
  type SandboxProfile,
  type ToolExecutionResult,
} from "../index.js";
import { createScriptedProvider, toolCall, collectEvents } from "./test-support.js";

interface StubCommandToolOptions {
  readonly onOutputs?: readonly { readonly stream: "stdout" | "stderr"; readonly text: string }[];
  readonly result?: ToolExecutionResult;
  readonly prepareError?: string;
  readonly alwaysConflict?: string;
  readonly emitDelayMs?: number;
}

function createStubCommandTool(options: StubCommandToolOptions = {}): {
  tool: PreparedCommandTool;
  prepared: unknown[];
  executed: readonly { approvedDigest: string; hasOutput: boolean }[];
} {
  const prepared: unknown[] = [];
  const executed: { approvedDigest: string; hasOutput: boolean }[] = [];
  const preview: CommandPreview = {
    runnerId: "npm-script",
    displayName: "npm run check",
    packageName: "siralos",
    scriptName: "check",
    workingDirectory: ".",
    executableIdentity: "node v26.1.0 + npm 11.13.0",
    arguments: ["run", "check", "--"],
    repositoryScript: "npm run format:check && npm run lint",
    timeoutMs: 120_000,
    stdoutLimitBytes: 1_048_576,
    stderrLimitBytes: 1_048_576,
    workspaceAccess: "read-only",
    networkAccess: "denied",
    environmentPolicy: "minimal",
    stdinPolicy: "closed",
    scriptShellNotice:
      "npm executes this repository-defined script through its platform script shell.",
    hooksNotice: "Automatically associated precheck/postcheck scripts are disabled.",
  };
  const tool: PreparedCommandTool = {
    kind: "prepared_command",
    definition: {
      name: "process.run",
      description: "Run a validated Siralos development command in the sandbox.",
      inputSchema: { type: "object", properties: {}, required: [] },
    },
    capability: "process.execute",
    async prepare(input, context): Promise<CommandToolPreparationResult> {
      prepared.push(input);
      await Promise.resolve();
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "Preparation was cancelled." };
      }
      if (options.prepareError !== undefined) {
        return { status: "failed", message: options.prepareError };
      }
      return {
        status: "ready",
        command: createPreparedCommand(),
        preview,
        digest: "digest-abc123",
        commandId: "cmd-1",
      };
    },
    async executePrepared(_command, context): Promise<ToolExecutionResult> {
      executed.push({
        approvedDigest: context.approvedDigest,
        hasOutput: context.onOutput !== undefined,
      });
      if (options.alwaysConflict !== undefined) {
        return { status: "conflict", message: options.alwaysConflict };
      }
      const delays = options.onOutputs ?? [];
      for (const entry of delays) {
        await new Promise<void>((resolve) => {
          setTimeout(resolve, options.emitDelayMs ?? 0);
        });
        context.onOutput?.({ type: entry.stream, text: entry.text });
      }
      return (
        options.result ?? {
          status: "success",
          output: {
            status: "completed",
            exitCode: 0,
            stdout: "",
            stderr: "",
            stdoutTruncated: false,
            stderrTruncated: false,
            durationMs: 42,
            runnerId: "npm-script",
            commandDigest: "digest-abc123",
          },
          summary: "Completed npm run check (exit 0).",
        }
      );
    },
  };
  return { tool, prepared, executed };
}

function scriptedApplication(
  turns: readonly (readonly ModelEvent[])[],
  tool: PreparedCommandTool,
  options: {
    reviewer?: (
      request: unknown,
    ) => Promise<{ type: "approve_once" | "deny" | "cancelled"; reason?: string }>;
    profile?: SandboxProfile;
  } = {},
) {
  const { provider } = createScriptedProvider(turns);
  const reviewer = options.reviewer;
  return createSiralosApplication({
    provider,
    tools: createToolRegistry([tool]),
    policy: createDefaultPolicy(options.profile?.id ?? "develop-offline"),
    profile: options.profile ?? DEVELOP_OFFLINE_PROFILE,
    ...(reviewer === undefined
      ? {}
      : { reviewer: { review: (request: unknown) => reviewer(request) } }),
  });
}

describe("prepared command tool flow", () => {
  it("sends process.run only when policy resolves to ask", async () => {
    const { tool } = createStubCommandTool();
    const requests: ModelRequest[] = [];
    const recordingProvider: ModelProvider = {
      id: "recording-stub",
      async *stream(request: ModelRequest): AsyncIterable<ModelEvent> {
        requests.push(request);
        await Promise.resolve();
        yield { type: "completed" };
      },
    };
    const hidden = createSiralosApplication({
      provider: recordingProvider,
      tools: createToolRegistry([tool]),
      policy: createDefaultPolicy("inspect"),
      profile: INSPECT_PROFILE,
    });
    await collectEvents(hidden.sendPrompt("hello"));
    expect(requests[0]?.tools.map((definition) => definition.name)).not.toContain("process.run");
    requests.length = 0;
    const visible = createSiralosApplication({
      provider: recordingProvider,
      tools: createToolRegistry([tool]),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
    });
    await collectEvents(visible.sendPrompt("hello"));
    expect(requests[0]?.tools.map((definition) => definition.name)).toContain("process.run");
  });

  it("asks for approval and executes exactly once after approval", async () => {
    const { tool, executed } = createStubCommandTool();
    let approved = false;
    const application = scriptedApplication(
      [
        [
          toolCall("c1", "process.run", { runner: "npm-script", script: "check" }),
          { type: "completed" },
        ],
        [{ type: "completed" }],
      ],
      tool,
      {
        reviewer: () => {
          approved = true;
          return Promise.resolve({ type: "approve_once" });
        },
      },
    );
    const events = await collectEvents(application.sendPrompt("run npm check"));
    expect(approved).toBe(true);
    expect(executed).toHaveLength(1);
    expect(executed[0]).toEqual({ approvedDigest: "digest-abc123", hasOutput: true });
    const approval = events.find((event) => event.type === "approval_requested");
    expect(approval).toMatchObject({
      type: "approval_requested",
      capability: "process.execute",
      toolName: "process.run",
      summary: "npm run check",
    });
    const started = events.find((event) => event.type === "command_started");
    expect(started).toMatchObject({
      type: "command_started",
      commandId: "cmd-1",
      runnerId: "npm-script",
      displayName: "npm run check",
      digestPrefix: "digest-a",
    });
    expect(events.some((event) => event.type === "command_completed")).toBe(true);
    expect(application.getLastCommandExitCode()).toBe(0);
    expect(application.getCommandHistory()).toHaveLength(1);
    expect(application.getCommandHistory()[0]).toMatchObject({
      commandId: "cmd-1",
      runnerId: "npm-script",
      outcome: "completed",
      exitCode: 0,
    });
  });

  it("denies without starting a process", async () => {
    const { tool, executed } = createStubCommandTool();
    const application = scriptedApplication(
      [
        [
          toolCall("c1", "process.run", { runner: "npm-script", script: "check" }),
          { type: "completed" },
        ],
        [{ type: "completed" }],
      ],
      tool,
      { reviewer: () => Promise.resolve({ type: "deny", reason: "Not now." }) },
    );
    const events = await collectEvents(application.sendPrompt("run npm check"));
    expect(executed).toHaveLength(0);
    expect(events.some((event) => event.type === "command_denied")).toBe(true);
    const failed = events.find((event) => event.type === "tool_failed");
    expect(failed).toMatchObject({ message: "Not now." });
    expect(application.getCommandHistory()).toHaveLength(0);
  });

  it("denies when the reviewer fails", async () => {
    const { tool, executed } = createStubCommandTool();
    const application = scriptedApplication(
      [[toolCall("c1", "process.run", {}), { type: "completed" }], [{ type: "completed" }]],
      tool,
      {
        reviewer: () => Promise.reject(new Error("reviewer exploded")),
      },
    );
    const events = await collectEvents(application.sendPrompt("run npm check"));
    expect(executed).toHaveLength(0);
    expect(events.some((event) => event.type === "command_denied")).toBe(true);
  });

  it("denies when no reviewer is available", async () => {
    const { tool, executed } = createStubCommandTool();
    const { provider } = createScriptedProvider([
      [toolCall("c1", "process.run", {}), { type: "completed" }],
    ]);
    const application = createSiralosApplication({
      provider,
      tools: createToolRegistry([tool]),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
    });
    const events = await collectEvents(application.sendPrompt("run npm check"));
    expect(executed).toHaveLength(0);
    const denied = events.find((event) => event.type === "command_denied");
    expect(denied).toMatchObject({ message: "No approval reviewer is available." });
  });

  it("cancels while awaiting approval without a successful response", async () => {
    const controller = new AbortController();
    const { tool, executed } = createStubCommandTool();
    const application = scriptedApplication(
      [[toolCall("c1", "process.run", {}), { type: "completed" }]],
      tool,
      {
        reviewer: async () => {
          await new Promise<void>((resolve) => {
            setTimeout(resolve, 10);
          });
          if (controller.signal.aborted) {
            return { type: "cancelled" };
          }
          return { type: "approve_once" };
        },
      },
    );
    const events: ApplicationEvent[] = [];
    const completion = (async () => {
      for await (const event of application.sendPrompt("run npm check", controller.signal)) {
        events.push(event);
        if (event.type === "tool_awaiting_approval") {
          controller.abort();
        }
      }
    })();
    await completion;
    expect(executed).toHaveLength(0);
    expect(events.some((event) => event.type === "command_cancelled")).toBe(true);
    expect(events.some((event) => event.type === "response_cancelled")).toBe(true);
    expect(events.some((event) => event.type === "response_completed")).toBe(false);
  });

  it("streams bounded output and completes with a nonzero exit", async () => {
    const { tool } = createStubCommandTool({
      onOutputs: [
        { stream: "stdout", text: "Checking formatting..." },
        { stream: "stderr", text: "warning: something" },
      ],
      result: {
        status: "success",
        output: {
          status: "completed",
          exitCode: 2,
          stdout: "Checking formatting...",
          stderr: "warning: something",
          stdoutTruncated: false,
          stderrTruncated: false,
          durationMs: 1800,
          runnerId: "npm-script",
          commandDigest: "digest-abc123",
        },
        summary: "Completed npm run check (exit 2).",
      },
    });
    const application = scriptedApplication(
      [[toolCall("c1", "process.run", {}), { type: "completed" }], [{ type: "completed" }]],
      tool,
      { reviewer: () => Promise.resolve({ type: "approve_once" }) },
    );
    const events = await collectEvents(application.sendPrompt("run npm check"));
    const stdout = events.filter((event) => event.type === "command_stdout");
    expect(stdout.map((event) => event.text)).toEqual(["Checking formatting..."]);
    const stderr = events.filter((event) => event.type === "command_stderr");
    expect(stderr.map((event) => event.text)).toEqual(["warning: something"]);
    const completed = events.find((event) => event.type === "command_completed");
    expect(completed).toMatchObject({ exitCode: 2, durationMs: 1800 });
    expect(application.getLastCommandExitCode()).toBe(2);
  });

  it("reports timed-out commands truthfully without success", async () => {
    const { tool } = createStubCommandTool({
      result: { status: "timed_out", message: "The command timed out after 2 seconds." },
    });
    const application = scriptedApplication(
      [[toolCall("c1", "process.run", {}), { type: "completed" }], [{ type: "completed" }]],
      tool,
      { reviewer: () => Promise.resolve({ type: "approve_once" }) },
    );
    const events = await collectEvents(application.sendPrompt("run npm check"));
    expect(events.some((event) => event.type === "command_timed_out")).toBe(true);
    expect(events.some((event) => event.type === "command_completed")).toBe(false);
    expect(events.some((event) => event.type === "response_completed")).toBe(true);
    expect(application.getCommandHistory()[0]?.outcome).toBe("timed_out");
  });

  it("reports a changed plan as a conflict under the old approval", async () => {
    const { tool } = createStubCommandTool({
      alwaysConflict: "The package.json changed after approval.",
    });
    const application = scriptedApplication(
      [[toolCall("c1", "process.run", {}), { type: "completed" }], [{ type: "completed" }]],
      tool,
      { reviewer: () => Promise.resolve({ type: "approve_once" }) },
    );
    const events = await collectEvents(application.sendPrompt("run npm check"));
    expect(events.some((event) => event.type === "command_conflict")).toBe(true);
    const failed = events.find((event) => event.type === "tool_failed");
    expect(failed).toMatchObject({ message: "The package.json changed after approval." });
  });

  it("records sandbox-denied outcomes without success", async () => {
    const { tool } = createStubCommandTool({
      result: { status: "sandbox_denied", message: "The sandbox denied a workspace write." },
    });
    const application = scriptedApplication(
      [[toolCall("c1", "process.run", {}), { type: "completed" }], [{ type: "completed" }]],
      tool,
      { reviewer: () => Promise.resolve({ type: "approve_once" }) },
    );
    const events = await collectEvents(application.sendPrompt("run npm check"));
    const failed = events.find((event) => event.type === "command_failed");
    expect(failed).toMatchObject({ message: "The sandbox denied a workspace write." });
    expect(events.some((event) => event.type === "command_completed")).toBe(false);
    expect(application.getCommandHistory()[0]?.outcome).toBe("sandbox_denied");
  });

  it("cancels an in-flight command and stores no successful response", async () => {
    const controller = new AbortController();
    const { tool } = createStubCommandTool({
      onOutputs: [{ stream: "stdout", text: "working..." }],
      emitDelayMs: 1,
      result: { status: "cancelled", message: "The command was cancelled." },
    });
    const application = scriptedApplication(
      [[toolCall("c1", "process.run", {}), { type: "completed" }]],
      tool,
      {
        reviewer: () => Promise.resolve({ type: "approve_once" }),
      },
    );
    const events: ApplicationEvent[] = [];
    const completion = (async () => {
      for await (const event of application.sendPrompt("run npm check", controller.signal)) {
        events.push(event);
        if (event.type === "command_started") {
          controller.abort();
        }
      }
    })();
    await completion;
    expect(events.some((event) => event.type === "command_cancelled")).toBe(true);
    expect(events.some((event) => event.type === "response_cancelled")).toBe(true);
    expect(events.some((event) => event.type === "response_completed")).toBe(false);
    expect(application.getCommandHistory()[0]?.outcome).toBe("cancelled");
  });

  it("keeps the session status truthful during execution", async () => {
    const { tool } = createStubCommandTool({
      onOutputs: [{ stream: "stdout", text: "x" }],
      emitDelayMs: 5,
    });
    let seenActive = false;
    const application = scriptedApplication(
      [[toolCall("c1", "process.run", {}), { type: "completed" }], [{ type: "completed" }]],
      tool,
      { reviewer: () => Promise.resolve({ type: "approve_once" }) },
    );
    const completion = (async () => {
      for await (const event of application.sendPrompt("run npm check")) {
        if (event.type === "command_stdout") {
          seenActive = application.getStatus().activeCommandId === "cmd-1";
        }
      }
    })();
    await completion;
    expect(seenActive).toBe(true);
    expect(application.getStatus().activeCommandId).toBeNull();
  });

  it("includes the exact preview in the approval request", async () => {
    const { tool } = createStubCommandTool();
    let seenPreview: unknown;
    const application = scriptedApplication(
      [[toolCall("c1", "process.run", {}), { type: "completed" }], [{ type: "completed" }]],
      tool,
      {
        reviewer: (request) => {
          seenPreview = request;
          return Promise.resolve({ type: "approve_once" });
        },
      },
    );
    await collectEvents(application.sendPrompt("run npm check"));
    const preview = (seenPreview as { preview?: CommandPreview })?.preview;
    expect(preview).toMatchObject({
      runnerId: "npm-script",
      displayName: "npm run check",
      repositoryScript: "npm run format:check && npm run lint",
      workspaceAccess: "read-only",
      networkAccess: "denied",
      stdinPolicy: "closed",
      timeoutMs: 120_000,
    });
  });

  it("does not store a successful assistant response after cancellation", async () => {
    const controller = new AbortController();
    const { tool } = createStubCommandTool({
      result: { status: "cancelled", message: "Cancelled." },
    });
    const application = scriptedApplication(
      [[toolCall("c1", "process.run", {}), { type: "completed" }]],
      tool,
      {
        reviewer: () => Promise.resolve({ type: "approve_once" }),
      },
    );
    for await (const event of application.sendPrompt("run npm check", controller.signal)) {
      if (event.type === "command_started") {
        controller.abort();
      }
    }
    expect(application.getStatus().messageCount).toBe(3);
  });

  it("binds the approved digest to the execution request", async () => {
    const { tool, executed } = createStubCommandTool();
    const application = scriptedApplication(
      [[toolCall("c1", "process.run", {}), { type: "completed" }], [{ type: "completed" }]],
      tool,
      { reviewer: () => Promise.resolve({ type: "approve_once" }) },
    );
    await collectEvents(application.sendPrompt("run npm check"));
    expect(executed[0]?.approvedDigest).toBe("digest-abc123");
  });
});

describe("prepared command tool preparation failures", () => {
  it("fails truthfully when preparation fails", async () => {
    const { tool } = createStubCommandTool({ prepareError: "The npm runner is unavailable." });
    const application = scriptedApplication(
      [[toolCall("c1", "process.run", {}), { type: "completed" }], [{ type: "completed" }]],
      tool,
      { reviewer: () => Promise.resolve({ type: "approve_once" }) },
    );
    const events = await collectEvents(application.sendPrompt("run npm check"));
    const failed = events.find((event) => event.type === "tool_failed");
    expect(failed).toMatchObject({ message: "The npm runner is unavailable." });
    expect(events.some((event) => event.type === "command_prepared")).toBe(false);
  });
});
