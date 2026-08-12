import { describe, expect, it } from "vitest";
import {
  isCancellationError,
  type ConversationItem,
  type ModelEvent,
  type ModelRequest,
  type ToolDefinition,
  type ToolExecutionResult,
} from "@siralos/core";
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

function hasLoneSurrogate(value: string): boolean {
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0;
    if (code >= 0xd800 && code <= 0xdfff) {
      return true;
    }
  }
  return false;
}

function encodeUtf8(value: string): Uint8Array {
  return new TextEncoder().encode(value);
}

function decodeUtf8(bytes: Uint8Array): string {
  return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
}

describe("deterministic fake provider unicode chunking", () => {
  const scenarios: readonly { name: string; text: string }[] = [
    { name: "emoji sequences", text: "step 1 👨‍👩‍👧‍👦 step 2 🚀 done" },
    { name: "combining characters", text: "café e\u0301tude n\u0303o \u1e9f" },
    { name: "mixed ascii and unicode", text: "a1\u00e9\u4e2d\u6587\u00f1B2\u20ac\u2026end" },
    { name: "astral plane", text: "😀😁😂🤣😃😄😅😆😉😊😋😎😍😘🥰😗" },
  ];
  for (const scenario of scenarios) {
    it(`chunks ${scenario.name} without splitting code points`, async () => {
      const { events, error } = await collect({
        messages: [{ type: "user_message", content: scenario.text }],
        tools,
      });
      expect(error).toBeUndefined();
      const joined = textOf(events);
      expect(joined).toBe(`Siralos received: ${scenario.text}`);
      const chunks = events
        .filter((event) => event.type === "text_delta")
        .map((event) => (event as { text: string }).text);
      expect(chunks.length).toBeGreaterThan(1);
      for (const chunk of chunks) {
        expect(hasLoneSurrogate(chunk)).toBe(false);
      }
      for (const chunk of chunks) {
        const encoded = encodeUtf8(chunk);
        const decoded = decodeUtf8(encoded);
        expect(decoded).toBe(chunk);
      }
    });
  }

  it("preserves deterministic chunking for identical input", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "café \u00e9tude 😀" }],
      tools,
    };
    const first = (await collect(request)).events.filter((event) => event.type === "text_delta");
    const second = (await collect(request)).events.filter((event) => event.type === "text_delta");
    expect(first).toEqual(second);
  });

  it("keeps each chunk independently encoded and valid", async () => {
    const text = "a\u00e9b\u4e2dc\u00f1d\u2026e".repeat(3);
    const { events, error } = await collect({
      messages: [{ type: "user_message", content: text }],
      tools,
    });
    expect(error).toBeUndefined();
    const chunks = events
      .filter((event) => event.type === "text_delta")
      .map((event) => (event as { text: string }).text);
    expect(chunks.every((chunk) => chunk.length > 0)).toBe(true);
    const recombined = chunks.map(encodeUtf8).reduce((all, bytes) => {
      const merged = new Uint8Array(all.length + bytes.length);
      merged.set(all, 0);
      merged.set(bytes, all.length);
      return merged;
    }, new Uint8Array(0));
    expect(decodeUtf8(recombined)).toBe(`Siralos received: ${text}`);
  });
});

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
    expect(textOf(events)).toBe("Siralos received: hello");
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
    expect(textOf(events)).toBe("Siralos inspected 1 workspace entries.");
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
    expect(textOf(events)).toBe("Siralos read README.md.");
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
    expect(textOf(events)).toBe("Siralos found 1 matching lines.");
  });

  it("does not request a tool that is not in the request definitions", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "list files" }],
      tools: [],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toBeUndefined();
    expect(textOf(events)).toBe("Siralos received: list files");
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
          result: { status: "denied", message: "Path is outside the Siralos workspace." },
        },
      ],
      tools: [LIST_TOOL],
    };
    const { events } = await collect(request);
    expect(textOf(events)).toBe(
      "Siralos could not complete the workspace operation: Path is outside the Siralos workspace.",
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
    expect(textOf(events)).toBe("Siralos received: hello there");
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
          path: "siralos-write-test.txt",
          sha256: hash,
          content: "Created by the deterministic Siralos test provider.\n",
          startLine: 1,
          endLine: 1,
          totalLines: 1,
          truncated: false,
        },
        summary: "1 lines",
      },
    };
  }

  it("requests the create tool for `create siralos-write-test`", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "create siralos-write-test" }],
      tools: [CREATE_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-create",
      toolName: "workspace.create_file",
      input: {
        path: "siralos-write-test.txt",
        content: "Created by the deterministic Siralos test provider.\n",
      },
    });
  });

  it("reports a truthful final text after a denied create", async () => {
    const request: ModelRequest = {
      messages: [
        { type: "user_message", content: "create siralos-write-test" },
        {
          type: "assistant_tool_call",
          callId: "call-create",
          toolName: "workspace.create_file",
          input: { path: "siralos-write-test.txt", content: "x\n" },
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
      "The workspace change was denied, so Siralos did not create siralos-write-test.txt.",
    );
  });

  it("reads the file before requesting the edit", async () => {
    const hash = "a".repeat(64);
    const request: ModelRequest = {
      messages: [
        { type: "user_message", content: "edit siralos-write-test" },
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
        path: "siralos-write-test.txt",
        expectedSha256: hash,
        replacements: [{ oldText: "Created", newText: "Updated" }],
      },
    });
  });

  it("requests the read first when no read result exists", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "edit siralos-write-test" }],
      tools: [READ_TOOL, EDIT_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-read",
      toolName: "workspace.read",
      input: { path: "siralos-write-test.txt" },
    });
  });

  it("reports a truthful final text after a denied edit", async () => {
    const request: ModelRequest = {
      messages: [
        { type: "user_message", content: "edit siralos-write-test" },
        readResultWithHash("a".repeat(64)),
        {
          type: "assistant_tool_call",
          callId: "call-edit",
          toolName: "workspace.edit_file",
          input: {
            path: "siralos-write-test.txt",
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
      "The workspace change was denied, so Siralos did not modify siralos-write-test.txt.",
    );
  });

  it("reports a truthful final text after an edit conflict", async () => {
    const request: ModelRequest = {
      messages: [
        { type: "user_message", content: "edit siralos-write-test" },
        readResultWithHash("a".repeat(64)),
        {
          type: "assistant_tool_call",
          callId: "call-edit",
          toolName: "workspace.edit_file",
          input: {
            path: "siralos-write-test.txt",
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
      "The file changed, so Siralos did not modify siralos-write-test.txt. Reread the file to continue.",
    );
  });

  it("reads before requesting the delete", async () => {
    const request: ModelRequest = {
      messages: [
        { type: "user_message", content: "delete siralos-write-test" },
        readResultWithHash("b".repeat(64)),
      ],
      tools: [READ_TOOL, DELETE_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-delete",
      toolName: "workspace.delete_file",
      input: { path: "siralos-write-test.txt", expectedSha256: "b".repeat(64) },
    });
  });

  it("does not request write tools that are not in the request definitions", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "create siralos-write-test" }],
      tools: [READ_TOOL, LIST_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toBeUndefined();
    expect(textOf(events)).toBe("Siralos received: create siralos-write-test");
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
    expect(textOf(events)).toBe("Siralos found 1 modified files and 1 untracked file.");
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
      "Siralos could not inspect Git: The workspace is not a Git repository.",
    );
  });

  it("does not request git tools that are not registered", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "git status" }],
      tools: [LIST_TOOL, READ_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toBeUndefined();
    expect(textOf(events)).toBe("Siralos received: git status");
  });
});

