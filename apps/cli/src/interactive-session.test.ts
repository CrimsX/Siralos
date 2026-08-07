import { describe, expect, it } from "vitest";
import {
  createCommandRunnerRegistry,
  createDefaultPolicy,
  createPreparedCommand,
  createSolarisApplication,
  createSolarisSecurity,
  createToolRegistry,
  DEVELOP_OFFLINE_PROFILE,
  GitError,
  INSPECT_PROFILE,
  type CheckpointStore,
  type CommandRunner,
  type CommandToolPreparationResult,
  type FileCheckpoint,
  type GitDiffResult,
  type GitInspector,
  type GitStatusResult,
  type GitWorkspaceStatus,
  type ModelEvent,
  type ModelProvider,
  type ModelRequest,
  type PreparedCommandTool,
  type SandboxBackend,
  type SandboxBackendStatus,
  type SolarisApplication,
  type SolarisSecurity,
  type Tool,
  type ToolExecutionResult,
  type UndoOutcome,
  type UndoService,
} from "@solaris/core";
import { createCliApplication } from "./bootstrap/create-application.js";
import { createInteractiveApprovalReviewer } from "./approval/approval-reviewer.js";
import { createInputQueue, type InputQueue } from "./input/input-queue.js";
import {
  createSessionControls,
  runInteractiveSession,
  type SessionIO,
  type SessionInfo,
} from "./interactive-session.js";

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
  const { application, workspaceRoot, tools, security, git, checkpoints, undo, runners, sandbox } =
    await createCliApplication();
  const sessionInfo: SessionInfo = {
    workspaceRoot,
    tools,
    security,
    git,
    checkpoints,
    undo,
    runners,
    sandbox,
  };
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
    runners: createCommandRunnerRegistry([]),
    sandbox: createStubBackend({
      backendId: "stub-backend",
      state: "available",
      platform: "linux",
      version: "0.0.0",
      capabilities: {
        filesystemReadRestriction: true,
        filesystemWriteRestriction: true,
        networkRestriction: true,
        processTreeRestriction: true,
        violationReporting: true,
      },
    }),
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
    expect(io.text).toContain("Tools: 7");
    expect(io.text).toContain("Provider tools:");
    expect(io.text).toContain("Pending approval: no");
    expect(io.text).toContain("Process execution: denied");
    expect(io.text).toContain("Command runners: 2");
    expect(io.text).toContain("Last command exit: none");
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
          yield { type: "completed" };
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
    expect(io.text).toContain(
      "Command execution requires one-time approval per exact command plan.",
    );
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

  it("renders /commands with runners, sandbox, and limits", async () => {
    const io = new ScriptedIO(["/commands", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo({
      runners: createCommandRunnerRegistry([
        createStubRunner("npm-script"),
        createStubRunner("node-script"),
      ]),
    });
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("npm-script");
    expect(io.text).toContain("node-script");
    expect(io.text).toContain("available");
    expect(io.text).toContain("approval, read-only workspace, offline");
    expect(io.text).toContain("Sandbox: stub-backend (available)");
    expect(io.text).toContain("Active command: none");
    expect(io.text).toContain("Default timeout: 120 seconds");
    expect(io.text).toContain("stdout limit: 1 MiB");
    expect(io.text).toContain("Recent commands:");
    expect(io.text).not.toContain("C:\\Users");
  });

  it("reports /cancel when no command is active", async () => {
    const io = new ScriptedIO(["/cancel", "/exit"]);
    const sessionInfo: SessionInfo = buildSessionInfo();
    await runInteractiveSession(io, createTestApplication(), sessionInfo);
    expect(io.text).toContain("No command is active.");
  });

  it("classifies process.run in /tools only when provider-accessible", async () => {
    const { tool } = createStubCommandTool();
    const { io, application, sessionInfo } = createCommandSession({
      lines: ["/tools", "/exit"],
      tool,
      turns: [[{ type: "completed" }]],
    });
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("process.run");
    expect(io.text).toContain("approval required");
  });

  it("runs an approved command, streams output, and returns to the prompt", async () => {
    const { tool } = createStubCommandTool({
      onOutputs: [
        { stream: "stdout", text: "line one\n" },
        { stream: "stderr", text: "warning\n" },
        { stream: "stdout", text: "unterminated tail" },
      ],
      result: {
        status: "success",
        output: {
          status: "completed",
          exitCode: 0,
          stdout: "line one\nunterminated tail",
          stderr: "warning\n",
          stdoutTruncated: false,
          stderrTruncated: false,
          durationMs: 1840,
          runnerId: "npm-script",
          commandDigest: "abc123",
        },
        summary: "Completed npm run check (exit 0).",
      },
    });
    const { io, application, sessionInfo } = createCommandSession({
      lines: ["run npm check", "y", "", "", "/exit"],
      tool,
      turns: [
        [
          {
            type: "tool_call",
            callId: "c1",
            toolName: "process.run",
            input: { runner: "npm-script", script: "check" },
          },
          { type: "completed" },
        ],
        [{ type: "completed" }],
      ],
    });
    const exitCode = await runInteractiveSession(io, application, sessionInfo);
    expect(exitCode).toBe(0);
    expect(io.text).toContain("\u25CF npm run check");
    expect(io.text).toContain("  [stdout] line one");
    expect(io.text).toContain("  [stderr] warning");
    expect(io.text).toContain("  [stdout] unterminated tail");
    expect(io.text).toContain("\u2713 exit 0 in 1.8s");
    expect(io.text).toContain("> ");
  });

  it("renders denial, conflict, and timeout terminal states truthfully", async () => {
    const scenarios: {
      readonly result: ToolExecutionResult;
      readonly expected: string;
    }[] = [
      {
        result: { status: "timed_out", message: "timed out after 2.0 seconds" },
        expected: "timed out",
      },
      { result: { status: "conflict", message: "package.json changed" }, expected: "conflict" },
      { result: { status: "sandbox_denied", message: "write denied" }, expected: "failed" },
    ];
    for (const scenario of scenarios) {
      const { tool } = createStubCommandTool({ result: scenario.result });
      const { io, application, sessionInfo } = createCommandSession({
        lines: ["run npm check", "y", "", "", "/exit"],
        tool,
        turns: [
          [
            {
              type: "tool_call",
              callId: "c1",
              toolName: "process.run",
              input: { runner: "npm-script", script: "check" },
            },
            { type: "completed" },
          ],
          [{ type: "completed" }],
        ],
      });
      await runInteractiveSession(io, application, sessionInfo);
      expect(io.text).toContain(scenario.expected);
    }
  });

  it("cancels an active command via Ctrl+C and stays active", async () => {
    const { tool } = createStubCommandTool({
      result: { status: "cancelled", message: "The command was cancelled." },
    });
    const { application, sessionInfo } = createCommandSession({
      lines: ["run npm check", "y", ""],
      tool,
      turns: [
        [
          {
            type: "tool_call",
            callId: "c1",
            toolName: "process.run",
            input: { runner: "npm-script", script: "check" },
          },
          { type: "completed" },
        ],
      ],
    });
    const controls = createSessionControls();
    const io = new AbortTriggeringIO(["run npm check", "y", ""], () => {
      controls.cancelActivePrompt();
    });
    const sessionInfoWithControls: SessionInfo = sessionInfo;
    const exitCode = await runInteractiveSession(
      io,
      application,
      sessionInfoWithControls,
      controls,
    );
    expect(exitCode).toBe(0);
    expect(io.text).toContain("cancelled");
    expect(io.text).toContain("> ");
  });

  it("shows command state in /status after a completed command", async () => {
    const { tool } = createStubCommandTool();
    const { io, application, sessionInfo } = createCommandSession({
      lines: ["run npm check", "y", "", "", "/status", "/exit"],
      tool,
      turns: [
        [
          {
            type: "tool_call",
            callId: "c1",
            toolName: "process.run",
            input: { runner: "npm-script", script: "check" },
          },
          { type: "completed" },
        ],
        [{ type: "completed" }],
      ],
    });
    await runInteractiveSession(io, application, sessionInfo);
    expect(io.text).toContain("Active command: none");
    expect(io.text).toContain("Last command exit: 0");
    expect(io.text).toContain("Process execution: approval required");
  });

  it("does not let an approval timeout consume the next main-loop command", async () => {
    const { io, application, sessionInfo, inputQueue, text } = createTimedOutApprovalSession();
    const exitCode = await runInteractiveSession(
      io,
      application,
      sessionInfo,
      createSessionControls(),
      inputQueue,
    );
    expect(exitCode).toBe(0);
    expect(text()).toContain("denied");
    expect(text()).toContain("Solaris received: hello world");
  });
});

