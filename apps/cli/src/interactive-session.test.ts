import { describe, expect, it } from "vitest";
import {
  createSolarisApplication,
  createToolRegistry,
  type ModelEvent,
  type ModelProvider,
} from "@solaris/core";
import { createCliApplication } from "./bootstrap/create-application.js";
import { runInteractiveSession, type SessionIO } from "./interactive-session.js";

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

function createComposedSession(lines: readonly string[]) {
  const io = new ScriptedIO(lines);
  const { application } = createCliApplication();
  return { io, application };
}

describe("runInteractiveSession", () => {
  it("submits a prompt and renders the streamed response", async () => {
    const { io, application } = createComposedSession(["hello", "/exit"]);
    const exitCode = await runInteractiveSession(io, application);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("Solaris received: hello");
  });

  it("preserves conversation history across prompts", async () => {
    const { io, application } = createComposedSession(["first", "second", "/exit"]);
    await runInteractiveSession(io, application);
    expect(io.text).toContain("Solaris received: first");
    expect(io.text).toContain("Solaris received: second");
  });

  it("ignores empty input", async () => {
    const { io, application } = createComposedSession(["", "   ", "/exit"]);
    const exitCode = await runInteractiveSession(io, application);
    expect(exitCode).toBe(0);
    expect(io.text).not.toContain("Solaris received: ");
  });

  it("reports an invalid slash command", async () => {
    const { io, application } = createComposedSession(["/bogus", "/exit"]);
    await runInteractiveSession(io, application);
    expect(io.text).toContain("Unknown command: /bogus");
    expect(io.text).not.toContain("Solaris received:");
  });

  it("exits cleanly on end of input", async () => {
    const { io, application } = createComposedSession([]);
    const exitCode = await runInteractiveSession(io, application);
    expect(exitCode).toBe(0);
  });

  it("renders help and status", async () => {
    const { io, application } = createComposedSession(["/help", "/status", "/exit"]);
    await runInteractiveSession(io, application);
    expect(io.text).toContain("Available commands");
    expect(io.text).toContain("Provider: deterministic-fake");
    expect(io.text).toContain("Messages: 0");
  });

  it("clears the terminal without clearing conversation history", async () => {
    const { io, application } = createComposedSession(["hello", "/clear", "/status", "/exit"]);
    await runInteractiveSession(io, application);
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
    const exitCode = await runInteractiveSession(io, application);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("provider exploded");
  });
});
