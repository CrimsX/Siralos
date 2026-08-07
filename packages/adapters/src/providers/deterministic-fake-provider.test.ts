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

  it("does not reuse a previous turn's tool result for a new prompt", async () => {
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
            output: { path: ".", entries: [], truncated: false },
            summary: "0 entries",
          },
        },
        { type: "user_message", content: "read README.md" },
      ],
      tools: [LIST_TOOL, READ_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-1",
      toolName: "workspace.read",
      input: { path: "README.md" },
    });
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

describe("deterministic fake provider write scenarios", () => {
  const CREATE_TOOL: ToolDefinition = {
    name: "workspace.create_file",
    description: "Create one new UTF-8 text file inside an existing workspace directory.",
    inputSchema: {},
  };
  const EDIT_TOOL: ToolDefinition = {
    name: "workspace.edit_file",
    description: "Apply a bounded sequence of exact text replacements.",
    inputSchema: {},
  };
  const DELETE_TOOL: ToolDefinition = {
    name: "workspace.delete_file",
    description: "Delete one existing UTF-8 text file after explicit review.",
    inputSchema: {},
  };

  function readResultWithHash(hash: string): ConversationItem {
    return {
      type: "tool_result",
      callId: "call-read",
      toolName: "workspace.read",
      result: {
        status: "success",
        output: {
          path: "solaris-write-test.txt",
          sha256: hash,
          content: "Created by the deterministic Solaris test provider.\n",
          startLine: 1,
          endLine: 1,
          totalLines: 1,
          truncated: false,
        },
        summary: "1 lines",
      },
    };
  }

  it("requests the create tool for `create solaris-write-test`", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "create solaris-write-test" }],
      tools: [CREATE_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-create",
      toolName: "workspace.create_file",
      input: {
        path: "solaris-write-test.txt",
        content: "Created by the deterministic Solaris test provider.\n",
      },
    });
  });

  it("reports a truthful final text after a denied create", async () => {
    const request: ModelRequest = {
      messages: [
        { type: "user_message", content: "create solaris-write-test" },
        {
          type: "assistant_tool_call",
          callId: "call-create",
          toolName: "workspace.create_file",
          input: { path: "solaris-write-test.txt", content: "x\n" },
        },
        {
          type: "tool_result",
          callId: "call-create",
          toolName: "workspace.create_file",
          result: { status: "denied", message: "The change was denied by the user." },
        },
      ],
      tools: [CREATE_TOOL],
    };
    const { events } = await collect(request);
    expect(textOf(events)).toBe(
      "The workspace change was denied, so Solaris did not create solaris-write-test.txt.",
    );
  });

  it("reads the file before requesting the edit", async () => {
    const hash = "a".repeat(64);
    const request: ModelRequest = {
      messages: [
        { type: "user_message", content: "edit solaris-write-test" },
        readResultWithHash(hash),
      ],
      tools: [READ_TOOL, EDIT_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-edit",
      toolName: "workspace.edit_file",
      input: {
        path: "solaris-write-test.txt",
        expectedSha256: hash,
        replacements: [{ oldText: "Created", newText: "Updated" }],
      },
    });
  });

  it("requests the read first when no read result exists", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "edit solaris-write-test" }],
      tools: [READ_TOOL, EDIT_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-read",
      toolName: "workspace.read",
      input: { path: "solaris-write-test.txt" },
    });
  });

  it("reports a truthful final text after a denied edit", async () => {
    const request: ModelRequest = {
      messages: [
        { type: "user_message", content: "edit solaris-write-test" },
        readResultWithHash("a".repeat(64)),
        {
          type: "assistant_tool_call",
          callId: "call-edit",
          toolName: "workspace.edit_file",
          input: {
            path: "solaris-write-test.txt",
            expectedSha256: "a".repeat(64),
            replacements: [],
          },
        },
        {
          type: "tool_result",
          callId: "call-edit",
          toolName: "workspace.edit_file",
          result: { status: "denied", message: "The change was denied by the user." },
        },
      ],
      tools: [READ_TOOL, EDIT_TOOL],
    };
    const { events } = await collect(request);
    expect(textOf(events)).toBe(
      "The workspace change was denied, so Solaris did not modify solaris-write-test.txt.",
    );
  });

  it("reports a truthful final text after an edit conflict", async () => {
    const request: ModelRequest = {
      messages: [
        { type: "user_message", content: "edit solaris-write-test" },
        readResultWithHash("a".repeat(64)),
        {
          type: "assistant_tool_call",
          callId: "call-edit",
          toolName: "workspace.edit_file",
          input: {
            path: "solaris-write-test.txt",
            expectedSha256: "a".repeat(64),
            replacements: [],
          },
        },
        {
          type: "tool_result",
          callId: "call-edit",
          toolName: "workspace.edit_file",
          result: { status: "conflict", message: "The file changed; reread the file." },
        },
      ],
      tools: [READ_TOOL, EDIT_TOOL],
    };
    const { events } = await collect(request);
    expect(textOf(events)).toBe(
      "The file changed, so Solaris did not modify solaris-write-test.txt. Reread the file to continue.",
    );
  });

  it("reads before requesting the delete", async () => {
    const request: ModelRequest = {
      messages: [
        { type: "user_message", content: "delete solaris-write-test" },
        readResultWithHash("b".repeat(64)),
      ],
      tools: [READ_TOOL, DELETE_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-delete",
      toolName: "workspace.delete_file",
      input: { path: "solaris-write-test.txt", expectedSha256: "b".repeat(64) },
    });
  });

  it("does not request write tools that are not in the request definitions", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "create solaris-write-test" }],
      tools: [READ_TOOL, LIST_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toBeUndefined();
    expect(textOf(events)).toBe("Solaris received: create solaris-write-test");
  });
});

