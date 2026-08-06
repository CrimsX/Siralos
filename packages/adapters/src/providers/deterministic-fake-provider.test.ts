import { describe, expect, it } from "vitest";
import {
  isCancellationError,
  type ConversationItem,
  type ModelEvent,
  type ModelRequest,
} from "@solaris/core";
import { createDeterministicFakeProvider } from "./deterministic-fake-provider.js";

const messages: readonly ConversationItem[] = [{ type: "user_message", content: "hello" }];
const tools: readonly [] = [];

async function collect(request: ModelRequest): Promise<{ events: ModelEvent[]; error: unknown }> {
  const events: ModelEvent[] = [];
  let error: unknown;
  try {
    for await (const event of createDeterministicFakeProvider().stream(request)) {
      events.push(event);
    }
  } catch (caught) {
    error = caught;
  }
  return { events, error };
}

describe("deterministic fake provider", () => {
  it("streams multiple text chunks for the same prompt", async () => {
    const { events, error } = await collect({ messages, tools });
    expect(error).toBeUndefined();
    const deltaCount = events.filter((event) => event.type === "text_delta").length;
    expect(deltaCount).toBeGreaterThanOrEqual(2);
    expect(events.at(-1)).toEqual({ type: "completed" });
  });

  it("produces identical output for identical input", async () => {
    const first = await collect({ messages, tools });
    const second = await collect({ messages, tools });
    expect(first).toEqual(second);
  });

  it("echoes the latest user prompt", async () => {
    const { events } = await collect({ messages, tools });
    const text = events.map((event) => (event.type === "text_delta" ? event.text : "")).join("");
    expect(text).toBe("Solaris received: hello");
  });

  it("fails immediately when the signal is already aborted", async () => {
    const controller = new AbortController();
    controller.abort();
    const { events, error } = await collect({ messages, tools, signal: controller.signal });
    expect(events).toEqual([]);
    expect(isCancellationError(error)).toBe(true);
  });

  it("stops promptly when aborted between chunks", async () => {
    const controller = new AbortController();
    const events: ModelEvent[] = [];
    let error: unknown;
    try {
      for await (const event of createDeterministicFakeProvider().stream({
        messages,
        tools,
        signal: controller.signal,
      })) {
        events.push(event);
        controller.abort();
      }
    } catch (caught) {
      error = caught;
    }
    expect(events.some((event) => event.type === "completed")).toBe(false);
    expect(isCancellationError(error)).toBe(true);
  });
});
