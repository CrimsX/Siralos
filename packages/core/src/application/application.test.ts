import { describe, expect, it } from "vitest";
import {
  createSolarisApplication,
  createToolRegistry,
  type ApplicationEvent,
  type ModelEvent,
  type ModelProvider,
  type ModelRequest,
} from "../index.js";

function createStreamingProvider(events: readonly ModelEvent[]): ModelProvider {
  return {
    id: "streaming-stub",
    async *stream(): AsyncIterable<ModelEvent> {
      await Promise.resolve();
      for (const event of events) {
        yield event;
      }
    },
  };
}

function createRecordingProvider(): {
  provider: ModelProvider;
  requests: ModelRequest[];
} {
  const requests: ModelRequest[] = [];
  const provider: ModelProvider = {
    id: "recording-stub",
    async *stream(request: ModelRequest): AsyncIterable<ModelEvent> {
      await Promise.resolve();
      requests.push(request);
      yield { type: "text_delta", text: "ok" };
      yield { type: "completed" };
    },
  };
  return { provider, requests };
}

function createFailingProvider(): ModelProvider {
  return {
    id: "failing-stub",
    stream(): AsyncIterable<ModelEvent> {
      throw new Error("provider exploded");
    },
  };
}

function createGateProvider(): {
  provider: ModelProvider;
  release: () => void;
} {
  let release: () => void = () => {};
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const provider: ModelProvider = {
    id: "gated-stub",
    async *stream(): AsyncIterable<ModelEvent> {
      yield { type: "text_delta", text: "part" };
      await gate;
      yield { type: "completed" };
    },
  };
  return { provider, release };
}

async function collectEvents(events: AsyncIterable<ApplicationEvent>): Promise<ApplicationEvent[]> {
  const collected: ApplicationEvent[] = [];
  for await (const event of events) {
    collected.push(event);
  }
  return collected;
}

describe("createSolarisApplication", () => {
  it("streams a prompt response through the application", async () => {
    const provider = createStreamingProvider([
      { type: "text_delta", text: "one" },
      { type: "text_delta", text: "two" },
      { type: "completed" },
    ]);
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([]),
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(events).toEqual([
      { type: "response_started" },
      { type: "text_delta", text: "one" },
      { type: "text_delta", text: "two" },
      { type: "response_completed" },
    ]);
  });

  it("passes the full conversation history to the provider", async () => {
    const { provider, requests } = createRecordingProvider();
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([]),
    });
    await collectEvents(application.sendPrompt("first"));
    await collectEvents(application.sendPrompt("second"));
    expect(
      requests.map((request) =>
        request.messages.map((item) =>
          item.type === "user_message" || item.type === "assistant_message"
            ? `${item.type}:${item.content}`
            : `${item.type}:${item.callId}`,
        ),
      ),
    ).toEqual([
      ["user_message:first"],
      ["user_message:first", "assistant_message:ok", "user_message:second"],
    ]);
  });

  it("reports message count for the stored conversation", async () => {
    const { provider } = createRecordingProvider();
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([]),
    });
    expect(application.getStatus().messageCount).toBe(0);
    await collectEvents(application.sendPrompt("hello"));
    expect(application.getStatus()).toEqual({
      providerId: "recording-stub",
      state: "idle",
      messageCount: 2,
      pendingApproval: false,
    });
  });

  it("reports a failed provider response without storing an assistant message", async () => {
    const application = createSolarisApplication({
      provider: createFailingProvider(),
      tools: createToolRegistry([]),
    });
    const events = await collectEvents(application.sendPrompt("hello"));
    expect(events.at(-1)).toEqual({
      type: "response_failed",
      message: "provider exploded",
    });
    expect(events.some((event) => event.type === "response_completed")).toBe(false);
    expect(application.getStatus().messageCount).toBe(1);
  });

  it("emits response_cancelled for a pre-aborted request", async () => {
    const controller = new AbortController();
    controller.abort();
    const application = createSolarisApplication({
      provider: createStreamingProvider([{ type: "completed" }]),
      tools: createToolRegistry([]),
    });
    const events = await collectEvents(application.sendPrompt("hello", controller.signal));
    expect(events.map((event) => event.type)).toEqual(["response_started", "response_cancelled"]);
    expect(application.getStatus().messageCount).toBe(1);
  });

  it("cancels an in-flight response without a successful completion", async () => {
    const controller = new AbortController();
    const provider: ModelProvider = {
      id: "abort-stub",
      async *stream(request: ModelRequest): AsyncIterable<ModelEvent> {
        for (const chunk of ["one", "two"]) {
          if (request.signal?.aborted) {
            throw new DOMException("The operation was aborted.", "AbortError");
          }
          yield { type: "text_delta", text: chunk };
          await Promise.resolve();
        }
        yield { type: "completed" };
      },
    };
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([]),
    });
    const events: ApplicationEvent[] = [];
    for await (const event of application.sendPrompt("hello", controller.signal)) {
      events.push(event);
      if (event.type === "text_delta") {
        controller.abort();
      }
    }
    expect(events.map((event) => event.type)).toEqual([
      "response_started",
      "text_delta",
      "response_cancelled",
    ]);
    expect(application.getStatus().messageCount).toBe(1);
  });

  it("reports the responding state while a response streams", async () => {
    const { provider, release } = createGateProvider();
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([]),
    });
    const events = application.sendPrompt("hello");
    const iterator = events[Symbol.asyncIterator]();
    await iterator.next();
    expect(application.getStatus().state).toBe("responding");
    release();
    await collectEvents(events);
    expect(application.getStatus().state).toBe("idle");
  });

  it("rejects a second prompt while one is responding", async () => {
    const { provider, release } = createGateProvider();
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([]),
    });
    const first = application.sendPrompt("first");
    const firstIterator = first[Symbol.asyncIterator]();
    await firstIterator.next();
    const second = application.sendPrompt("second");
    await expect(second[Symbol.asyncIterator]().next()).rejects.toThrow("already responding");
    release();
    await collectEvents(first);
  });
});
