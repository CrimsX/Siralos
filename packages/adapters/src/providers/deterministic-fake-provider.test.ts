import { describe, expect, it } from "vitest";
import {
  isCancellationError,
  type ConversationItem,
  type ModelEvent,
  type ModelRequest,
  type ToolDefinition,
} from "@solaris/core";
import { createDeterministicFakeProvider } from "./deterministic-fake-provider.js";

const messages: readonly ConversationItem[] = [{ type: "user_message", content: "hello" }];
const tools: readonly [] = [];

const LIST_TOOL: ToolDefinition = {
  name: "workspace.list",
  description: "List one directory within the approved workspace.",
  inputSchema: {},
};
const READ_TOOL: ToolDefinition = {
  name: "workspace.read",
  description: "Read a bounded range from one text file inside the workspace.",
  inputSchema: {},
};
const SEARCH_TOOL: ToolDefinition = {
  name: "workspace.search",
  description: "Search text files recursively within a bounded workspace directory.",
  inputSchema: {},
};

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

function textOf(events: readonly ModelEvent[]): string {
  return events.map((event) => (event.type === "text_delta" ? event.text : "")).join("");
}

function toolCallEvent(events: readonly ModelEvent[]): ModelEvent | undefined {
  return events.find((event) => event.type === "tool_call");
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
    expect(textOf(events)).toBe("Solaris received: hello");
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

describe("deterministic fake provider tool scenarios", () => {
  it("requests workspace.list for `list files`", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "list files" }],
      tools: [LIST_TOOL],
    };
    const { events, error } = await collect(request);
    expect(error).toBeUndefined();
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-1",
      toolName: "workspace.list",
      input: { path: "." },
    });
  });

  it("reports a final response after the list result", async () => {
    const request: ModelRequest = {
      messages: [
        { type: "user_message", content: "list files" },
        {
          type: "assistant_tool_call",
          callId: "call-1",
          toolName: "workspace.list",
          input: { path: "." },
        },
        {
          type: "tool_result",
          callId: "call-1",
          toolName: "workspace.list",
          result: {
            status: "success",
            output: {
              path: ".",
              entries: [{ name: "a.txt", path: "a.txt", type: "file", size: 1 }],
              truncated: false,
            },
            summary: "1 entries",
          },
        },
      ],
      tools: [LIST_TOOL],
    };
    const { events } = await collect(request);
    expect(textOf(events)).toBe("Solaris inspected 1 workspace entries.");
  });

  it("requests workspace.read for `read README.md`", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "read README.md" }],
      tools: [READ_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-1",
      toolName: "workspace.read",
      input: { path: "README.md" },
    });
  });

  it("reports a final response after the read result", async () => {
    const request: ModelRequest = {
      messages: [
        { type: "user_message", content: "read README.md" },
        {
          type: "assistant_tool_call",
          callId: "call-1",
          toolName: "workspace.read",
          input: { path: "README.md" },
        },
        {
          type: "tool_result",
          callId: "call-1",
          toolName: "workspace.read",
          result: {
            status: "success",
            output: {
              path: "README.md",
              content: "hello",
              startLine: 1,
              endLine: 1,
              totalLines: 1,
              truncated: false,
            },
            summary: "1 lines",
          },
        },
      ],
      tools: [READ_TOOL],
    };
    const { events } = await collect(request);
    expect(textOf(events)).toBe("Solaris read README.md.");
  });

  it("requests workspace.search for `search <text>`", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "search modular monolith" }],
      tools: [SEARCH_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-1",
      toolName: "workspace.search",
      input: { query: "modular monolith", path: "." },
    });
  });

  it("reports a final response after the search result", async () => {
    const request: ModelRequest = {
      messages: [
        { type: "user_message", content: "search modular" },
        {
          type: "assistant_tool_call",
          callId: "call-1",
          toolName: "workspace.search",
          input: { query: "modular", path: "." },
        },
        {
          type: "tool_result",
          callId: "call-1",
          toolName: "workspace.search",
          result: {
            status: "success",
            output: {
              query: "modular",
              path: ".",
              matches: [{ path: "a.txt", line: 1, column: 1, text: "modular" }],
              scannedFiles: 1,
              skippedFiles: 0,
              truncated: false,
            },
            summary: "1 matches",
          },
        },
      ],
      tools: [SEARCH_TOOL],
    };
    const { events } = await collect(request);
    expect(textOf(events)).toBe("Solaris found 1 matching lines.");
  });

  it("does not request a tool that is not in the request definitions", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "list files" }],
      tools: [],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toBeUndefined();
    expect(textOf(events)).toBe("Solaris received: list files");
  });

  it("reports failed tool results truthfully", async () => {
    const request: ModelRequest = {
      messages: [
        { type: "user_message", content: "list files" },
        {
          type: "assistant_tool_call",
          callId: "call-1",
          toolName: "workspace.list",
          input: { path: "." },
        },
        {
          type: "tool_result",
          callId: "call-1",
          toolName: "workspace.list",
          result: { status: "denied", message: "Path is outside the Solaris workspace." },
        },
      ],
      tools: [LIST_TOOL],
    };
    const { events } = await collect(request);
    expect(textOf(events)).toBe(
      "Solaris could not complete the workspace operation: Path is outside the Solaris workspace.",
    );
  });

  it("retains text-only behaviour for arbitrary prompts", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "hello there" }],
      tools: [LIST_TOOL, READ_TOOL, SEARCH_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toBeUndefined();
    expect(textOf(events)).toBe("Solaris received: hello there");
  });
});