describe("deterministic fake provider development scenarios", () => {
  const READ_TOOL: ToolDefinition = {
    name: "workspace.read",
    description: "Read one text file inside the workspace.",
    inputSchema: {},
  };
  const CHANGESET_TOOL: ToolDefinition = {
    name: "workspace.apply_text_changeset",
    description: "Propose one exact text change set inside the development workflow.",
    inputSchema: {},
  };
  const FIXTURE_HASH = "a".repeat(64);

  function applySuccessOutput(): import("@siralos/core").JsonValue {
    return {
      status: "completed",
      iterations: 1,
      changedFiles: [
        {
          path: "scripts/player/player.gd",
          operation: "update",
          beforeSha256: FIXTURE_HASH,
          afterSha256: "b".repeat(64),
        },
      ],
      diagnostics: { errors: 0, warnings: 0 },
      validation: { parser: true, lsp: true, workspaceIntegrity: true },
      checkpointIds: ["cp_test"],
    };
  }

  it("reads the fixture, proposes an exact edit, and summarizes the applied change", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "develop fixture" }],
      tools: [READ_TOOL, CHANGESET_TOOL],
    };
    const first = await collect(request);
    expect(toolCallEvent(first.events)).toMatchObject({
      type: "tool_call",
      callId: "call-dev-read",
      toolName: "workspace.read",
      input: { path: "scripts/player/player.gd" },
    });
    const second = await collect({
      messages: [
        { type: "user_message", content: "develop fixture" },
        {
          type: "assistant_tool_call",
          callId: "call-dev-read",
          toolName: "workspace.read",
          input: { path: "scripts/player/player.gd" },
        },
        {
          type: "tool_result",
          callId: "call-dev-read",
          toolName: "workspace.read",
          result: {
            status: "success",
            output: {
              path: "scripts/player/player.gd",
              sha256: FIXTURE_HASH,
              content:
                "extends CharacterBody2D\n\nfunc _physics_process(delta):\n\tmove_and_slide()\n",
              startLine: 1,
              endLine: 4,
              totalLines: 4,
              truncated: false,
            },
            summary: "4 lines",
          },
        },
      ],
      tools: [READ_TOOL, CHANGESET_TOOL],
    });
    expect(toolCallEvent(second.events)).toMatchObject({
      type: "tool_call",
      callId: "call-dev-change",
      toolName: "workspace.apply_text_changeset",
      input: {
        changes: [
          {
            operation: "edit",
            path: "scripts/player/player.gd",
            expectedSha256: FIXTURE_HASH,
            replacements: [{ oldText: "move_and_slide()", newText: "move_and_slide(Vector2.UP)" }],
          },
        ],
      },
    });
    const third = await collect({
      messages: [
        { type: "user_message", content: "develop fixture" },
        {
          type: "assistant_tool_call",
          callId: "call-dev-read",
          toolName: "workspace.read",
          input: { path: "scripts/player/player.gd" },
        },
        {
          type: "tool_result",
          callId: "call-dev-read",
          toolName: "workspace.read",
          result: {
            status: "success",
            output: {
              path: "scripts/player/player.gd",
              sha256: FIXTURE_HASH,
              content: "",
              startLine: 1,
              endLine: 1,
              totalLines: 1,
              truncated: false,
            },
            summary: "1 lines",
          },
        },
        {
          type: "assistant_tool_call",
          callId: "call-dev-change",
          toolName: "workspace.apply_text_changeset",
          input: { changes: [] },
        },
        {
          type: "tool_result",
          callId: "call-dev-change",
          toolName: "workspace.apply_text_changeset",
          result: {
            status: "success",
            output: applySuccessOutput(),
            summary: "change set applied",
          },
        },
      ],
      tools: [READ_TOOL, CHANGESET_TOOL],
    });
    expect(textOf(third.events)).toContain("Siralos applied the approved change set");
    expect(textOf(third.events)).toContain("parser passed");
  });

  it("proposes a broken first edit, then a repair, in the with-repair scenario", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "develop fixture with repair" }],
      tools: [READ_TOOL, CHANGESET_TOOL],
    };
    const first = await collect(request);
    expect(toolCallEvent(first.events)).toMatchObject({
      type: "tool_call",
      callId: "call-dev-read",
      toolName: "workspace.read",
    });
    const readItems: ConversationItem[] = [
      {
        type: "tool_result",
        callId: "call-dev-read",
        toolName: "workspace.read",
        result: {
          status: "success",
          output: {
            path: "scripts/player/player.gd",
            sha256: FIXTURE_HASH,
            content:
              "extends CharacterBody2D\n\nfunc _physics_process(delta):\n\tmove_and_slide()\n",
            startLine: 1,
            endLine: 4,
            totalLines: 4,
            truncated: false,
          },
          summary: "4 lines",
        },
      },
    ];
    const second = await collect({
      messages: [{ type: "user_message", content: "develop fixture with repair" }, ...readItems],
      tools: [READ_TOOL, CHANGESET_TOOL],
    });
    const firstChange = toolCallEvent(second.events);
    expect(firstChange).toMatchObject({
      type: "tool_call",
      callId: "call-dev-change",
      toolName: "workspace.apply_text_changeset",
    });
    expect(firstChange).not.toBeNull();
    const firstInput = (firstChange as { input: unknown }).input as {
      changes: readonly { replacements: readonly { newText: string }[] }[];
    };
    expect(firstInput.changes[0]?.replacements[0]?.newText).toBe("move_and_slide())");
    // After the first apply (with parser errors), a repair is proposed.
    const third = await collect({
      messages: [
        { type: "user_message", content: "develop fixture with repair" },
        ...readItems,
        {
          type: "assistant_tool_call",
          callId: "call-dev-change",
          toolName: "workspace.apply_text_changeset",
          input: { changes: [] },
        },
        {
          type: "tool_result",
          callId: "call-dev-change",
          toolName: "workspace.apply_text_changeset",
          result: {
            status: "success",
            output: {
              ...(applySuccessOutput() as Record<string, unknown>),
              diagnostics: { errors: 1, warnings: 0 },
              changedFiles: [
                {
                  path: "scripts/player/player.gd",
                  operation: "update",
                  beforeSha256: FIXTURE_HASH,
                  afterSha256: "c".repeat(64),
                },
              ],
            },
            summary: "change set applied with errors",
          },
        },
      ],
      tools: [READ_TOOL, CHANGESET_TOOL],
    });
    const repair = toolCallEvent(third.events);
    expect(repair).toMatchObject({
      type: "tool_call",
      callId: "call-dev-repair",
      toolName: "workspace.apply_text_changeset",
    });
    expect(repair).not.toBeNull();
    const repairInput = (repair as { input: unknown }).input as {
      changes: readonly { replacements: readonly { oldText: string; newText: string }[] }[];
    };
    expect(repairInput.changes[0]?.replacements[0]?.oldText).toBe("move_and_slide())");
    expect(repairInput.changes[0]?.replacements[0]?.newText).toBe("move_and_slide(Vector2.UP)");
  });

  it("reports the truthful unavailable outcome when the change-set tool is missing", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "develop fixture" }],
      tools: [READ_TOOL],
    };
    const { events } = await collect(request);
    expect(textOf(events)).toContain("cannot propose source changes");
  });
});

