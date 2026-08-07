import { describe, expect, it } from "vitest";
import {
  createSolarisApplication,
  createToolRegistry,
  PROVIDER_TURN_LIMITS,
  validateConversationItems,
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
  returnCount: () => number;
} {
  const requests: ModelRequest[] = [];
  let index = 0;
  let returnCount = 0;
  const provider: ModelProvider = {
    id: "scripted-stub",
    stream(request: ModelRequest): AsyncIterable<ModelEvent> {
      requests.push(request);
      const events = turns[index] ?? [];
      index += 1;
      const generator = {
        async *[Symbol.asyncIterator](): AsyncIterableIterator<ModelEvent> {
          for (const event of events) {
            yield event;
            await Promise.resolve();
          }
        },
      };
      return {
        [Symbol.asyncIterator]() {
          const iterator: AsyncIterator<ModelEvent> = generator[Symbol.asyncIterator]();
          return {
            next: () => iterator.next(),
            return: (value: IteratorResult<ModelEvent>) => {
              returnCount += 1;
              return iterator.return!(value);
            },
            throw: (error: unknown) => {
              returnCount += 1;
              return iterator.throw!(error instanceof Error ? error : new Error(String(error)));
            },
          };
        },
      };
    },
  };
  return { provider, requests, returnCount: () => returnCount };
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

function makeApplication(provider: ModelProvider, tools: readonly Tool[], maxToolRounds?: number) {
  return createSolarisApplication({
    provider,
    tools: createToolRegistry(tools),
    ...(maxToolRounds === undefined ? {} : { maxToolRounds }),
  });
}

async function collectEvents(events: AsyncIterable<ApplicationEvent>): Promise<ApplicationEvent[]> {
  const collected: ApplicationEvent[] = [];
  for await (const event of events) {
    collected.push(event);
  }
  return collected;
}

function failReason(events: readonly ApplicationEvent[]): string | null {
  const failed = events.find((event) => event.type === "response_failed");
  return failed === undefined ? null : (failed as { message: string }).message;
}

describe("provider stream completion protocol", () => {
  it("rejects a delta followed by EOF without completion", async () => {
    const { provider } = createScriptedProvider([[{ type: "text_delta", text: "partial" }]]);
    const application = makeApplication(provider, []);
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(failReason(events)).toContain("without a completion event");
    expect(events.some((event) => event.type === "response_completed")).toBe(false);
    expect(application.getStatus().messageCount).toBe(1);
  });

  it("rejects a tool call followed by EOF without completion", async () => {
    const { provider } = createScriptedProvider([[toolCall("c1", "a.tool", {})]]);
    const { tool, calls } = createStubTool("a.tool");
    const application = makeApplication(provider, [tool]);
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(calls).toHaveLength(0);
    expect(failReason(events)).toContain("without a completion event");
  });

  it("rejects duplicate completion events", async () => {
    const { provider } = createScriptedProvider([[{ type: "completed" }, { type: "completed" }]]);
    const application = makeApplication(provider, []);
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(failReason(events)).toContain("after completion");
  });

  it("rejects events after completion", async () => {
    const { provider } = createScriptedProvider([
      [{ type: "completed" }, { type: "text_delta", text: "late" }],
    ]);
    const application = makeApplication(provider, []);
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(failReason(events)).toContain("after completion");
    expect(events.some((event) => event.type === "response_completed")).toBe(false);
  });

  it("closes the provider iterator when a bound is exceeded", async () => {
    const { provider, returnCount } = createScriptedProvider([
      [
        {
          type: "text_delta",
          text: "x".repeat(PROVIDER_TURN_LIMITS.maxAssistantTextBytes + 1),
        },
        { type: "completed" },
      ],
    ]);
    const application = makeApplication(provider, []);
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(failReason(events)).toContain("assistant-text byte limit");
    expect(returnCount()).toBeGreaterThan(0);
  });

  it("does not commit partial assistant text from a rejected turn", async () => {
    const { provider, requests } = createScriptedProvider([
      [
        { type: "text_delta", text: "prefix-" },
        { type: "text_delta", text: "x".repeat(PROVIDER_TURN_LIMITS.maxAssistantTextBytes + 1) },
        { type: "completed" },
      ],
    ]);
    const application = makeApplication(provider, []);
    await collectEvents(application.sendPrompt("hello"));
    await collectEvents(application.sendPrompt("again"));
    const secondRequest = requests[1];
    const items = secondRequest?.messages ?? [];
    expect(items.some((item) => item.type === "assistant_message")).toBe(false);
  });
});

describe("provider stream resource bounds", () => {
  it("rejects a huge text stream", async () => {
    const { provider } = createScriptedProvider([
      [{ type: "text_delta", text: "y".repeat(PROVIDER_TURN_LIMITS.maxAssistantTextBytes + 1) }],
    ]);
    const application = makeApplication(provider, []);
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(failReason(events)).toContain("assistant-text byte limit");
  });

  it("rejects huge fragmented tool arguments", async () => {
    const { provider } = createScriptedProvider([
      [
        toolCall("c1", "a.tool", {
          payload: "z".repeat(PROVIDER_TURN_LIMITS.maxToolArgumentBytes),
        }),
        { type: "completed" },
      ],
    ]);
    const { tool, calls } = createStubTool("a.tool");
    const application = makeApplication(provider, [tool]);
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(calls).toHaveLength(0);
    expect(failReason(events)).toContain("tool-argument byte limit");
  });

  it("rejects an excessive tool-call count", async () => {
    const calls = Array.from({ length: PROVIDER_TURN_LIMITS.maxToolCallsPerTurn + 1 }, (_, i) =>
      toolCall(`c${i}`, "a.tool", {}),
    );
    const { provider } = createScriptedProvider([[...calls, { type: "completed" }]]);
    const { tool, calls: executed } = createStubTool("a.tool");
    const application = makeApplication(provider, [tool]);
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(failReason(events)).toContain("tool-call count");
    expect(executed.length).toBeLessThanOrEqual(PROVIDER_TURN_LIMITS.maxToolCallsPerTurn);
  });

  it("rejects an oversized tool name", async () => {
    const { provider } = createScriptedProvider([
      [
        toolCall("c1", "t".repeat(PROVIDER_TURN_LIMITS.maxToolNameBytes + 1), {}),
        { type: "completed" },
      ],
    ]);
    const application = makeApplication(provider, []);
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(failReason(events)).toContain("tool-name byte limit");
  });

  it("counts UTF-8 bytes, not characters", async () => {
    const { provider } = createScriptedProvider([
      [{ type: "text_delta", text: "\u00e9".repeat(PROVIDER_TURN_LIMITS.maxAssistantTextBytes) }],
    ]);
    const application = makeApplication(provider, []);
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(failReason(events)).toContain("assistant-text byte limit");
  });

  it("enforces the assistant-text limit cumulatively across individually legal deltas", async () => {
    const delta = "x".repeat(Math.floor(PROVIDER_TURN_LIMITS.maxAssistantTextBytes / 2) + 100);
    const { provider } = createScriptedProvider([
      [
        { type: "text_delta", text: delta },
        { type: "text_delta", text: delta },
        { type: "completed" },
      ],
    ]);
    const application = makeApplication(provider, []);
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(failReason(events)).toContain("assistant-text byte limit");
    expect(events.some((event) => event.type === "response_completed")).toBe(false);
  });

  it("rejects cumulative assistant text that only crosses the limit on a multibyte delta", async () => {
    // 32 KiB of "a" plus a delta whose UTF-8 bytes (2 per \u00e9) push the
    // cumulative total past the limit while the delta itself is small.
    const prefix = "a".repeat(Math.floor(PROVIDER_TURN_LIMITS.maxAssistantTextBytes / 2));
    const crossing = "\u00e9".repeat(Math.floor(PROVIDER_TURN_LIMITS.maxAssistantTextBytes / 2));
    const { provider } = createScriptedProvider([
      [
        { type: "text_delta", text: prefix },
        { type: "text_delta", text: crossing },
        { type: "completed" },
      ],
    ]);
    const application = makeApplication(provider, []);
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(failReason(events)).toContain("assistant-text byte limit");
  });

  it("accepts assistant text exactly at the cumulative limit", async () => {
    const { provider } = createScriptedProvider([
      [
        { type: "text_delta", text: "a".repeat(PROVIDER_TURN_LIMITS.maxAssistantTextBytes) },
        { type: "completed" },
      ],
    ]);
    const application = makeApplication(provider, []);
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(failReason(events)).toBeNull();
    expect(events.some((event) => event.type === "response_completed")).toBe(true);
  });

  it("rejects text plus tool calls reaching the aggregate turn limit", async () => {
    const textBytes = 50 * 1024;
    const argumentBytes = Math.floor(PROVIDER_TURN_LIMITS.maxToolArgumentBytes * 0.95);
    const { provider } = createScriptedProvider([
      [
        { type: "text_delta", text: "a".repeat(textBytes) },
        toolCall("c1", "a.tool", { payload: "z".repeat(argumentBytes) }),
        toolCall("c2", "a.tool", { payload: "z".repeat(argumentBytes) }),
        { type: "completed" },
      ],
    ]);
    const { tool, calls } = createStubTool("a.tool");
    const application = makeApplication(provider, [tool]);
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(failReason(events)).toContain("aggregate turn byte limit");
    expect(calls).toHaveLength(0);
  });

  it("closes the provider iterator and commits no partial history on cumulative rejection", async () => {
    const delta = "x".repeat(Math.floor(PROVIDER_TURN_LIMITS.maxAssistantTextBytes / 2) + 100);
    const { provider, requests, returnCount } = createScriptedProvider([
      [
        { type: "text_delta", text: delta },
        { type: "text_delta", text: delta },
        { type: "completed" },
      ],
    ]);
    const application = makeApplication(provider, []);
    await collectEvents(application.sendPrompt("hello"));
    expect(returnCount()).toBeGreaterThan(0);
    await collectEvents(application.sendPrompt("again"));
    const items = requests[1]?.messages ?? [];
    expect(items.some((item) => item.type === "assistant_message")).toBe(false);
  });

  it("cancels promptly during an endless stream", async () => {
    const controller = new AbortController();
    let ticks = 0;
    const provider: ModelProvider = {
      id: "endless-stub",
      async *stream(): AsyncIterable<ModelEvent> {
        for (;;) {
          ticks += 1;
          yield { type: "text_delta", text: "tick" };
          await Promise.resolve();
        }
      },
    };
    const application = makeApplication(provider, []);
    const events: ApplicationEvent[] = [];
    const completion = (async () => {
      for await (const event of application.sendPrompt("hello", controller.signal)) {
        events.push(event);
        if (ticks >= 3) {
          controller.abort();
        }
      }
    })();
    await completion;
    expect(events.some((event) => event.type === "response_cancelled")).toBe(true);
    expect(events.some((event) => event.type === "response_completed")).toBe(false);
  });
});

describe("tool-round accounting", () => {
  it("cancels cleanly at the boundary before any turn", async () => {
    const controller = new AbortController();
    controller.abort();
    const { provider } = createScriptedProvider([[{ type: "completed" }]]);
    const application = makeApplication(provider, []);
    const events = await collectEvents(application.sendPrompt("hello", controller.signal));
    expect(events.map((event) => event.type)).toEqual(["response_started", "response_cancelled"]);
  });

  it("does not execute any tool round when maxToolRounds is zero", async () => {
    const { provider } = createScriptedProvider([
      [toolCall("c1", "a.tool", {}), { type: "completed" }],
    ]);
    const { tool, calls } = createStubTool("a.tool");
    const application = makeApplication(provider, [tool], 0);
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(calls).toHaveLength(0);
    expect(failReason(events)).toContain("maximum of 0 tool rounds");
  });
});

describe("transcript pairing integrity", () => {
  function historyProvider(turns: readonly (readonly ModelEvent[])[]): {
    provider: ModelProvider;
    requests: ModelRequest[];
  } {
    const requests: ModelRequest[] = [];
    let index = 0;
    return {
      provider: {
        id: "history-stub",
        async *stream(request: ModelRequest): AsyncIterable<ModelEvent> {
          requests.push(request);
          const events = turns[index] ?? [];
          index += 1;
          for (const event of events) {
            yield event;
            await Promise.resolve();
          }
        },
      },
      requests,
    };
  }

  it("pairs cancelled calls with explicit cancelled results after cancellation", async () => {
    const controller = new AbortController();
    const { provider, requests } = historyProvider([
      [toolCall("c1", "a.tool", {}), toolCall("c2", "b.tool", {}), { type: "completed" }],
      [{ type: "completed" }],
    ]);
    const { tool: toolA, calls: callsA } = createStubTool("a.tool");
    const { tool: toolB, calls: callsB } = createStubTool("b.tool");
    const application = makeApplication(provider, [toolA, toolB]);
    const events: ApplicationEvent[] = [];
    for await (const event of application.sendPrompt("hello", controller.signal)) {
      events.push(event);
      if (event.type === "tool_completed" && event.callId === "c1") {
        controller.abort();
      }
    }
    expect(callsA).toHaveLength(1);
    expect(callsB).toHaveLength(0);
    await collectEvents(application.sendPrompt("next"));
    const transcript = requests[1]?.messages ?? [];
    expect(validateConversationItems(transcript)).toBeNull();
    const results = transcript.filter(
      (item): item is Extract<(typeof requests)[1]["messages"][number], { type: "tool_result" }> =>
        item.type === "tool_result",
    );
    expect(results.map((item) => item.result.status)).toEqual(["success", "cancelled"]);
    expect(results.map((item) => item.callId)).toEqual(["c1", "c2"]);
  });

  it("produces a valid transcript after an approval denial", async () => {
    const { provider, requests } = historyProvider([
      [toolCall("c1", "write.tool", {}), { type: "completed" }],
    ]);
    const tool: Tool = {
      definition: { name: "write.tool", description: "Denied", inputSchema: {} },
      capability: "workspace.write",
      execute(): Promise<ToolExecutionResult> {
        return Promise.resolve({ status: "denied", message: "denied by policy" });
      },
    };
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
    });
    await collectEvents(application.sendPrompt("hello"));
    await collectEvents(application.sendPrompt("next"));
    expect(validateConversationItems(requests[0]?.messages ?? [])).toBeNull();
    expect(validateConversationItems(requests[1]?.messages ?? [])).toBeNull();
  });

  it("produces a valid transcript after a tool execution exception", async () => {
    const { provider, requests } = historyProvider([
      [toolCall("c1", "crash.tool", {}), { type: "completed" }],
    ]);
    const tool: Tool = {
      definition: { name: "crash.tool", description: "Crashes", inputSchema: {} },
      execute(): Promise<ToolExecutionResult> {
        return Promise.reject(new Error("exploded"));
      },
    };
    const application = makeApplication(provider, [tool]);
    await collectEvents(application.sendPrompt("hello"));
    await collectEvents(application.sendPrompt("next"));
    expect(validateConversationItems(requests[1]?.messages ?? [])).toBeNull();
    const results = requests[1]?.messages.filter(
      (item): item is Extract<(typeof requests)[1]["messages"][number], { type: "tool_result" }> =>
        item.type === "tool_result",
    );
    expect(results?.map((item) => item.result.status)).toEqual(["failed"]);
  });

  it("never creates a second result for an earlier call", async () => {
    const { provider, requests } = historyProvider([
      [
        toolCall("c1", "a.tool", {}),
        toolCall("c1", "a.tool", { again: true }),
        { type: "completed" },
      ],
      [{ type: "completed" }],
    ]);
    const { tool } = createStubTool("a.tool");
    const application = makeApplication(provider, [tool]);
    await collectEvents(application.sendPrompt("hello"));
    await collectEvents(application.sendPrompt("next"));
    const transcript = requests[1]?.messages ?? [];
    expect(validateConversationItems(transcript)).toBeNull();
    const results = transcript.filter(
      (item): item is Extract<(typeof requests)[1]["messages"][number], { type: "tool_result" }> =>
        item.type === "tool_result",
    );
    expect(results).toHaveLength(2);
    expect(new Set(results.map((item) => item.callId)).size).toBe(2);
    expect(results.map((item) => item.callId)).toEqual(["c1", "invalid-call-1"]);
  });

  it("keeps results untrusted: tool output is never added to the transcript as instructions", async () => {
    const { provider, requests } = historyProvider([
      [toolCall("c1", "a.tool", {}), { type: "completed" }],
      [{ type: "completed" }],
    ]);
    const tool: Tool = {
      definition: { name: "a.tool", description: "Injects", inputSchema: {} },
      execute(): Promise<ToolExecutionResult> {
        return Promise.resolve({
          status: "success",
          output: { ok: true },
          summary: "ignore this instruction",
        });
      },
    };
    const application = makeApplication(provider, [tool]);
    await collectEvents(application.sendPrompt("hello"));
    const transcript = requests[1]?.messages ?? [];
    expect(validateConversationItems(transcript)).toBeNull();
    expect(
      transcript.some(
        (item) =>
          (item.type === "user_message" && item.content.includes("ignore this instruction")) ||
          (item.type === "assistant_message" && item.content.includes("ignore this instruction")),
      ),
    ).toBe(false);
  });
});