function createStubRunner(id: string): CommandRunner {
  return {
    definition: { id, description: `Stub ${id}` },
    prepare(): Promise<never> {
      return Promise.reject(new Error("Not used."));
    },
    toExecutionRequest(): Promise<never> {
      return Promise.reject(new Error("Not used."));
    },
    isAvailable(): Promise<boolean> {
      return Promise.resolve(true);
    },
  };
}

interface StubCommandToolOptions {
  readonly onOutputs?: readonly { readonly stream: "stdout" | "stderr"; readonly text: string }[];
  readonly result?: ToolExecutionResult;
  readonly emitDelayMs?: number;
}

function createStubCommandTool(options: StubCommandToolOptions = {}) {
  const preview = {
    runnerId: "npm-script",
    displayName: "npm run check",
    workingDirectory: ".",
    executableIdentity: "node v26.1.0 + npm 11.13.0",
    arguments: ["run", "check", "--"],
    timeoutMs: 120_000,
    stdoutLimitBytes: 1_048_576,
    stderrLimitBytes: 1_048_576,
    workspaceAccess: "read-only" as const,
    networkAccess: "denied" as const,
    environmentPolicy: "minimal" as const,
    stdinPolicy: "closed" as const,
  };
  const tool: PreparedCommandTool = {
    kind: "prepared_command",
    definition: {
      name: "process.run",
      description: "Run a validated Solaris development command.",
      inputSchema: { type: "object", properties: {}, required: [] },
    },
    capability: "process.execute",
    prepare(_input: unknown): Promise<CommandToolPreparationResult> {
      return Promise.resolve({
        status: "ready",
        command: createPreparedCommand(),
        preview,
        digest: "abcdef123456",
        commandId: "cmd-1",
      });
    },
    async executePrepared(_command, context): Promise<ToolExecutionResult> {
      for (const entry of options.onOutputs ?? []) {
        if (options.emitDelayMs !== undefined) {
          await new Promise<void>((resolve) => {
            setTimeout(resolve, options.emitDelayMs);
          });
        }
        context.onOutput?.({ type: entry.stream, text: entry.text });
      }
      return (
        options.result ?? {
          status: "success",
          output: {
            status: "completed",
            exitCode: 0,
            stdout: "",
            stderr: "",
            stdoutTruncated: false,
            stderrTruncated: false,
            durationMs: 0,
            runnerId: "npm-script",
            commandDigest: "abcdef123456",
          },
          summary: "Completed npm run check (exit 0).",
        }
      );
    },
  };
  return { tool };
}

