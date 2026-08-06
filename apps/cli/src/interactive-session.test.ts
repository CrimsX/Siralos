import { describe, expect, it } from "vitest";
import {
  createSolarisApplication,
  createToolRegistry,
  type ModelEvent,
  type ModelProvider,
  type Tool,
  type ToolExecutionResult,
} from "@solaris/core";
import { createCliApplication } from "./bootstrap/create-application.js";
import { runInteractiveSession, type SessionIO, type SessionInfo } from "./interactive-session.js";

class ScriptedIO implements SessionIO {
  private readonly lines: readonly string[];
  private index = 0;
  private readonly chunks: string[] = [];

  constructor(lines: readonly string[]) {
    this.lines = lines;
  }

  ask(_prompt: string): Promise<string | null> {
    if (this.index >= this.lines.length) {
      return Promise.resolve(null);
    }
    const line = this.lines[this.index];
    this.index += 1;
    return Promise.resolve(line === undefined ? null : line);
  }

  write(text: string): void {
    this.chunks.push(text);
  }

  clear(): void {
    this.chunks.push("[clear]");
  }

  get text(): string {
    return this.chunks.join("");
  }
}

async function createComposedSession(lines: readonly string[]) {
  const io = new ScriptedIO(lines);
  const { application, workspaceRoot, tools } = await createCliApplication();
  const sessionInfo: SessionInfo = { workspaceRoot, tools };
  return { io, application, sessionInfo };
}

describe("runInteractiveSession", () => {
  it("submits a prompt and renders the streamed response", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["hello", "/exit"]);
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("Solaris received: hello");
  });

  it("preserves conversation history across prompts", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "first",
      "second",
      "/exit",
    ]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Solaris received: first");
    expect(io.text).toContain("Solaris received: second");
  });

  it("ignores empty input", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["", "   ", "/exit"]);
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).not.toContain("Solaris received: ");
  });

  it("reports an invalid slash command", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["/bogus", "/exit"]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Unknown command: /bogus");
    expect(io.text).not.toContain("Solaris received:");
  });

  it("exits cleanly on end of input", async () => {
    const { io, application, sessionInfo } = await createComposedSession([]);
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
  });

  it("renders help and status", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "/help",
      "/status",
      "/exit",
    ]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Available commands");
    expect(io.text).toContain("Provider: deterministic-fake");
    expect(io.text).toContain("Messages: 0");
  });

  it("clears the terminal without clearing conversation history", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "hello",
      "/clear",
      "/status",
      "/exit",
    ]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("[clear]");
    expect(io.text).toContain("Messages: 2");
  });

  it("renders a provider failure and keeps the session alive", async () => {
    const failingProvider: ModelProvider = {
      id: "failing-stub",
      stream(): AsyncIterable<ModelEvent> {
        throw new Error("provider exploded");
      },
    };
    const application = createSolarisApplication({
      provider: failingProvider,
      tools: createToolRegistry([]),
    });
    const io = new ScriptedIO(["hello", "/exit"]);
    const sessionInfo: SessionInfo = { workspaceRoot: "/workspace", tools: [] };
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("provider exploded");
  });
});

describe("runInteractiveSession tool activity", () => {
  it("lists the registered tools for /tools", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["/tools", "/exit"]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Available tools");
    expect(io.text).toContain("workspace.list");
    expect(io.text).toContain("workspace.read");
    expect(io.text).toContain("workspace.search");
    expect(io.text).toContain("(read-only)");
  });

  it("includes the workspace and tool count in /status", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["/status", "/exit"]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Workspace:");
    expect(io.text).toContain("Tools: 3");
  });

  it("renders list-files tool activity and a final response", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["list files", "/exit"]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain('\u25CF workspace.list {"path":"."}');
    expect(io.text).toMatch(/^\s+\d+ entries/m);
    expect(io.text).toMatch(/Solaris inspected \d+ workspace entries\./);
  });

  it("renders read activity without exposing raw file contents", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "read README.md",
      "/exit",
    ]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain('\u25CF workspace.read {"path":"README.md"}');
    expect(io.text).toContain("Solaris read README.md.");
    expect(io.text).not.toContain("interactive agent harness for programming");
  });

  it("renders search activity with the match count", async () => {
    const { io, application, sessionInfo } = await createComposedSession([
      "search Solaris",
      "/exit",
    ]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain('\u25CF workspace.search {"query":"Solaris","path":"."}');
    expect(io.text).toMatch(/Solaris found \d+ matching lines\./);
  });

  it("renders a tool failure and returns to the prompt", async () => {
    let turn = 0;
    const provider: ModelProvider = {
      id: "tool-failure-stub",
      async *stream(): AsyncIterable<ModelEvent> {
        turn += 1;
        if (turn === 1) {
          yield {
            type: "tool_call",
            callId: "call-1",
            toolName: "exploding.tool",
            input: { path: "." },
          };
          await Promise.resolve();
          return;
        }
        yield { type: "text_delta", text: "recovered" };
        await Promise.resolve();
        yield { type: "completed" };
      },
    };
    const tool: Tool = {
      definition: { name: "exploding.tool", description: "Fails", inputSchema: {} },
      execute(): Promise<ToolExecutionResult> {
        return Promise.resolve({
          status: "denied",
          message: "Path is outside the Solaris workspace.",
        });
      },
    };
    const application = createSolarisApplication({
      provider,
      tools: createToolRegistry([tool]),
    });
    const io = new ScriptedIO(["hello", "/status", "/exit"]);
    const sessionInfo: SessionInfo = { workspaceRoot: "/workspace", tools: [tool.definition] };
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("\u2715 Path is outside the Solaris workspace.");
    expect(io.text).toContain("recovered");
    expect(io.text).toContain("Messages: 4");
  });
});
