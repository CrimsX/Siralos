import { describe, expect, it } from "vitest";
import {
  createDefaultPolicy,
  createSolarisApplication,
  createSolarisSecurity,
  createToolRegistry,
  GitError,
  INSPECT_PROFILE,
  type CheckpointStore,
  type FileCheckpoint,
  type GitDiffResult,
  type GitInspector,
  type GitStatusResult,
  type GitWorkspaceStatus,
  type ModelEvent,
  type ModelProvider,
  type SandboxBackend,
  type SandboxBackendStatus,
  type SolarisSecurity,
  type Tool,
  type ToolExecutionResult,
  type UndoOutcome,
  type UndoService,
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
  const { application, workspaceRoot, tools, security, git, checkpoints, undo } =
    await createCliApplication();
  const sessionInfo: SessionInfo = { workspaceRoot, tools, security, git, checkpoints, undo };
  return { io, application, sessionInfo };
}

function createStubGit(): GitInspector {
  return {
    inspectRepository(): Promise<GitWorkspaceStatus> {
      return Promise.resolve({
        gitAvailable: false,
        gitVersion: null,
        repositoryState: "unavailable",
        repositoryRoot: null,
        message: "Git is not installed or not on PATH.",
      });
    },
    getStatus(): Promise<GitStatusResult> {
      return Promise.reject(new GitError("git_unavailable", "Git is not available."));
    },
    getDiff(): Promise<GitDiffResult> {
      return Promise.reject(new GitError("git_unavailable", "Git is not available."));
    },
  };
}

function createStubCheckpointStore(): CheckpointStore {
  return {
    prepare(): Promise<FileCheckpoint> {
      return Promise.reject(new Error("Not used in session tests."));
    },
    finalizeApplied(): Promise<FileCheckpoint> {
      return Promise.reject(new Error("Not used in session tests."));
    },
    markUndone(): Promise<FileCheckpoint> {
      return Promise.reject(new Error("Not used in session tests."));
    },
    markState(): Promise<FileCheckpoint> {
      return Promise.reject(new Error("Not used in session tests."));
    },
    get(): Promise<FileCheckpoint | null> {
      return Promise.resolve(null);
    },
    list(): Promise<readonly FileCheckpoint[]> {
      return Promise.resolve([]);
    },
    loadPreimage(): Promise<Uint8Array | null> {
      return Promise.resolve(null);
    },
  };
}

function createStubUndo(): UndoService {
  return {
    undo(): Promise<UndoOutcome> {
      return Promise.resolve({
        type: "failed",
        checkpointId: null,
        path: null,
        message: "No undo service available.",
      });
    },
  };
}

function buildSessionInfo(overrides: Partial<SessionInfo> = {}): SessionInfo {
  return {
    workspaceRoot: "/workspace",
    tools: [],
    security: createFakeSecurity(),
    git: createStubGit(),
    checkpoints: createStubCheckpointStore(),
    undo: createStubUndo(),
    ...overrides,
  };
}

function createStubBackend(status: SandboxBackendStatus): SandboxBackend {
  return {
    id: "stub-backend",
    inspect(): Promise<SandboxBackendStatus> {
      return Promise.resolve(status);
    },
    execute(): Promise<never> {
      throw new Error("Not used in session tests.");
    },
    close(): Promise<void> {
      return Promise.resolve();
    },
  };
}

function createFakeSecurity(status?: SandboxBackendStatus): SolarisSecurity {
  const resolvedStatus: SandboxBackendStatus = status ?? {
    backendId: "fake-backend",
    state: "available",
    platform: "linux",
    version: "0.0.0-fake",
    capabilities: {
      filesystemReadRestriction: true,
      filesystemWriteRestriction: true,
      networkRestriction: true,
      processTreeRestriction: true,
      violationReporting: true,
    },
  };
  return createSolarisSecurity({
    backend: createStubBackend(resolvedStatus),
    policy: createDefaultPolicy("inspect"),
    profile: INSPECT_PROFILE,
  });
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
    expect(io.text).toContain("Sandbox:");
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
    const sessionInfo: SessionInfo = buildSessionInfo();
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("provider exploded");
  });
});

describe("runInteractiveSession tool activity", () => {
  it("lists the registered tools with classifications for /tools", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["/tools", "/exit"]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Available tools");
    expect(io.text).toContain("workspace.list");
    expect(io.text).toContain("workspace.read");
    expect(io.text).toContain("workspace.search");
    expect(io.text).toContain("(read-only, allowed)");
  });

  it("includes the workspace, sandbox, and tool counts in /status", async () => {
    const { io, application, sessionInfo } = await createComposedSession(["/status", "/exit"]);
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Workspace:");
    expect(io.text).toContain("Tools: 6");
    expect(io.text).toContain("Provider tools:");
    expect(io.text).toContain("Pending approval: no");
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
    const sessionInfo: SessionInfo = buildSessionInfo({
      tools: [{ definition: tool.definition, capability: "workspace.write" }],
    });
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("\u2715 Path is outside the Solaris workspace.");
    expect(io.text).toContain("recovered");
    expect(io.text).toContain("Messages: 4");
  });
});