describe("deterministic fake provider command scenarios", () => {
  const PROCESS_TOOL: ToolDefinition = {
    name: "process.run",
    description: "Run a validated Siralos development command.",
    inputSchema: {},
  };

  function commandRequest(prompt: string, result?: ToolExecutionResult): ModelRequest {
    const items: ConversationItem[] = [{ type: "user_message", content: prompt }];
    if (result !== undefined) {
      items.push({
        type: "assistant_tool_call",
        callId: "call-command",
        toolName: "process.run",
        input: {},
      });
      items.push({
        type: "tool_result",
        callId: "call-command",
        toolName: "process.run",
        result,
      });
    }
    return { messages: items, tools: [PROCESS_TOOL] };
  }

  it("requests npm check through the npm-script runner", async () => {
    const { events } = await collect(commandRequest("run npm check"));
    expect(toolCallEvent(events)).toMatchObject({
      type: "tool_call",
      toolName: "process.run",
      input: {
        runner: "npm-script",
        script: "check",
        arguments: [],
        workingDirectory: ".",
      },
    });
  });

  it("requests npm test through the npm-script runner", async () => {
    const { events } = await collect(commandRequest("run npm test"));
    expect(toolCallEvent(events)).toMatchObject({
      type: "tool_call",
      toolName: "process.run",
      input: {
        runner: "npm-script",
        script: "test",
        arguments: [],
        workingDirectory: ".",
      },
    });
  });

  it("requests the validation fixture through the node-script runner", async () => {
    const { events } = await collect(commandRequest("run node validation fixture"));
    expect(toolCallEvent(events)).toMatchObject({
      type: "tool_call",
      toolName: "process.run",
      input: {
        runner: "node-script",
        path: "scripts/process-validation-fixture.mjs",
        arguments: [],
        workingDirectory: ".",
      },
    });
  });

  it("does not request process.run when the tool is unavailable", async () => {
    const request: ModelRequest = {
      messages: [{ type: "user_message", content: "run npm check" }],
      tools: [LIST_TOOL],
    };
    const { events } = await collect(request);
    expect(toolCallEvent(events)).toBeUndefined();
    expect(textOf(events)).toContain("cannot run development commands");
  });

  it("summarizes a successful command truthfully", async () => {
    const { events } = await collect(
      commandRequest("run npm check", {
        status: "success",
        output: { status: "completed", exitCode: 0 },
        summary: "ok",
      }),
    );
    expect(textOf(events)).toBe("Siralos ran `npm run check` and it exited with code 0.");
  });

  it("summarizes a nonzero exit truthfully without treating it as infrastructure failure", async () => {
    const { events } = await collect(
      commandRequest("run npm check", {
        status: "success",
        output: { status: "completed", exitCode: 2 },
        summary: "ok",
      }),
    );
    expect(textOf(events)).toBe("Siralos ran `npm run check`, but it exited with code 2.");
  });

  it("summarizes denial truthfully", async () => {
    const { events } = await collect(
      commandRequest("run npm check", { status: "denied", message: "denied" }),
    );
    expect(textOf(events)).toBe("The command was not approved, so Siralos did not run it.");
  });

  it("summarizes cancellation and timeout truthfully", async () => {
    const cancelled = await collect(
      commandRequest("run npm check", { status: "cancelled", message: "cancelled" }),
    );
    expect(textOf(cancelled.events)).toBe("The command was cancelled before it completed.");
    const timedOut = await collect(
      commandRequest("run npm check", { status: "timed_out", message: "timed out" }),
    );
    expect(textOf(timedOut.events)).toBe(
      "The command timed out and its process tree was terminated.",
    );
  });

  it("summarizes sandbox unavailability truthfully", async () => {
    const { events } = await collect(
      commandRequest("run npm check", { status: "sandbox_unavailable", message: "no sandbox" }),
    );
    expect(textOf(events)).toBe("The sandbox is unavailable, so the command did not run.");
  });

  it("summarizes workspace violations truthfully", async () => {
    const { events } = await collect(
      commandRequest("run npm check", { status: "workspace_violation", message: "violation" }),
    );
    expect(textOf(events)).toBe(
      "Siralos detected unexpected workspace changes; command execution is disabled for this session.",
    );
  });
});

