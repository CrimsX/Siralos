import { describe, expect, it } from "vitest";
import {
  createSolarisApplication,
  createToolRegistry,
  type ApplicationEvent,
  type ModelEvent,
  type ModelProvider,
  type ModelRequest,
  type Tool,
  type ToolExecutionResult,
} from "../index.js";

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

function createStubTool(name: string): { tool: Tool; calls: unknown[] } {
  const calls: unknown[] = [];
  const tool: Tool = {
    definition: { name, description: `Stub ${name}`, inputSchema: {} },
    execute(input: unknown): Promise<ToolExecutionResult> {
      calls.push(input);
      return Promise.resolve({ status: "success", output: { ok: true }, summary: "ok" });
    },
  };
  return { tool, calls };
}

function toolCall(callId: string, toolName: string, input: unknown): ModelEvent {
  return { type: "tool_call", callId, toolName, input };
}

async function collectEvents(events: AsyncIterable<ApplicationEvent>): Promise<ApplicationEvent[]> {
  const collected: ApplicationEvent[] = [];
  for await (const event of events) {
    collected.push(event);
  }
  return collected;
}

function describeItems(requests: ModelRequest[]): string[][] {
  return requests.map((request) =>
    request.messages.map((item) => {
      switch (item.type) {
        case "user_message":
        case "assistant_message":
          return `${item.type}:${item.content}`;
        case "assistant_tool_call":
          return `${item.type}:${item.toolName}`;
        case "tool_result":
          return `${item.type}:${item.toolName}:${item.result.status}`;
      }
    }),
  );
}