describe("runInteractiveSession sandbox diagnostics", () => {
  it("renders the capability rules for /permissions", async () => {
    const io = new ScriptedIO(["/permissions", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    const exitCode = await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("Profile: inspect");
    expect(io.text).toMatch(/workspace\.read\s+allow/);
    expect(io.text).toMatch(/workspace\.write\s+deny/);
    expect(io.text).toMatch(/process\.execute\s+deny/);
    expect(io.text).toMatch(/network\.outbound\s+deny/);
    expect(io.text).toContain("No provider-accessible process or write tool exists yet.");
  });

  it("renders the sandbox status for /sandbox without secrets", async () => {
    const io = new ScriptedIO(["/sandbox", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    const exitCode = await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("Profile: inspect");
    expect(io.text).toContain("Backend: fake-backend");
    expect(io.text).toContain("State: available");
    expect(io.text).toContain("Network: denied");
    expect(io.text).toContain("Environment: minimal");
    expect(io.text).not.toContain("sk-");
  });

  it("renders setup-required guidance when the backend needs setup", async () => {
    const io = new ScriptedIO(["/sandbox", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo({
      security: createFakeSecurity({
        backendId: "fake-backend",
        state: "setup-required",
        platform: "windows",
        version: "0.0.0-fake",
        capabilities: {
          filesystemReadRestriction: false,
          filesystemWriteRestriction: false,
          networkRestriction: false,
          processTreeRestriction: false,
          violationReporting: false,
        },
        message: "Run the one-time elevated setup command.",
      }),
    });
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("State: setup-required");
    expect(io.text).toContain("Run the one-time elevated setup command.");
    expect(io.text).toContain("alpha");
  });
});

function createTestApplication() {
  return createSolarisApplication({
    provider: {
      id: "session-test-provider",
      async *stream(): AsyncIterable<ModelEvent> {
        yield { type: "text_delta", text: "ok" };
        await Promise.resolve();
        yield { type: "completed" };
      },
    },
    tools: createToolRegistry([]),
  });
}

describe("runInteractiveSession git and checkpoint commands", () => {
  it("renders git status for a non-repository workspace", async () => {
    const io = new ScriptedIO(["/git-status", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    const exitCode = await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("Git: unavailable");
    expect(io.text).toContain("Repository: unavailable");
  });

  it("renders a diff failure without raw traces", async () => {
    const io = new ScriptedIO(["/diff", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Git is not available.");
    expect(io.text).not.toContain("at ");
  });

  it("rejects invalid diff scopes", async () => {
    const io = new ScriptedIO(["/diff bogus", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Usage: /diff");
  });

  it("lists checkpoints without preimage content", async () => {
    const io = new ScriptedIO(["/checkpoints", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo({
      checkpoints: {
        prepare() {
          return Promise.reject(new Error("not used"));
        },
        finalizeApplied() {
          return Promise.reject(new Error("not used"));
        },
        markUndone() {
          return Promise.reject(new Error("not used"));
        },
        markState() {
          return Promise.reject(new Error("not used"));
        },
        get() {
          return Promise.resolve(null);
        },
        list() {
          return Promise.resolve([
            {
              version: 1,
              id: "cp_01Jtest12345",
              workspaceFingerprint: "fingerprint",
              relativePath: "README.md",
              operation: "update",
              toolName: "workspace.edit_file",
              createdAt: new Date().toISOString(),
              state: "applied",
              before: { exists: true, sha256: "a", byteLength: 1 },
              after: { exists: true, sha256: "b", byteLength: 1 },
              preview: { addedLines: 1, removedLines: 1 },
            },
          ] as FileCheckpoint[]);
        },
        loadPreimage() {
          return Promise.resolve(null);
        },
      },
    });
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("cp_01Jtest12");
    expect(io.text).toContain("applied");
    expect(io.text).not.toContain("preimage");
  });

  it("renders undo failures", async () => {
    const io = new ScriptedIO(["/undo", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("No undo service available.");
  });

  it("includes git and checkpoint summaries in /status", async () => {
    const io = new ScriptedIO(["/status", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("Git: unavailable");
    expect(io.text).toContain("Checkpoint: none");
    expect(io.text).toContain("Uncertain checkpoints: 0");
  });
});