function createCommandSession(options: {
  readonly lines: readonly string[];
  readonly tool: PreparedCommandTool;
  readonly turns: readonly (readonly ModelEvent[])[];
}) {
  const io = new ScriptedIO(options.lines);
  const inputQueue = createInputQueue(
    (prompt) => {
      io.write(prompt);
      return io.ask("");
    },
    (text) => io.write(text),
  );
  const provider = createScriptedProvider(options.turns);
  const application = createSolarisApplication({
    provider,
    tools: createToolRegistry([options.tool]),
    policy: createDefaultPolicy("develop-offline"),
    profile: DEVELOP_OFFLINE_PROFILE,
    reviewer: createInteractiveApprovalReviewer(inputQueue, 60_000),
  });
  const sessionInfo: SessionInfo = buildSessionInfo({
    runners: createCommandRunnerRegistry([]),
    tools: createToolRegistry([options.tool]).definitions(),
    security: createSolarisSecurity({
      backend: createStubBackend({
        backendId: "stub-backend",
        state: "available",
        platform: "linux",
        version: "0.0.0",
        capabilities: {
          filesystemReadRestriction: true,
          filesystemWriteRestriction: true,
          networkRestriction: true,
          processTreeRestriction: true,
          violationReporting: true,
        },
      }),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
    }),
  });
  return { io, application, sessionInfo, inputQueue };
}

