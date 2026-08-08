import { describe, expect, it } from "vitest";
import { readdir, stat } from "node:fs/promises";
import { join } from "node:path";
import {
  COMMAND_LIMITS,
  createCommandRunnerRegistry,
  createDefaultPolicy,
  createPreparedCommand,
  VALIDATION_OFFLINE_PROFILE,
  type CommandDigestService,
  type CommandExecutionContext,
  type CommandPreparationContext,
  type CommandPreparationResult,
  type CommandPreview,
  type CommandRunner,
  type GitInspector,
  type GitStatusResult,
  type GitWorkspaceStatus,
  type PreparedCommand,
  type PreparedCommandTool,
  type SandboxBackend,
  type SandboxedProcessRequest,
  type SandboxedProcessResult,
} from "@solaris/core";
import { createSha256CommandDigestService } from "./command-digest.js";
import { createRunDirectoryProvider } from "./run-directories.js";
import { buildCommandEnvironment } from "../environment/command-environment.js";
import { readParentEnvironment } from "../environment/child-environment.js";
import { createNpmScriptRunner } from "./runners/npm-script-runner.js";
import { createProcessRunTool } from "./process-run-tool.js";
import { createFakeSandboxBackend, completedResult } from "../sandbox/fake-sandbox-backend.js";
import { createMutationLock } from "../tools/workspace/mutations/mutation-lock.js";
import { createTempWorkspace, writeFixtureFiles } from "../tools/workspace/workspace-fixtures.js";

const RUNS_ROOT = join(process.cwd(), "node_modules", ".solaris-test-runs");

/**
 * Test-only deterministic stand-in for the node-script runner. The real
 * node-script runner fails closed as unavailable (the pinned runtime cannot
 * bind execution to the approved bytes), so these tool-mechanics tests use
 * this fixture to exercise the process.run tool itself: preparation, digest
 * plumbing, run directories, output streaming, and workspace protection.
 * It is never used by production code.
 */
function createTestNodeRunner(digest: CommandDigestService): CommandRunner {
  const plans = new WeakMap<
    PreparedCommand,
    { readonly preview: CommandPreview; readonly planDigest: string }
  >();
  return {
    definition: {
      id: "node-script",
      description: "Test-only deterministic stand-in for the unavailable node-script runner.",
    },
    prepare(
      input: unknown,
      context: CommandPreparationContext,
    ): Promise<CommandPreparationResult> {
      if (context.signal?.aborted) {
        return Promise.resolve({ status: "cancelled", message: "Preparation was cancelled." });
      }
      if (typeof input !== "object" || input === null || Array.isArray(input)) {
        return Promise.resolve({ status: "invalid_input", message: "Invalid command input." });
      }
      const record = input as Record<string, unknown>;
      if (typeof record["path"] !== "string" || record["path"].length === 0) {
        return Promise.resolve({ status: "invalid_input", message: '"path" is required.' });
      }
      if (record["environment"] !== undefined || record["network"] !== undefined) {
        return Promise.resolve({
          status: "invalid_input",
          message: "Provider-controlled fields are rejected.",
        });
      }
      const argumentsValue =
        Array.isArray(record["arguments"]) &&
        record["arguments"].every((argument) => typeof argument === "string")
          ? record["arguments"]
          : [];
      const timeoutMs =
        typeof record["timeoutMs"] === "number"
          ? record["timeoutMs"]
          : COMMAND_LIMITS.defaultTimeoutMs;
      const command = createPreparedCommand();
      const preview: CommandPreview = {
        runnerId: "node-script",
        displayName: `node ${record["path"]}`,
        workingDirectory: "/",
        executableIdentity: "node (test stand-in)",
        arguments: [record["path"], ...argumentsValue],
        timeoutMs,
        stdoutLimitBytes: COMMAND_LIMITS.stdoutHardLimitBytes,
        stderrLimitBytes: COMMAND_LIMITS.stderrHardLimitBytes,
        workspaceAccess: "read-only",
        networkAccess: "denied",
        environmentPolicy: "minimal",
        stdinPolicy: "closed",
      };
      const planDigest = digest.compute({
        runnerId: "node-script",
        executableIdentity: "node (test stand-in)",
        executableVersion: process.versions.node,
        script: record["path"],
        fileHash: null,
        repositoryScript: null,
        arguments: argumentsValue,
        workingDirectory: "/",
        profileId: VALIDATION_OFFLINE_PROFILE.id,
        environmentPolicy: "minimal",
        timeoutMs,
        stdoutLimitBytes: COMMAND_LIMITS.stdoutHardLimitBytes,
        stderrLimitBytes: COMMAND_LIMITS.stderrHardLimitBytes,
        stdinPolicy: "closed",
        networkPolicy: "denied",
      });
      plans.set(command, { preview, planDigest });
      return Promise.resolve({
        status: "ready",
        command,
        preview,
        digest: planDigest,
        commandId: `test-cmd-${Math.random().toString(36).slice(2, 10)}`,
      });
    },
    toExecutionRequest(command: PreparedCommand, context: CommandExecutionContext) {
      const plan = plans.get(command);
      if (plan === undefined) {
        return Promise.resolve({ status: "failed", message: "The prepared command is unknown." });
      }
      if (context.signal?.aborted) {
        return Promise.resolve({ status: "failed", message: "The command was cancelled." });
      }
      const environment = buildCommandEnvironment(
        readParentEnvironment(),
        {
          home: context.runPaths.home,
          temp: context.runPaths.temp,
          npmCache: context.runPaths.npmCache,
          npmUserConfig: context.runPaths.npmUserConfig,
        },
        { npm: false },
      );
      return Promise.resolve({
        status: "ready",
        request: {
          executable: process.execPath,
          executableIdentity: "node (test stand-in)",
          executableVersion: process.versions.node,
          arguments: plan.preview.arguments,
          workingDirectory: "/",
          environment,
          digest: plan.planDigest,
        },
      });
    },
    isAvailable(): Promise<boolean> {
      return Promise.resolve(true);
    },
  };
}