describe("deterministic fake provider git scenarios", () => {
  const GIT_STATUS_TOOL: ToolDefinition = {
    name: "git.status",
    description: "Show structured Git repository status.",
    inputSchema: {},
  };
  const GIT_DIFF_TOOL: ToolDefinition = {
    name: "git.diff",
    description: "Show a bounded Git diff.",
    inputSchema: {},
  };

  it("requests git.status for `git status`", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "git status" }],
      tools: [GIT_STATUS_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-git",
      toolName: "git.status",
      input: {},
    });
  });

  it("requests git.diff with the correct scope", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "show staged diff" }],
      tools: [GIT_DIFF_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-git",
      toolName: "git.diff",
      input: { scope: "staged" },
    });
  });

  it("summarizes status results truthfully", async () => {
    const request: ModelRequest = {
      messages: [
        { type: "user_message", content: "git status" },
        {
          type: "assistant_tool_call",
          callId: "call-git",
          toolName: "git.status",
          input: {},
        },
        {
          type: "tool_result",
          callId: "call-git",
          toolName: "git.status",
          result: {
            status: "success",
            output: {
              repository: true,
              branch: {
                head: "main",
                oid: null,
                upstream: null,
                ahead: null,
                behind: null,
                detached: false,
                unborn: false,
              },
              changes: [
                {
                  path: "a.txt",
                  originalPath: null,
                  indexStatus: "unmodified",
                  worktreeStatus: "modified",
                  kind: "ordinary",
                },
              ],
              conflicts: [],
              untracked: ["b.txt"],
              truncated: false,
            },
            summary: "1 changed files, 1 untracked",
          },
        },
      ],
      tools: [GIT_STATUS_TOOL],
    };
    const { events } = await collect(request);
    expect(textOf(events)).toBe("Solaris found 1 modified files and 1 untracked file.");
  });

  it("summarizes failed git results truthfully", async () => {
    const request: ModelRequest = {
      messages: [
        { type: "user_message", content: "git status" },
        {
          type: "assistant_tool_call",
          callId: "call-git",
          toolName: "git.status",
          input: {},
        },
        {
          type: "tool_result",
          callId: "call-git",
          toolName: "git.status",
          result: { status: "failed", message: "The workspace is not a Git repository." },
        },
      ],
      tools: [GIT_STATUS_TOOL],
    };
    const { events } = await collect(request);
    expect(textOf(events)).toBe(
      "Solaris could not inspect Git: The workspace is not a Git repository.",
    );
  });

  it("does not request git tools that are not registered", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "git status" }],
      tools: [LIST_TOOL, READ_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toBeUndefined();
    expect(textOf(events)).toBe("Solaris received: git status");
  });
});