describe("deterministic fake provider godot scenarios", () => {
  const ENGINE_TOOL: ToolDefinition = {
    name: "godot.inspect_engine",
    description: "Inspect the selected Godot installation.",
    inputSchema: {},
  };
  const PROJECT_TOOL: ToolDefinition = {
    name: "godot.inspect_project",
    description: "Statically inspect the Godot project.",
    inputSchema: {},
  };
  const PROBE_TOOL: ToolDefinition = {
    name: "godot.probe_project",
    description: "Recovery-mode Godot project probe.",
    inputSchema: {},
  };

  it("requests godot.probe_project for `probe godot project`", async () => {
    const { events } = await collect({
      messages: [{ type: "user_message", content: "probe godot project" }],
      tools: [PROBE_TOOL],
    });
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-godot",
      toolName: "godot.probe_project",
      input: {},
    });
  });

  it("requests godot.probe_project for `run godot project probe`", async () => {
    const { events } = await collect({
      messages: [{ type: "user_message", content: "run godot project probe" }],
      tools: [PROBE_TOOL],
    });
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-godot",
      toolName: "godot.probe_project",
      input: {},
    });
  });

  it("summarizes a completed probe result truthfully", async () => {
    const { events: probeEvents } = await collect({
      messages: [
        { type: "user_message", content: "probe godot project" },
        {
          type: "assistant_tool_call",
          callId: "call-godot",
          toolName: "godot.probe_project",
          input: {},
        },
        {
          type: "tool_result",
          callId: "call-godot",
          toolName: "godot.probe_project",
          result: {
            status: "success",
            output: {
              status: "completed",
              engine: {
                installationId: "path-1",
                version: "4.7.1.stable.official",
                executableFingerprint: "abc",
              },
              recoveryMode: true,
              sourceWorkspaceLoaded: false,
              mirror: {
                sourceFiles: 3,
                sourceBytes: 42,
                generatedGodotDirectory: true,
                generatedBytes: 10,
                generatedFiles: 2,
                importState: "imports observed",
              },
              diagnostics: {
                errors: [],
                warnings: [{ severity: "warning", category: "import", message: "import warning" }],
                truncated: false,
              },
              process: { exitCode: 0, durationMs: 100, timedOut: false },
              workspaceIntegrity: { unchanged: true, bounded: false },
              cleanup: { completed: true },
            },
            summary: "probe completed",
          },
        },
      ],
      tools: [PROBE_TOOL],
    });
    expect(textOf(probeEvents)).toContain("recovery-mode Godot project probe");
    expect(textOf(probeEvents)).toContain("4.7.1.stable.official");
    expect(textOf(probeEvents)).toContain("completed");
    expect(textOf(probeEvents)).toContain("1 warning");
    expect(textOf(probeEvents)).toContain("source workspace was not loaded");
    expect(textOf(probeEvents)).toContain("was unchanged");
    expect(textOf(probeEvents)).toContain("removed");
  });

  it("summarizes an unavailable probe without claiming execution", async () => {
    const { events: probeEvents } = await collect({
      messages: [
        { type: "user_message", content: "probe godot project" },
        {
          type: "assistant_tool_call",
          callId: "call-godot",
          toolName: "godot.probe_project",
          input: {},
        },
        {
          type: "tool_result",
          callId: "call-godot",
          toolName: "godot.probe_project",
          result: {
            status: "unavailable",
            message: "Recovery-mode project probing is unavailable on this platform.",
          },
        },
      ],
      tools: [PROBE_TOOL],
    });
    expect(textOf(probeEvents)).toContain("could not probe");
    expect(textOf(probeEvents)).toContain("unavailable");
  });

  it("summarizes a denied probe without claiming execution", async () => {
    const { events: probeEvents } = await collect({
      messages: [
        { type: "user_message", content: "probe godot project" },
        {
          type: "assistant_tool_call",
          callId: "call-godot",
          toolName: "godot.probe_project",
          input: {},
        },
        {
          type: "tool_result",
          callId: "call-godot",
          toolName: "godot.probe_project",
          result: { status: "denied", message: "The project probe was denied." },
        },
      ],
      tools: [PROBE_TOOL],
    });
    expect(textOf(probeEvents)).toContain("not approved");
  });

  it("explains when the probe tool is unavailable in the profile", async () => {
    const { events: probeEvents } = await collect({
      messages: [{ type: "user_message", content: "probe godot project" }],
      tools: [],
    });
    expect(textOf(probeEvents)).toContain("godot.probe_project is unavailable");
  });

  it("requests godot.inspect_engine for `inspect godot`", async () => {
    const { events } = await collect({
      messages: [{ type: "user_message", content: "inspect godot" }],
      tools: [ENGINE_TOOL],
    });
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-godot",
      toolName: "godot.inspect_engine",
      input: {},
    });
  });

  it("requests godot.inspect_engine for `inspect godot engine`", async () => {
    const { events } = await collect({
      messages: [{ type: "user_message", content: "inspect godot engine" }],
      tools: [ENGINE_TOOL],
    });
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-godot",
      toolName: "godot.inspect_engine",
      input: {},
    });
  });

  it("requests godot.inspect_project for `inspect godot project`", async () => {
    const { events } = await collect({
      messages: [{ type: "user_message", content: "inspect godot project" }],
      tools: [PROJECT_TOOL],
    });
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-godot",
      toolName: "godot.inspect_project",
      input: {},
    });
  });

  it("requests godot.inspect_project for compatibility questions", async () => {
    const { events } = await collect({
      messages: [{ type: "user_message", content: "is this project compatible with godot" }],
      tools: [PROJECT_TOOL],
    });
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-godot",
      toolName: "godot.inspect_project",
      input: {},
    });
  });

  it("summarizes a missing installation truthfully", async () => {
    const { events } = await collect({
      messages: [
        { type: "user_message", content: "inspect godot" },
        {
          type: "assistant_tool_call",
          callId: "call-godot",
          toolName: "godot.inspect_engine",
          input: {},
        },
        {
          type: "tool_result",
          callId: "call-godot",
          toolName: "godot.inspect_engine",
          result: {
            status: "success",
            output: {
              selected: false,
              version: null,
              edition: null,
              releaseChannel: null,
              support: null,
            },
            summary: "No Godot installation is selected.",
          },
        },
      ],
      tools: [ENGINE_TOOL],
    });
    expect(textOf(events)).toContain("No Godot installation is selected");
  });

  it("summarizes a non-Godot workspace truthfully and states the inspection was static", async () => {
    const { events } = await collect({
      messages: [
        { type: "user_message", content: "is this project compatible with godot" },
        {
          type: "assistant_tool_call",
          callId: "call-godot",
          toolName: "godot.inspect_project",
          input: {},
        },
        {
          type: "tool_result",
          callId: "call-godot",
          toolName: "godot.inspect_project",
          result: {
            status: "success",
            output: {
              detected: false,
              name: null,
              compatibility: { status: "no-project", severity: "info", reasons: [] },
            },
            summary: "No Godot project detected at the workspace root.",
          },
        },
      ],
      tools: [PROJECT_TOOL],
    });
    expect(textOf(events)).toContain("no Godot project");
  });

  it("reports unverified versions accurately", async () => {
    const { events } = await collect({
      messages: [
        { type: "user_message", content: "inspect godot" },
        {
          type: "assistant_tool_call",
          callId: "call-godot",
          toolName: "godot.inspect_engine",
          input: {},
        },
        {
          type: "tool_result",
          callId: "call-godot",
          toolName: "godot.inspect_engine",
          result: {
            status: "success",
            output: {
              selected: true,
              installationId: "rc",
              version: "4.7.2.rc1.official",
              edition: "standard",
              support: "prerelease-untested",
              verifiedCapabilities: ["version", "help"],
            },
            summary: "4.7.2.rc1.official (standard, prerelease-untested)",
          },
        },
      ],
      tools: [ENGINE_TOOL],
    });
    expect(textOf(events)).toContain("prerelease-untested");
  });

  it("does not request project execution", async () => {
    const { events } = await collect({
      messages: [{ type: "user_message", content: "inspect godot project" }],
      tools: [PROJECT_TOOL],
    });
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-godot",
      toolName: "godot.inspect_project",
      input: {},
    });
  });

  it("summarizes tool unavailability when Godot tools are denied", async () => {
    const { events } = await collect({
      messages: [{ type: "user_message", content: "inspect godot" }],
      tools: [],
    });
    expect(textOf(events)).toContain("Godot inspection tools are unavailable");
  });
});