interface ToolHarness {
  readonly tool: PreparedCommandTool;
  readonly backend: ReturnType<typeof createFakeSandboxBackend>;
  readonly runsRoot: string;
  readonly workspaceRoot: string;
  cleanup(): Promise<void>;
}

async function createHarness(
  options: {
    readonly results?: readonly SandboxedProcessResult[];
    readonly backend?: SandboxBackend;
    readonly git?: GitInspector;
    readonly outputs?: readonly { readonly type: "stdout" | "stderr"; readonly text: string }[];
    readonly executionPolicy?: ReturnType<typeof createDefaultPolicy>;
  } = {},
): Promise<ToolHarness & { readonly workspaceRoot: string }> {
  const workspace = await createTempWorkspace();
  await writeFixtureFiles(workspace.root, {
    "scripts/validate.mjs": "console.log('validated');",
    "package.json": JSON.stringify({
      name: "fixture",
      scripts: { check: "node scripts/validate.mjs" },
    }),
  });
  const digest = createSha256CommandDigestService();
  const nodeRunner = createTestNodeRunner(digest);
  const npmRunner = createNpmScriptRunner({
    digest,
    npmResolver: () =>
      Promise.resolve({
        status: "resolved" as const,
        cliPath: "/trusted/npm-cli.js",
        version: "11.0.0",
      }),
  });
  const registry = createCommandRunnerRegistry([npmRunner, nodeRunner]);
  const runsRoot = join(RUNS_ROOT, workspace.root.split("\\").pop() ?? "run");
  const fake = createFakeSandboxBackend(
    options.results === undefined && options.outputs === undefined
      ? {}
      : {
          ...(options.results === undefined ? {} : { results: options.results }),
          ...(options.outputs === undefined ? {} : { outputs: options.outputs }),
        },
  );
  const backend = options.backend ?? fake.backend;
  const tool = createProcessRunTool({
    workspaceRoot: workspace.root,
    runners: registry,
    backend,
    runDirectories: createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot }),
    lock: createMutationLock(),
    ...(options.git === undefined ? {} : { git: options.git }),
    executionProfile: VALIDATION_OFFLINE_PROFILE,
    executionPolicy: options.executionPolicy ?? createDefaultPolicy("validation-offline"),
  });
  return {
    tool,
    backend: fake,
    runsRoot,
    workspaceRoot: workspace.root,
    cleanup: () => workspace.cleanup(),
  };
}