describe("provider/tool loop", () => {
  it("sends the available tool definitions to the provider", async () => {
    const { provider, requests } = createScriptedProvider([[{ type: "completed" }]]);
    const { tool } = createStubTool("a.tool");
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
    });
    await collectEvents(application.sendPrompt("hello"));
    expect(requests).toHaveLength(1);
    expect(requests[0]?.tools.map((definition) => definition.name)).toEqual(["a.tool"]);
  });

  it("executes a provider tool call and stores call and result distinctly", async () => {
    const { provider, requests } = createScriptedProvider([
      [toolCall("c1", "a.tool", { value: 1 }), { type: "completed" }],
      [{ type: "text_delta", text: "done" }, { type: "completed" }],
    ]);
    const { tool, calls } = createStubTool("a.tool");
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(calls).toEqual([{ value: 1 }]);
    expect(events).toEqual([
      { type: "response_started" },
      { type: "tool_started", callId: "c1", toolName: "a.tool", displayInput: '{"value":1}' },
      { type: "tool_completed", callId: "c1", toolName: "a.tool", summary: "ok" },
      { type: "text_delta", text: "done" },
      { type: "response_completed" },
    ]);
    expect(describeItems(requests)).toEqual([
      ["user_message:hello"],
      ["user_message:hello", "assistant_tool_call:a.tool", "tool_result:a.tool:success"],
    ]);
    expect(application.getStatus().messageCount).toBe(4);
  });

  it("executes multiple tool calls sequentially", async () => {
    const { provider } = createScriptedProvider([
      [toolCall("c1", "a.tool", {}), toolCall("c2", "b.tool", {}), { type: "completed" }],
      [{ type: "completed" }],
    ]);
    const { tool: toolA, calls: callsA } = createStubTool("a.tool");
    const { tool: toolB, calls: callsB } = createStubTool("b.tool");
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([toolA, toolB]),
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(callsA).toHaveLength(1);
    expect(callsB).toHaveLength(1);
    const started = events.filter((event) => event.type === "tool_started");
    expect(started.map((event) => event.toolName)).toEqual(["a.tool", "b.tool"]);
  });

  it("fails safely for unknown tools and lets the provider recover", async () => {
    const { provider, requests } = createScriptedProvider([
      [toolCall("c1", "mystery.tool", {}), { type: "completed" }],
      [{ type: "text_delta", text: "recovered" }, { type: "completed" }],
    ]);
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([]),
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(events.some((event) => event.type === "response_completed")).toBe(true);
    const failed = events.find((event) => event.type === "tool_failed");
    expect(failed).toEqual({
      type: "tool_failed",
      callId: "c1",
      toolName: "mystery.tool",
      message: "Unknown tool: mystery.tool.",
    });
    expect(describeItems(requests).at(-1)).toContain("tool_result:mystery.tool:failed");
  });

  it("returns invalid_input without executing the tool", async () => {
    const { provider } = createScriptedProvider([
      [toolCall("c1", "strict.tool", 42), { type: "completed" }],
      [{ type: "completed" }],
    ]);
    let workRan = false;
    const tool: Tool = {
      definition: { name: "strict.tool", description: "Strict stub", inputSchema: {} },
      execute(input: unknown): Promise<ToolExecutionResult> {
        if (typeof input !== "object" || input === null) {
          return Promise.resolve({ status: "invalid_input", message: "Input must be an object." });
        }
        workRan = true;
        return Promise.resolve({ status: "success", output: {}, summary: "ok" });
      },
    };
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(workRan).toBe(false);
    const failed = events.find((event) => event.type === "tool_failed");
    expect(failed).toMatchObject({ message: "Input must be an object." });
    expect(events.some((event) => event.type === "tool_completed")).toBe(false);
  });

  it("normalizes duplicate tool call ids into a safe failure", async () => {
    const { provider } = createScriptedProvider([
      [
        toolCall("c1", "a.tool", {}),
        toolCall("c1", "a.tool", { again: true }),
        { type: "completed" },
      ],
      [{ type: "completed" }],
    ]);
    const { tool, calls } = createStubTool("a.tool");
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(calls).toHaveLength(1);
    const failures = events.filter((event) => event.type === "tool_failed");
    expect(failures).toHaveLength(1);
    expect(failures[0]).toMatchObject({ message: "Duplicate tool call id: c1." });
  });

  it("enforces the tool-round limit and stops without executing beyond it", async () => {
    const { provider, requests } = createScriptedProvider([
      [toolCall("c1", "a.tool", {}), { type: "completed" }],
      [toolCall("c2", "a.tool", {}), { type: "completed" }],
      [toolCall("c3", "a.tool", {}), { type: "completed" }],
      [{ type: "completed" }],
    ]);
    const { tool, calls } = createStubTool("a.tool");
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
      maxToolRounds: 2,
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(calls).toHaveLength(2);
    const failed = events.find((event) => event.type === "response_failed");
    expect(failed).toMatchObject({
      type: "response_failed",
      message:
        "Solaris reached the maximum of 2 tool rounds; the requested tool round was not executed.",
    });
    expect(events.some((event) => event.type === "response_completed")).toBe(false);
    expect(requests).toHaveLength(3);
  });

  it("allows a final answer after the last permitted tool round", async () => {
    const { provider } = createScriptedProvider([
      [toolCall("c1", "a.tool", {}), { type: "completed" }],
      [{ type: "text_delta", text: "final answer" }, { type: "completed" }],
    ]);
    const { tool } = createStubTool("a.tool");
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
      maxToolRounds: 1,
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(events.map((event) => event.type)).toEqual([
      "response_started",
      "tool_started",
      "tool_completed",
      "text_delta",
      "response_completed",
    ]);
  });

  it("rejects a second tool round beyond maxToolRounds without executing it", async () => {
    const { provider } = createScriptedProvider([
      [toolCall("c1", "a.tool", {}), { type: "completed" }],
      [toolCall("c2", "a.tool", {}), { type: "completed" }],
    ]);
    const { tool, calls } = createStubTool("a.tool");
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
      maxToolRounds: 1,
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(calls).toHaveLength(1);
    expect(events.some((event) => event.type === "response_completed")).toBe(false);
    const failed = events.find((event) => event.type === "response_failed") as
      { message?: string } | undefined;
    expect(failed?.message).toContain("maximum of 1 tool rounds");
  });

  it("accepts an empty valid completion", async () => {
    const { provider } = createScriptedProvider([[{ type: "completed" }]]);
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([]),
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(events.map((event) => event.type)).toEqual(["response_started", "response_completed"]);
    expect(application.getStatus().messageCount).toBe(1);
  });

  it("does not report success when the provider fails after a tool result", async () => {
    let turn = 0;
    const provider: ModelProvider = {
      id: "failing-after-tools-stub",
      async *stream(): AsyncIterable<ModelEvent> {
        turn += 1;
        if (turn === 1) {
          yield { type: "tool_call", callId: "c1", toolName: "a.tool", input: {} };
          yield { type: "completed" };
          await Promise.resolve();
          return;
        }
        throw new Error("provider exploded");
      },
    };
    const { tool } = createStubTool("a.tool");
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(events.map((event) => event.type)).toEqual([
      "response_started",
      "tool_started",
      "tool_completed",
      "response_failed",
    ]);
    expect(events.some((event) => event.type === "response_completed")).toBe(false);
    expect(application.getStatus().messageCount).toBe(3);
  });

  it("stops an active tool when the request is cancelled", async () => {
    const controller = new AbortController();
    const { provider } = createScriptedProvider([
      [toolCall("c1", "wait.tool", {}), { type: "completed" }],
    ]);
    let toolStarted: (() => void) | undefined;
    const startedGate = new Promise<void>((resolve) => {
      toolStarted = resolve;
    });
    const abortWhenStarted = startedGate.then(() => {
      controller.abort();
    });
    const tool: Tool = {
      definition: { name: "wait.tool", description: "Waits", inputSchema: {} },
      async execute(_input, context): Promise<ToolExecutionResult> {
        if (context.signal?.aborted) {
          return { status: "cancelled", message: "Cancelled." };
        }
        toolStarted?.();
        await new Promise<void>((resolve) => {
          context.signal?.addEventListener("abort", () => resolve(), { once: true });
        });
        return { status: "cancelled", message: "Cancelled." };
      },
    };
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
    });
    const events = await collectEvents(application.sendPrompt("hello", controller.signal));
    await abortWhenStarted;
    expect(events.map((event) => event.type)).toEqual([
      "response_started",
      "tool_started",
      "tool_cancelled",
      "response_cancelled",
    ]);
    expect(events.some((event) => event.type === "response_completed")).toBe(false);
  });

  it("does not start later tool calls after cancellation", async () => {
    const controller = new AbortController();
    const { provider } = createScriptedProvider([
      [toolCall("c1", "a.tool", {}), toolCall("c2", "b.tool", {}), { type: "completed" }],
    ]);
    const { tool: toolA } = createStubTool("a.tool");
    const { tool: toolB, calls: callsB } = createStubTool("b.tool");
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([toolA, toolB]),
    });
    const events: ApplicationEvent[] = [];
    for await (const event of application.sendPrompt("hello", controller.signal)) {
      events.push(event);
      if (event.type === "tool_completed" && event.callId === "c1") {
        controller.abort();
      }
    }
    expect(callsB).toHaveLength(0);
    expect(events.some((event) => event.type === "tool_started" && event.callId === "c2")).toBe(
      false,
    );
    expect(events.some((event) => event.type === "response_cancelled")).toBe(true);
    expect(events.some((event) => event.type === "response_completed")).toBe(false);
    expect(application.getStatus().messageCount).toBe(5);
  });
});