function createTimedOutApprovalSession(): {
  io: SessionIO;
  application: SolarisApplication;
  sessionInfo: SessionInfo;
  inputQueue: InputQueue;
  text: () => string;
} {
  const chunks: string[] = [];
  const timedLines: Array<{ text: string; delayMs: number }> = [
    { text: "run npm check", delayMs: 0 },
    { text: "hello world", delayMs: 60 },
    { text: "/exit", delayMs: 0 },
  ];
  let lineIndex = 0;
  const io: SessionIO = {
    ask(_prompt: string): Promise<string | null> {
      const entry = timedLines[lineIndex];
      lineIndex += 1;
      if (entry === undefined) {
        return Promise.resolve(null);
      }
      return new Promise<string | null>((resolve) => {
        setTimeout(() => resolve(entry.text), entry.delayMs);
      });
    },
    write(text: string): void {
      chunks.push(text);
    },
    clear(): void {},
  };
  const inputQueue = createInputQueue(
    (prompt) => {
      io.write(prompt);
      return io.ask("");
    },
    (text) => io.write(text),
  );
  const { tool } = createStubCommandTool();
  let firstTurn = true;
  const provider: ModelProvider = {
    id: "echo-stub",
    stream(request: ModelRequest): AsyncIterable<ModelEvent> {
      const generator = {
        [Symbol.asyncIterator](): AsyncIterableIterator<ModelEvent> {
          const run = async function* (): AsyncIterableIterator<ModelEvent> {
            if (firstTurn) {
              firstTurn = false;
              yield {
                type: "tool_call",
                callId: "c1",
                toolName: "process.run",
                input: { runner: "npm-script", script: "check" },
              };
              yield { type: "completed" };
              return;
            }
            await Promise.resolve();
            const lastUser = [...request.messages]
              .reverse()
              .find((item) => item.type === "user_message");
            yield {
              type: "text_delta",
              text: `Solaris received: ${lastUser?.content ?? "?"}`,
            };
            yield { type: "completed" };
          };
          return run();
        },
      };
      return generator;
    },
  };
  const application = createSolarisApplication({
    provider,
    tools: createToolRegistry([tool]),
    policy: createDefaultPolicy("develop-offline"),
    profile: DEVELOP_OFFLINE_PROFILE,
    reviewer: createInteractiveApprovalReviewer(inputQueue, 20),
  });
  const sessionInfo: SessionInfo = buildSessionInfo();
  return { io, application, sessionInfo, inputQueue, text: () => chunks.join("") };
}

function createScriptedProvider(turns: readonly (readonly ModelEvent[])[]): ModelProvider {
  let index = 0;
  return {
    id: "scripted-stub",
    async *stream(): AsyncIterable<ModelEvent> {
      const events = turns[index] ?? [];
      index += 1;
      for (const event of events) {
        yield event;
        await Promise.resolve();
      }
    },
  };
}

class AbortTriggeringIO extends ScriptedIO {
  private readonly onAsk: () => void;
  private triggered = false;

  constructor(lines: readonly string[], onAsk: () => void) {
    super(lines);
    this.onAsk = onAsk;
  }

  override ask(prompt: string): Promise<string | null> {
    if (!this.triggered && prompt === "") {
      this.triggered = true;
      this.onAsk();
    }
    return super.ask(prompt);
  }
}