async function prepareCommand(
  tool: PreparedCommandTool,
  input: unknown,
): Promise<{ command: Parameters<PreparedCommandTool["executePrepared"]>[0]; digest: string }> {
  const prepared = await tool.prepare(input, {});
  if (prepared.status !== "ready") {
    throw new Error(`Expected ready, got ${prepared.status}: ${prepared.message}`);
  }
  return { command: prepared.command, digest: prepared.digest };
}

function cleanGitStatus(): GitStatusResult {
  return {
    repository: true,
    branch: {
      head: "main",
      oid: "abc",
      upstream: null,
      ahead: 0,
      behind: 0,
      detached: false,
      unborn: false,
    },
    changes: [],
    conflicts: [],
    untracked: [],
    truncated: false,
  };
}

function createGitStub(changesAfterFirstStatus: boolean): GitInspector {
  let firstStatus = true;
  const clean = cleanGitStatus();
  return {
    inspectRepository(): Promise<GitWorkspaceStatus> {
      return Promise.resolve({
        gitAvailable: true,
        gitVersion: "2.40.0",
        repositoryState: "repository",
        repositoryRoot: "/workspace",
      });
    },
    getStatus(): Promise<GitStatusResult> {
      if (firstStatus) {
        firstStatus = false;
        return Promise.resolve(clean);
      }
      if (changesAfterFirstStatus) {
        return Promise.resolve({
          ...clean,
          changes: [
            {
              path: "leaked.txt",
              originalPath: null,
              indexStatus: "modified",
              worktreeStatus: "modified",
              kind: "ordinary",
            },
          ],
        });
      }
      return Promise.resolve(clean);
    },
    getDiff(): Promise<never> {
      return Promise.reject(new Error("Not used."));
    },
  };
}

async function listWorkspaceFiles(root: string): Promise<string[]> {
  const entries: string[] = [];
  const walk = async (directory: string, prefix: string): Promise<void> => {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      if (entry.isDirectory()) {
        await walk(join(directory, entry.name), `${prefix}${entry.name}/`);
      } else {
        entries.push(`${prefix}${entry.name}`);
      }
    }
  };
  await walk(root, "");
  return entries;
}

describe("process.run tool preparation", () => {
  it("rejects unknown runners", async () => {
    const harness = await createHarness();
    try {
      const result = await harness.tool.prepare({ runner: "bash", script: "x" }, {});
      expect(result.status).toBe("invalid_input");
    } finally {
      await harness.cleanup();
    }
  });

  it("rejects missing runner fields", async () => {
    const harness = await createHarness();
    try {
      const result = await harness.tool.prepare({}, {});
      expect(result.status).toBe("invalid_input");
    } finally {
      await harness.cleanup();
    }
  });

  it("rejects provider-controlled environment and network fields", async () => {
    const harness = await createHarness();
    try {
      const result = await harness.tool.prepare(
        { runner: "node-script", path: "scripts/validate.mjs", environment: { PATH: "/x" } },
        {},
      );
      expect(result.status).toBe("invalid_input");
      const network = await harness.tool.prepare(
        { runner: "node-script", path: "scripts/validate.mjs", network: true },
        {},
      );
      expect(network.status).toBe("invalid_input");
    } finally {
      await harness.cleanup();
    }
  });

  it("exposes a discriminated input schema without raw command fields", async () => {
    const harness = await createHarness();
    try {
      const schema = harness.tool.definition.inputSchema as Record<string, unknown>;
      expect(schema["additionalProperties"]).toBe(false);
      expect(JSON.stringify(schema)).not.toContain('"command"');
      expect(JSON.stringify(schema)).not.toContain('"executablePath"');
      expect(JSON.stringify(schema)).not.toContain('"shell"');
      expect(JSON.stringify(schema)).not.toContain('"environment"');
      expect(JSON.stringify(schema)).not.toContain('"network"');
      expect(JSON.stringify(schema)).not.toContain('"writablePaths"');
    } finally {
      await harness.cleanup();
    }
  });
});