describe("deterministic fake provider Godot API and diagnostics scenarios", () => {
  const API_SEARCH_TOOL: ToolDefinition = {
    name: "godot.api_search",
    description: "search",
    inputSchema: {
      type: "object",
      properties: { query: { type: "string" } },
      required: ["query"],
    },
  };
  const CHECK_TOOL: ToolDefinition = {
    name: "godot.check_script",
    description: "check",
    inputSchema: { type: "object", properties: { path: { type: "string" } }, required: ["path"] },
  };

  it("requests godot.api_search for `search godot api`", async () => {
    const { events } = await collect({
      messages: [{ type: "user_message", content: "search godot api" }],
      tools: [API_SEARCH_TOOL],
    });
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-godot",
      toolName: "godot.api_search",
      input: { query: "Node owner" },
    });
  });

  it("passes the query text after `search godot api`", async () => {
    const { events } = await collect({
      messages: [
        { type: "user_message", content: "search godot api CharacterBody2D move_and_slide" },
      ],
      tools: [API_SEARCH_TOOL],
    });
    const call = toolCallEvent(events);
    if (call?.type === "tool_call") {
      expect(call.input).toEqual({ query: "CharacterBody2D move_and_slide" });
    } else {
      expect.fail("expected a tool_call event");
    }
  });

  it("summarizes API search results with the exact engine version", async () => {
    const { events } = await collect({
      messages: [
        { type: "user_message", content: "search godot api" },
        {
          type: "assistant_tool_call",
          callId: "call-godot",
          toolName: "godot.api_search",
          input: { query: "Node owner" },
        },
        {
          type: "tool_result",
          callId: "call-godot",
          toolName: "godot.api_search",
          result: {
            status: "success",
            output: {
              engineVersion: "4.7.1.stable.official",
              results: [
                {
                  symbol: "class:Node/property:owner",
                  kind: "property",
                  name: "owner",
                  owner: "Node",
                  summary: "The owner of this node.",
                  rank: "exact",
                  apiType: "native",
                },
              ],
              truncated: false,
            },
            summary: "1 result",
          },
        },
      ],
      tools: [API_SEARCH_TOOL],
    });
    const text = textOf(events);
    expect(text).toContain("1 API result");
    expect(text).toContain("4.7.1.stable.official");
    expect(text).toContain("Node.owner");
  });

  it("reports an unavailable API search truthfully", async () => {
    const { events } = await collect({
      messages: [
        { type: "user_message", content: "search godot api" },
        {
          type: "assistant_tool_call",
          callId: "call-godot",
          toolName: "godot.api_search",
          input: { query: "Node owner" },
        },
        {
          type: "tool_result",
          callId: "call-godot",
          toolName: "godot.api_search",
          result: {
            status: "unavailable",
            message: "No Godot API knowledge is loaded.",
          },
        },
      ],
      tools: [API_SEARCH_TOOL],
    });
    expect(textOf(events)).toContain("cannot search the Godot API");
  });

  it("requests godot.check_script for `check godot script`", async () => {
    const { events } = await collect({
      messages: [{ type: "user_message", content: "check godot script" }],
      tools: [CHECK_TOOL],
    });
    expect(toolCallEvent(events)).toEqual({
      type: "tool_call",
      callId: "call-godot",
      toolName: "godot.check_script",
      input: { path: "src/player/player.gd" },
    });
  });

  it("summarizes a parser-error check result as a diagnostic result, not an infrastructure failure", async () => {
    const { events } = await collect({
      messages: [
        { type: "user_message", content: "check godot script" },
        {
          type: "assistant_tool_call",
          callId: "call-godot",
          toolName: "godot.check_script",
          input: { path: "src/player/player.gd" },
        },
        {
          type: "tool_result",
          callId: "call-godot",
          toolName: "godot.check_script",
          result: {
            status: "success",
            output: {
              engineVersion: "4.7.1.stable.official",
              checked: true,
              valid: false,
              scriptsChecked: 1,
              validCount: 0,
              invalidCount: 1,
              diagnostics: [
                {
                  source: "godot-check-only",
                  severity: "error",
                  path: "src/player/player.gd",
                  line: 34,
                  column: 17,
                  code: "undeclared-identifier",
                  message: 'Identifier "velocityy" not declared in the current scope.',
                  rawCategory: "error",
                },
              ],
              truncated: false,
            },
            summary: "invalid",
          },
        },
      ],
      tools: [CHECK_TOOL],
    });
    const text = textOf(events);
    expect(text).toContain("--check-only");
    expect(text).toContain("1 invalid script");
    expect(text).toContain("1 normalized diagnostic");
    expect(text).toContain("No game code was executed.");
    expect(text).not.toContain("could not run");
  });

  it("summarizes a denied check without claiming execution", async () => {
    const { events } = await collect({
      messages: [
        { type: "user_message", content: "check godot script" },
        {
          type: "assistant_tool_call",
          callId: "call-godot",
          toolName: "godot.check_script",
          input: { path: "src/player/player.gd" },
        },
        {
          type: "tool_result",
          callId: "call-godot",
          toolName: "godot.check_script",
          result: { status: "denied", message: "not approved" },
        },
      ],
      tools: [CHECK_TOOL],
    });
    expect(textOf(events)).toContain("was not approved");
    expect(textOf(events)).not.toContain("checked");
  });

  it("summarizes an unavailable check truthfully", async () => {
    const { events } = await collect({
      messages: [
        { type: "user_message", content: "check godot script" },
        {
          type: "assistant_tool_call",
          callId: "call-godot",
          toolName: "godot.check_script",
          input: { path: "src/player/player.gd" },
        },
        {
          type: "tool_result",
          callId: "call-godot",
          toolName: "godot.check_script",
          result: { status: "unavailable", message: "execution gate refuses" },
        },
      ],
      tools: [CHECK_TOOL],
    });
    expect(textOf(events)).toContain("could not run the GDScript check");
  });
});