describe("process.run tool execution", () => {
  it("runs the command once through the sandbox backend with the execution profile", async () => {
    const harness = await createHarness({ results: [completedResult({ stdout: "validated\n" })] });
    try {
      const controller = new AbortController();
      const { command, digest } = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const result = await harness.tool.executePrepared(command, {
        approvedDigest: digest,
        signal: controller.signal,
        onOutput: () => {},
      });
      expect(result.status).toBe("success");
      const requests = harness.backend.requests();
      expect(requests).toHaveLength(1);
      const request = requests[0] as SandboxedProcessRequest;
      expect(request.profile.id).toBe("validation-offline");
      expect(request.profile.filesystem.workspaceAccess).toBe("read-only");
      expect(request.executable).toBe(process.execPath);
      expect(request.timeoutMs).toBe(COMMAND_LIMITS.defaultTimeoutMs);
      expect(request.stdoutLimitBytes).toBe(COMMAND_LIMITS.stdoutHardLimitBytes);
      expect(request.stderrLimitBytes).toBe(COMMAND_LIMITS.stderrHardLimitBytes);
      expect(request.signal).toBeDefined();
      expect(request.onOutput).toBeDefined();
      if (result.status === "success") {
        expect(result.output).toMatchObject({
          status: "completed",
          exitCode: 0,
          stdout: "validated\n",
          runnerId: "node-script",
        });
      }
    } finally {
      await harness.cleanup();
    }
  });

  it("can be used once only", async () => {
    const harness = await createHarness({ results: [completedResult()] });
    try {
      const { command, digest } = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const first = await harness.tool.executePrepared(command, { approvedDigest: digest });
      expect(first.status).toBe("success");
      const second = await harness.tool.executePrepared(command, { approvedDigest: digest });
      expect(second.status).toBe("failed");
      expect(harness.backend.requests()).toHaveLength(1);
    } finally {
      await harness.cleanup();
    }
  });

  it("refuses to execute a foreign prepared command", async () => {
    const harness = await createHarness();
    try {
      const result = await harness.tool.executePrepared(createPreparedCommand(), {
        approvedDigest: "x",
      });
      expect(result.status).toBe("failed");
    } finally {
      await harness.cleanup();
    }
  });

  it("conflicts when the approved digest does not match the revalidated plan", async () => {
    const harness = await createHarness();
    try {
      const { command } = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const result = await harness.tool.executePrepared(command, { approvedDigest: "wrong" });
      expect(result.status).toBe("conflict");
      expect(harness.backend.requests()).toHaveLength(0);
    } finally {
      await harness.cleanup();
    }
  });

  it("denies when the effective policy denies process execution", async () => {
    const harness = await createHarness({
      results: [completedResult()],
      executionPolicy: createDefaultPolicy("inspect"),
    });
    try {
      const { command, digest } = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const result = await harness.tool.executePrepared(command, { approvedDigest: digest });
      expect(result.status).toBe("denied");
      expect(harness.backend.requests()).toHaveLength(0);
    } finally {
      await harness.cleanup();
    }
  });

  it("fails closed when the sandbox backend is unavailable", async () => {
    const unavailable: SandboxBackend = {
      id: "unavailable-backend",
      inspect: () =>
        Promise.resolve({
          backendId: "unavailable-backend",
          state: "setup-required",
          platform: "windows",
          version: "0.0.0",
          capabilities: {
            filesystemReadRestriction: false,
            filesystemWriteRestriction: false,
            networkRestriction: false,
            processTreeRestriction: false,
            violationReporting: false,
          },
        }),
      execute: () => Promise.reject(new Error("must not run")),
      close: async () => {},
    };
    const harness = await createHarness({ backend: unavailable });
    try {
      const { command, digest } = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const result = await harness.tool.executePrepared(command, { approvedDigest: digest });
      expect(result.status).toBe("sandbox_unavailable");
    } finally {
      await harness.cleanup();
    }
  });

  it("fails closed when the backend cannot enforce key restrictions", async () => {
    const weak: SandboxBackend = {
      id: "weak-backend",
      inspect: () =>
        Promise.resolve({
          backendId: "weak-backend",
          state: "available",
          platform: "linux",
          version: "0.0.0",
          capabilities: {
            filesystemReadRestriction: true,
            filesystemWriteRestriction: false,
            networkRestriction: true,
            processTreeRestriction: true,
            violationReporting: false,
          },
        }),
      execute: () => Promise.reject(new Error("must not run")),
      close: async () => {},
    };
    const harness = await createHarness({ backend: weak });
    try {
      const { command, digest } = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const result = await harness.tool.executePrepared(command, { approvedDigest: digest });
      expect(result.status).toBe("sandbox_unavailable");
    } finally {
      await harness.cleanup();
    }
  });

  it("fails closed when the backend cannot enforce the host-read allowlist", async () => {
    const weak: SandboxBackend = {
      id: "weak-read-backend",
      inspect: () =>
        Promise.resolve({
          backendId: "weak-read-backend",
          state: "available",
          platform: "windows",
          version: "0.0.0",
          capabilities: {
            filesystemReadRestriction: false,
            filesystemWriteRestriction: true,
            networkRestriction: true,
            processTreeRestriction: true,
            violationReporting: true,
          },
        }),
      execute: () => Promise.reject(new Error("must not run")),
      close: async () => {},
    };
    const harness = await createHarness({ backend: weak });
    try {
      const { command, digest } = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const result = await harness.tool.executePrepared(command, { approvedDigest: digest });
      expect(result.status).toBe("sandbox_unavailable");
      expect(harness.backend.requests()).toHaveLength(0);
    } finally {
      await harness.cleanup();
    }
  });

  it("reports a nonzero exit as a completed command", async () => {
    const harness = await createHarness({
      results: [completedResult({ exitCode: 2, stdout: "tests failed" })],
    });
    try {
      const { command, digest } = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const result = await harness.tool.executePrepared(command, { approvedDigest: digest });
      expect(result.status).toBe("success");
      if (result.status === "success") {
        expect(result.output).toMatchObject({ status: "completed", exitCode: 2 });
      }
    } finally {
      await harness.cleanup();
    }
  });

  it("classifies timeout, output-limit, and sandbox denial truthfully", async () => {
    const results: readonly SandboxedProcessResult[] = [
      { ...completedResult(), status: "timed-out", exitCode: null },
      { ...completedResult(), status: "output-limit", exitCode: null },
      {
        ...completedResult(),
        status: "sandbox-denied",
        exitCode: 1,
        violations: [{ category: "sandbox", summary: "write denied" }],
      },
      { ...completedResult(), status: "sandbox-unavailable", exitCode: null },
      { ...completedResult(), status: "failed", exitCode: null },
    ];
    const harness = await createHarness({ results });
    try {
      const outcomes: string[] = [];
      for (let index = 0; index < results.length; index += 1) {
        const { command, digest } = await prepareCommand(harness.tool, {
          runner: "node-script",
          path: "scripts/validate.mjs",
        });
        const result = await harness.tool.executePrepared(command, { approvedDigest: digest });
        outcomes.push(result.status);
      }
      expect(outcomes).toEqual([
        "timed_out",
        "output_limit",
        "sandbox_denied",
        "sandbox_unavailable",
        "failed",
      ]);
    } finally {
      await harness.cleanup();
    }
  });

  it("streams bounded output events", async () => {
    const harness = await createHarness({
      results: [completedResult({ stdout: "one\ntwo\n" })],
      outputs: [
        { type: "stdout", text: "one\n" },
        { type: "stderr", text: "warn\n" },
      ],
    });
    try {
      const { command, digest } = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const seen: string[] = [];
      const result = await harness.tool.executePrepared(command, {
        approvedDigest: digest,
        onOutput: (event) => {
          seen.push(`${event.type}:${event.text}`);
        },
      });
      expect(result.status).toBe("success");
      expect(seen).toEqual(["stdout:one\n", "stderr:warn\n"]);
    } finally {
      await harness.cleanup();
    }
  });

  it("bounds provider-visible output with an explicit omission marker", async () => {
    const big = "y".repeat(COMMAND_LIMITS.providerStdoutReturnBytes + 40_000);
    const harness = await createHarness({
      results: [completedResult({ stdout: big })],
    });
    try {
      const { command, digest } = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const result = await harness.tool.executePrepared(command, { approvedDigest: digest });
      expect(result.status).toBe("success");
      if (result.status === "success") {
        const output = result.output as Record<string, unknown>;
        expect(output["stdoutTruncated"]).toBe(true);
        const stdout = output["stdout"] as string;
        expect(Buffer.byteLength(stdout, "utf8")).toBeLessThanOrEqual(
          COMMAND_LIMITS.providerStdoutReturnBytes + 200,
        );
        expect(stdout).toContain("[omitted");
        expect(stdout.startsWith("yyy")).toBe(true);
        expect(stdout.endsWith("yyy")).toBe(true);
      }
    } finally {
      await harness.cleanup();
    }
  });

  it("cancels when the signal aborts", async () => {
    const harness = await createHarness({ results: [completedResult()] });
    try {
      const { command, digest } = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const controller = new AbortController();
      controller.abort();
      const result = await harness.tool.executePrepared(command, {
        approvedDigest: digest,
        signal: controller.signal,
      });
      expect(result.status).toBe("cancelled");
      expect(harness.backend.requests()).toHaveLength(0);
    } finally {
      await harness.cleanup();
    }
  });
});

describe("process.run tool workspace protection", () => {
  it("does not create any files in the project workspace", async () => {
    const harness = await createHarness({ results: [completedResult({ stdout: "ok\n" })] });
    try {
      const before = await listWorkspaceFiles(harness.workspaceRoot);
      const { command, digest } = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const result = await harness.tool.executePrepared(command, { approvedDigest: digest });
      expect(result.status).toBe("success");
      const after = await listWorkspaceFiles(harness.workspaceRoot);
      expect(after).toEqual(before);
    } finally {
      await harness.cleanup();
    }
  });

  it("reports a workspace mutation as a violation and disables commands for the session", async () => {
    const harness = await createHarness({
      results: [completedResult()],
      git: createGitStub(true),
    });
    try {
      const first = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const firstResult = await harness.tool.executePrepared(first.command, {
        approvedDigest: first.digest,
      });
      expect(firstResult.status).toBe("workspace_violation");
      const second = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const secondResult = await harness.tool.executePrepared(second.command, {
        approvedDigest: second.digest,
      });
      expect(secondResult.status).toBe("workspace_violation");
      expect(harness.backend.requests()).toHaveLength(1);
    } finally {
      await harness.cleanup();
    }
  });

  it("leaves commands enabled when the workspace is unchanged", async () => {
    const harness = await createHarness({
      results: [completedResult(), completedResult()],
      git: createGitStub(false),
    });
    try {
      const first = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      expect(
        (await harness.tool.executePrepared(first.command, { approvedDigest: first.digest }))
          .status,
      ).toBe("success");
      const second = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      expect(
        (await harness.tool.executePrepared(second.command, { approvedDigest: second.digest }))
          .status,
      ).toBe("success");
      expect(harness.backend.requests()).toHaveLength(2);
    } finally {
      await harness.cleanup();
    }
  });
});

describe("process.run tool run directories", () => {
  it("creates sandbox-private run directories and cleans them up", async () => {
    const harness = await createHarness({ results: [completedResult()] });
    try {
      const { command, digest } = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const result = await harness.tool.executePrepared(command, { approvedDigest: digest });
      expect(result.status).toBe("success");
      if (result.status === "success") {
        const request = harness.backend.requests()[0] as SandboxedProcessRequest;
        const environment = request.environment;
        const runHome = environment["HOME"];
        expect(runHome).toBeTruthy();
        expect(runHome?.includes("home")).toBe(true);
        expect(environment["TEMP"]).toBeTruthy();
      }
      const runsRootExists = await stat(harness.runsRoot).catch(() => null);
      expect(runsRootExists?.isDirectory() ?? false).toBe(true);
      const leftovers: string[] = [];
      const walk = async (directory: string): Promise<void> => {
        for (const entry of await readdir(directory, { withFileTypes: true })) {
          if (entry.isDirectory()) {
            await walk(join(directory, entry.name));
          }
        }
      };
      await walk(harness.runsRoot);
      expect(leftovers).toEqual([]);
    } finally {
      await harness.cleanup();
    }
  });

  it("grants the backend exactly the current run directory, never the shared runs root", async () => {
    const harness = await createHarness({ results: [completedResult()] });
    try {
      const { command, digest } = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const result = await harness.tool.executePrepared(command, { approvedDigest: digest });
      expect(result.status).toBe("success");
      if (result.status === "success") {
        const request = harness.backend.requests()[0] as SandboxedProcessRequest;
        expect(request.runDirectory).toBeTruthy();
        expect(request.runDirectory?.startsWith(harness.runsRoot)).toBe(true);
        expect(request.runDirectory).not.toBe(harness.runsRoot);
      }
    } finally {
      await harness.cleanup();
    }
  });

  it("surfaces npm-script unavailability through the process tool", async () => {
    const harness = await createHarness({ results: [completedResult()] });
    try {
      const prepared = await harness.tool.prepare({ runner: "npm-script", script: "check" }, {});
      expect(prepared.status).toBe("unavailable");
      if (prepared.status === "unavailable") {
        expect(prepared.message).toContain("npm-script runner is unavailable");
      }
      expect(harness.backend.requests()).toHaveLength(0);
    } finally {
      await harness.cleanup();
    }
  });

  it("reports run-directory cleanup failures truthfully", async () => {
    const workspace = await createTempWorkspace();
    try {
      await writeFixtureFiles(workspace.root, {
        "scripts/validate.mjs": "console.log('x');",
      });
      const digest = createSha256CommandDigestService();
      const registry = createCommandRunnerRegistry([createTestNodeRunner(digest)]);
      const fake = createFakeSandboxBackend({ results: [completedResult()] });
      const runsRoot = join(RUNS_ROOT, "cleanup-fail");
      const tool = createProcessRunTool({
        workspaceRoot: workspace.root,
        runners: registry,
        backend: fake.backend,
        runDirectories: {
          create: async () => {
            const { mkdir } = await import("node:fs/promises");
            const runDirectory = join(runsRoot, `run-${Date.now()}-${Math.random()}`);
            const directories = {
              runId: "run-x",
              root: runDirectory,
              home: join(runDirectory, "home"),
              temp: join(runDirectory, "tmp"),
              npmCache: join(runDirectory, "npm-cache"),
              npmUserConfig: join(runDirectory, "npmrc"),
              scriptCache: join(runDirectory, "script-cache"),
            };
            await mkdir(directories.root, { recursive: true });
            await mkdir(directories.scriptCache, { recursive: true });
            return directories;
          },
          remove: () => Promise.resolve({ ok: false, message: "The directory is locked." }),
        },
        lock: createMutationLock(),
        executionProfile: VALIDATION_OFFLINE_PROFILE,
        executionPolicy: createDefaultPolicy("validation-offline"),
      });
      const prepared = await tool.prepare(
        { runner: "node-script", path: "scripts/validate.mjs" },
        {},
      );
      if (prepared.status !== "ready") {
        throw new Error("Expected ready.");
      }
      const result = await tool.executePrepared(prepared.command, {
        approvedDigest: prepared.digest,
      });
      expect(result.status).toBe("success");
      if (result.status === "success") {
        expect((result.output as Record<string, unknown>)["cleanupWarning"]).toContain(
          "directory is locked",
        );
      }
    } finally {
      await workspace.cleanup();
    }
  });
});

describe("process.run tool serialization", () => {
  it("waits for the execution lock before starting", async () => {
    let releaseFirst!: () => void;
    const gate = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    let started = 0;
    const blockingBackend: SandboxBackend = {
      id: "blocking-backend",
      inspect: () =>
        Promise.resolve({
          backendId: "blocking-backend",
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
      execute: async (_request: SandboxedProcessRequest): Promise<SandboxedProcessResult> => {
        started += 1;
        if (started === 1) {
          await gate;
        }
        return completedResult();
      },
      close: async () => {},
    };
    const harness = await createHarness({ backend: blockingBackend });
    try {
      const first = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const second = await prepareCommand(harness.tool, {
        runner: "node-script",
        path: "scripts/validate.mjs",
      });
      const firstRun = harness.tool.executePrepared(first.command, {
        approvedDigest: first.digest,
      });
      await waitFor(() => started === 1);
      expect(started).toBe(1);
      const secondRun = harness.tool.executePrepared(second.command, {
        approvedDigest: second.digest,
      });
      // The second run must remain blocked behind the mutation lock.
      await waitFor(() => started === 1);
      await new Promise<void>((resolve) => {
        setTimeout(resolve, 20);
      });
      expect(started).toBe(1);
      releaseFirst();
      expect((await firstRun).status).toBe("success");
      expect((await secondRun).status).toBe("success");
      expect(started).toBe(2);
    } finally {
      await harness.cleanup();
    }
  });
});

async function waitFor(predicate: () => boolean): Promise<void> {
  const deadline = Date.now() + 5000;
  while (!predicate()) {
    if (Date.now() > deadline) {
      throw new Error("Timed out waiting for the expected state.");
    }
    await new Promise<void>((resolve) => {
      setTimeout(resolve, 5);
    });
  }
}
