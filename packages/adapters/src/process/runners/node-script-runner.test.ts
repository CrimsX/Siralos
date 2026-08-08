import { afterEach, describe, expect, it } from "vitest";
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { pathToFileURL } from "node:url";
import {
  COMMAND_LIMITS,
  createPreparedCommand,
  type CommandExecutionContext,
  type CommandExecutionRequest,
  type CommandPreparationResult,
} from "@solaris/core";
import { createSha256CommandDigestService } from "../command-digest.js";
import {
  createNodeScriptRunner,
  SOLARIS_CJS_DENIAL_BOOTSTRAP_FILE,
  SOLARIS_ESM_DENIAL_BOOTSTRAP_FILE,
} from "./node-script-runner.js";
import {
  createFile,
  createSymlink,
  createTempWorkspace,
  SYMLINKS_SUPPORTED,
  writeFixtureFiles,
} from "../../tools/workspace/workspace-fixtures.js";

function createRunner() {
  return createNodeScriptRunner({ digest: createSha256CommandDigestService() });
}

const runPathDirectories: string[] = [];
const sentinelRoots: string[] = [];

async function createRunPaths(): Promise<CommandExecutionContext["runPaths"]> {
  const root = await mkdtemp(join(tmpdir(), "solaris-run-paths-"));
  runPathDirectories.push(root);
  const home = join(root, "home");
  const temp = join(root, "tmp");
  const npmCache = join(root, "npm-cache");
  const scriptCache = join(root, "script-cache");
  await mkdir(home);
  await mkdir(temp);
  await mkdir(npmCache);
  await mkdir(scriptCache);
  return {
    runId: "run-1",
    root,
    home,
    temp,
    npmCache,
    npmUserConfig: join(root, "npmrc"),
    scriptCache,
  };
}

async function createSentinelRoot(): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), "solaris-sentinel-"));
  sentinelRoots.push(root);
  return root;
}

afterEach(async () => {
  for (const directory of runPathDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
  for (const directory of sentinelRoots.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
});

async function prepare(workspaceRoot: string, input: unknown): Promise<CommandPreparationResult> {
  return createRunner().prepare(input, { workspaceRoot });
}

function ready(
  result: CommandPreparationResult,
): Extract<CommandPreparationResult, { status: "ready" }> {
  if (result.status !== "ready") {
    throw new Error(`Expected ready, got ${result.status}: ${result.message}`);
  }
  return result;
}

async function prepareAndExecute(
  workspaceRoot: string,
  input: unknown,
): Promise<CommandExecutionRequest> {
  const runner = createRunner();
  const prepared = ready(await runner.prepare(input, { workspaceRoot }));
  const runPaths = await createRunPaths();
  const execution = await runner.toExecutionRequest(prepared.command, {
    approvedDigest: prepared.digest,
    runPaths,
  });
  expect(execution.status).toBe("ready");
  if (execution.status !== "ready") {
    throw new Error(`Expected ready, got ${execution.status}`);
  }
  return execution.request;
}

interface SpawnedResult {
  readonly exitCode: number | null;
  readonly stdout: string;
  readonly stderr: string;
}

/** Executes the exact request argv the runner produced, through a real spawn. */
function executeRequest(request: CommandExecutionRequest): Promise<SpawnedResult> {
  return new Promise((resolve, reject) => {
    const child = spawn(request.executable, [...request.arguments], {
      cwd: request.workingDirectory,
      env: { ...request.environment },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout?.setEncoding("utf8");
    child.stderr?.setEncoding("utf8");
    child.stdout?.on("data", (chunk: string) => {
      stdout += chunk;
    });
    child.stderr?.on("data", (chunk: string) => {
      stderr += chunk;
    });
    child.on("error", reject);
    child.on("close", (exitCode: number | null) => {
      resolve({ exitCode, stdout, stderr });
    });
  });
}

function scriptIndex(request: CommandExecutionRequest, scriptCache: string): number {
  const bootstrapPath = join(scriptCache, SOLARIS_CJS_DENIAL_BOOTSTRAP_FILE);
  const index = request.arguments.findIndex(
    (argument) => argument.startsWith(scriptCache) && argument !== bootstrapPath,
  );
  expect(index).toBeGreaterThanOrEqual(0);
  return index;
}

describe("node-script runner input validation", () => {
  it("accepts .js, .mjs, and .cjs scripts", async () => {
    const workspace = await createTempWorkspace();
    try {
      await writeFixtureFiles(workspace.root, {
        "a.js": "console.log('ok');",
        "b.mjs": "console.log('ok');",
        "c.cjs": "console.log('ok');",
      });
      for (const script of ["a.js", "b.mjs", "c.cjs"]) {
        const result = await prepare(workspace.root, {
          runner: "node-script",
          path: script,
        });
        expect(result.status).toBe("ready");
      }
    } finally {
      await workspace.cleanup();
    }
  });

  it("rejects TypeScript and unsupported extensions", async () => {
    const workspace = await createTempWorkspace();
    try {
      await writeFixtureFiles(workspace.root, {
        "a.ts": "console.log('ok');",
        "b.txt": "hello",
      });
      const ts = await prepare(workspace.root, { runner: "node-script", path: "a.ts" });
      expect(ts.status).toBe("invalid_input");
      const txt = await prepare(workspace.root, { runner: "node-script", path: "b.txt" });
      expect(txt.status).toBe("invalid_input");
    } finally {
      await workspace.cleanup();
    }
  });

  it("rejects a missing path field", async () => {
    const workspace = await createTempWorkspace();
    try {
      const result = await prepare(workspace.root, { runner: "node-script" });
      expect(result.status).toBe("invalid_input");
    } finally {
      await workspace.cleanup();
    }
  });

  it("rejects an unknown runner id", async () => {
    const workspace = await createTempWorkspace();
    try {
      const result = await prepare(workspace.root, {
        runner: "bash",
        path: "a.js",
      });
      expect(result.status).toBe("invalid_input");
    } finally {
      await workspace.cleanup();
    }
  });

  it("rejects provider-controlled Node flags and raw command fields", async () => {
    const workspace = await createTempWorkspace();
    try {
      await writeFixtureFiles(workspace.root, { "a.js": "console.log('ok');" });
      for (const input of [
        { runner: "node-script", path: "a.js", nodeFlags: ["--inspect"] },
        { runner: "node-script", path: "a.js", nodeOptions: "--eval x" },
        { runner: "node-script", path: "a.js", command: "node a.js" },
        { runner: "node-script", path: "a.js", executablePath: "/usr/bin/node" },
        { runner: "node-script", path: "a.js", environment: { PATH: "/x" } },
        { runner: "node-script", path: "a.js", network: true },
        { runner: "node-script", path: "a.js", writablePaths: ["/tmp"] },
        { runner: "node-script", path: "a.js", shell: "bash" },
      ]) {
        const result = await prepare(workspace.root, input);
        expect(result.status).toBe("invalid_input");
      }
    } finally {
      await workspace.cleanup();
    }
  });

  it("rejects absolute and escaping working directories", async () => {
    const workspace = await createTempWorkspace();
    try {
      await writeFixtureFiles(workspace.root, { "a.js": "console.log('ok');" });
      for (const workingDirectory of ["/abs", "C:\\abs", "../escape", ".."]) {
        const result = await prepare(workspace.root, {
          runner: "node-script",
          path: "a.js",
          workingDirectory,
        });
        expect(result.status).toBe("invalid_input");
      }
    } finally {
      await workspace.cleanup();
    }
  });

  it("rejects NUL, oversized, and excess arguments", async () => {
    const workspace = await createTempWorkspace();
    try {
      await writeFixtureFiles(workspace.root, { "a.js": "console.log('ok');" });
      const nul = await prepare(workspace.root, {
        runner: "node-script",
        path: "a.js",
        arguments: ["a\u0000b"],
      });
      expect(nul.status).toBe("invalid_input");
      const oversized = await prepare(workspace.root, {
        runner: "node-script",
        path: "a.js",
        arguments: ["x".repeat(COMMAND_LIMITS.maxArgumentBytes + 1)],
      });
      expect(oversized.status).toBe("invalid_input");
      const excess = await prepare(workspace.root, {
        runner: "node-script",
        path: "a.js",
        arguments: Array.from(
          { length: COMMAND_LIMITS.maxArguments + 1 },
          (_, index) => `a${index}`,
        ),
      });
      expect(excess.status).toBe("invalid_input");
      const control = await prepare(workspace.root, {
        runner: "node-script",
        path: "a.js",
        arguments: ["\u001b[31m"],
      });
      expect(control.status).toBe("invalid_input");
    } finally {
      await workspace.cleanup();
    }
  });

  it("rejects invalid timeouts", async () => {
    const workspace = await createTempWorkspace();
    try {
      await writeFixtureFiles(workspace.root, { "a.js": "console.log('ok');" });
      for (const timeoutMs of [0, 500, 999, 600_001, 1.5, "120000"]) {
        const result = await prepare(workspace.root, {
          runner: "node-script",
          path: "a.js",
          timeoutMs,
        });
        expect(result.status).toBe("invalid_input");
      }
      const ok = await prepare(workspace.root, {
        runner: "node-script",
        path: "a.js",
        timeoutMs: 60_000,
      });
      expect(ok.status).toBe("ready");
      if (ok.status === "ready") {
        expect(ok.preview.timeoutMs).toBe(60_000);
      }
    } finally {
      await workspace.cleanup();
    }
  });

  it("defaults the timeout and working directory", async () => {
    const workspace = await createTempWorkspace();
    try {
      await writeFixtureFiles(workspace.root, { "a.js": "console.log('ok');" });
      const result = ready(await prepare(workspace.root, { runner: "node-script", path: "a.js" }));
      expect(result.preview.timeoutMs).toBe(COMMAND_LIMITS.defaultTimeoutMs);
      expect(result.preview.workingDirectory).toBe(".");
    } finally {
      await workspace.cleanup();
    }
  });
});

describe("node-script runner script validation", () => {
  it("rejects symbolic-linked scripts", async () => {
    if (!SYMLINKS_SUPPORTED) {
      return;
    }
    const workspace = await createTempWorkspace();
    try {
      await writeFixtureFiles(workspace.root, { "real.js": "console.log('ok');" });
      await createSymlink(join(workspace.root, "real.js"), join(workspace.root, "link.js"));
      const result = await prepare(workspace.root, {
        runner: "node-script",
        path: "link.js",
      });
      expect(result.status).toBe("invalid_input");
    } finally {
      await workspace.cleanup();
    }
  });

  it("rejects symbolic-linked working directories", async () => {
    if (!SYMLINKS_SUPPORTED) {
      return;
    }
    const workspace = await createTempWorkspace();
    try {
      await createFile(workspace.root, "real-dir/a.js", "console.log('ok');");
      await createSymlink(join(workspace.root, "real-dir"), join(workspace.root, "link-dir"));
      const result = await prepare(workspace.root, {
        runner: "node-script",
        path: "a.js",
        workingDirectory: "link-dir",
      });
      expect(result.status).toBe("invalid_input");
    } finally {
      await workspace.cleanup();
    }
  });

  it("rejects oversized scripts", async () => {
    const workspace = await createTempWorkspace();
    try {
      const path = await createFile(
        workspace.root,
        "big.js",
        "// pad\n" + "x".repeat(COMMAND_LIMITS.maxNodeScriptBytes),
      );
      await writeFile(path, "x".repeat(COMMAND_LIMITS.maxNodeScriptBytes + 1));
      const result = await prepare(workspace.root, {
        runner: "node-script",
        path: "big.js",
      });
      expect(result.status).toBe("invalid_input");
    } finally {
      await workspace.cleanup();
    }
  });
});

describe("node-script runner preparation and execution", () => {
  it("records a deterministic digest over the script hash", async () => {
    const workspace = await createTempWorkspace();
    try {
      await writeFixtureFiles(workspace.root, { "a.js": "console.log('one');" });
      const first = ready(await prepare(workspace.root, { runner: "node-script", path: "a.js" }));
      const second = ready(await prepare(workspace.root, { runner: "node-script", path: "a.js" }));
      expect(first.digest).toBe(second.digest);
      await writeFile(join(workspace.root, "a.js"), "console.log('two');");
      const changed = ready(await prepare(workspace.root, { runner: "node-script", path: "a.js" }));
      expect(changed.digest).not.toBe(first.digest);
    } finally {
      await workspace.cleanup();
    }
  });

  it("executes the immutable private copy of the approved script, never the workspace path", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner();
    try {
      await writeFixtureFiles(workspace.root, { "a.mjs": "console.log('ok');" });
      const prepared = ready(
        await runner.prepare(
          {
            runner: "node-script",
            path: "a.mjs",
            arguments: ["--flag", "tests/example test.ts"],
          },
          { workspaceRoot: workspace.root },
        ),
      );
      const runPaths = await createRunPaths();
      const execution = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths,
      });
      expect(execution.status).toBe("ready");
      if (execution.status !== "ready") {
        return;
      }
      expect(execution.request.executable).toBe(process.execPath);
      expect(execution.request.executableVersion).toBe(process.version);
      const index = scriptIndex(execution.request, runPaths.scriptCache);
      const scriptArgument = execution.request.arguments[index];
      expect(scriptArgument).toBeDefined();
      expect(scriptArgument?.startsWith(runPaths.scriptCache)).toBe(true);
      expect(scriptArgument).not.toContain(workspace.root);
      expect(execution.request.arguments.slice(index + 1)).toEqual([
        "--flag",
        "tests/example test.ts",
      ]);
      // The private copy carries the exact approved bytes.
      const privateBytes = await readFile(scriptArgument as string, "utf8");
      expect(privateBytes).toBe("console.log('ok');");
      expect(execution.request.digest).toBe(prepared.digest);
      // The workspace script can now be replaced freely; the executed input
      // is already staged and digest-bound.
      await writeFile(join(workspace.root, "a.mjs"), "console.log('MALICIOUS');");
      const staged = await readFile(join(runPaths.scriptCache, basename("a.mjs")), "utf8");
      expect(staged).toBe("console.log('ok');");
    } finally {
      await workspace.cleanup();
    }
  });

  it("places every self-containment flag before the staged script path", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner();
    try {
      await writeFixtureFiles(workspace.root, { "a.js": "console.log('ok');" });
      const prepared = ready(
        await runner.prepare(
          {
            runner: "node-script",
            path: "a.js",
            arguments: ["--user-flag"],
          },
          { workspaceRoot: workspace.root },
        ),
      );
      const runPaths = await createRunPaths();
      const execution = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths,
      });
      expect(execution.status).toBe("ready");
      if (execution.status !== "ready") {
        return;
      }
      const args = execution.request.arguments;
      expect(args[0]).toBe("--no-addons");
      expect(args[1]).toBe("--disallow-code-generation-from-strings");
      expect(args.indexOf("--import")).toBe(2);
      expect(args.indexOf("--require")).toBe(4);
      const esmUrl = args[3];
      expect(esmUrl).toBeDefined();
      expect(esmUrl?.startsWith("file://")).toBe(true);
      expect(esmUrl).toContain(SOLARIS_ESM_DENIAL_BOOTSTRAP_FILE);
      expect(args[5]).toBe(join(runPaths.scriptCache, SOLARIS_CJS_DENIAL_BOOTSTRAP_FILE));
      const index = scriptIndex(execution.request, runPaths.scriptCache);
      expect(index).toBe(6);
      expect(args[index]).toBe(join(runPaths.scriptCache, "a.js"));
      expect(args.slice(index + 1)).toEqual(["--user-flag"]);
    } finally {
      await workspace.cleanup();
    }
  });

  it("conflicts when a pre-existing private copy already occupies the staging path", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner();
    try {
      await writeFixtureFiles(workspace.root, { "a.js": "console.log('ok');" });
      const prepared = ready(
        await runner.prepare(
          { runner: "node-script", path: "a.js" },
          { workspaceRoot: workspace.root },
        ),
      );
      const runPaths = await createRunPaths();
      // A pre-existing entry at the staging name must never be overwritten.
      await writeFile(join(runPaths.scriptCache, "a.js"), "attacker bytes");
      const execution = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths,
      });
      expect(execution.status).toBe("conflict");
    } finally {
      await workspace.cleanup();
    }
  });

  it("conflicts when the staged private copy is replaced after staging", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner();
    try {
      await writeFixtureFiles(workspace.root, { "a.js": "console.log('one');" });
      const prepared = ready(
        await runner.prepare(
          { runner: "node-script", path: "a.js" },
          { workspaceRoot: workspace.root },
        ),
      );
      const runPaths = await createRunPaths();
      const first = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths,
      });
      expect(first.status).toBe("ready");
      // The staged private copy is replaced after staging; the replaced
      // bytes must never be executed by any subsequent execution attempt.
      const stagedPath = join(runPaths.scriptCache, "a.js");
      await writeFile(stagedPath, "console.log('attacker');");
      expect(await readFile(stagedPath, "utf8")).toBe("console.log('attacker');");
      const second = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths,
      });
      expect(second.status).toBe("conflict");
    } finally {
      await workspace.cleanup();
    }
  });

  it("conflicts when a Solaris bootstrap staging path is already occupied", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner();
    try {
      await writeFixtureFiles(workspace.root, { "a.js": "console.log('ok');" });
      const prepared = ready(
        await runner.prepare(
          { runner: "node-script", path: "a.js" },
          { workspaceRoot: workspace.root },
        ),
      );
      const runPaths = await createRunPaths();
      await writeFile(
        join(runPaths.scriptCache, SOLARIS_CJS_DENIAL_BOOTSTRAP_FILE),
        "attacker bytes",
      );
      const execution = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths,
      });
      expect(execution.status).toBe("conflict");
    } finally {
      await workspace.cleanup();
    }
  });

  it("conflicts when a workspace script name collides with a Solaris bootstrap name", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner();
    try {
      await writeFixtureFiles(workspace.root, {
        [SOLARIS_CJS_DENIAL_BOOTSTRAP_FILE]: "console.log('ok');",
      });
      const prepared = ready(
        await runner.prepare(
          { runner: "node-script", path: SOLARIS_CJS_DENIAL_BOOTSTRAP_FILE },
          { workspaceRoot: workspace.root },
        ),
      );
      const runPaths = await createRunPaths();
      const execution = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths,
      });
      expect(execution.status).toBe("conflict");
    } finally {
      await workspace.cleanup();
    }
  });

  it("conflicts when the script changes after approval", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner();
    try {
      await writeFixtureFiles(workspace.root, { "a.js": "console.log('one');" });
      const prepared = ready(
        await runner.prepare(
          { runner: "node-script", path: "a.js" },
          { workspaceRoot: workspace.root },
        ),
      );
      await writeFile(join(workspace.root, "a.js"), "console.log('two');");
      const execution = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths: await createRunPaths(),
      });
      expect(execution.status).toBe("conflict");
    } finally {
      await workspace.cleanup();
    }
  });

  it("binds arguments and working directory into the digest", async () => {
    const workspace = await createTempWorkspace();
    try {
      await writeFixtureFiles(workspace.root, {
        "a.js": "console.log('ok');",
        "sub/a.js": "console.log('ok');",
      });
      const plain = ready(await prepare(workspace.root, { runner: "node-script", path: "a.js" }));
      const withArgs = ready(
        await prepare(workspace.root, {
          runner: "node-script",
          path: "a.js",
          arguments: ["--flag"],
        }),
      );
      const subdir = ready(
        await prepare(workspace.root, {
          runner: "node-script",
          path: "a.js",
          workingDirectory: "sub",
        }),
      );
      expect(withArgs.digest).not.toBe(plain.digest);
      expect(subdir.digest).not.toBe(plain.digest);
    } finally {
      await workspace.cleanup();
    }
  });

  it("conflicts when the working directory changes after approval", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner();
    try {
      await writeFixtureFiles(workspace.root, {
        "a.js": "console.log('ok');",
        "sub/a.js": "console.log('ok');",
      });
      const prepared = ready(
        await runner.prepare(
          {
            runner: "node-script",
            path: "a.js",
            workingDirectory: "sub",
          },
          { workspaceRoot: workspace.root },
        ),
      );
      const execution = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths: await createRunPaths(),
      });
      expect(execution.status).toBe("ready");
    } finally {
      await workspace.cleanup();
    }
  });

  it("provides a minimal environment without NODE_OPTIONS or credentials", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner();
    try {
      await writeFixtureFiles(workspace.root, { "a.js": "console.log('ok');" });
      const prepared = ready(
        await runner.prepare(
          { runner: "node-script", path: "a.js" },
          { workspaceRoot: workspace.root },
        ),
      );
      const runPaths = await createRunPaths();
      const execution = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths,
      });
      expect(execution.status).toBe("ready");
      if (execution.status !== "ready") {
        return;
      }
      const environment = execution.request.environment;
      expect(environment["NODE_OPTIONS"]).toBeUndefined();
      expect(environment["OPENROUTER_API_KEY"]).toBeUndefined();
      expect(environment["NPM_TOKEN"]).toBeUndefined();
      expect(environment["HTTP_PROXY"]).toBeUndefined();
      expect(environment["HOME"]).toBe(runPaths.home);
      expect(environment["TEMP"]).toBe(runPaths.temp);
      expect(environment["NO_COLOR"]).toBe("1");
      expect(environment["NPM_CONFIG_CACHE"]).toBeUndefined();
      // The staged entry path reaches the Solaris-owned bootstraps privately.
      expect(environment["SOLARIS_STAGED_ENTRY"]).toBe(join(runPaths.scriptCache, "a.js"));
    } finally {
      await workspace.cleanup();
    }
  });

  it("conflicts when the working directory changes after approval", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner();
    try {
      await writeFixtureFiles(workspace.root, {
        "a.js": "console.log('ok');",
        "sub/a.js": "console.log('ok');",
      });
      const prepared = ready(
        await runner.prepare(
          {
            runner: "node-script",
            path: "a.js",
            workingDirectory: "sub",
          },
          { workspaceRoot: workspace.root },
        ),
      );
      const execution = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths: await createRunPaths(),
      });
      expect(execution.status).toBe("ready");
    } finally {
      await workspace.cleanup();
    }
  });

  it("refuses to execute a plan that did not originate from the runner", async () => {
    const workspace = await createTempWorkspace();
    try {
      await writeFixtureFiles(workspace.root, { "a.js": "console.log('ok');" });
      const runner = createRunner();
      const foreign = createPreparedCommand();
      const execution = await runner.toExecutionRequest(foreign, {
        approvedDigest: "deadbeef",
        runPaths: await createRunPaths(),
      });
      expect(execution.status).toBe("conflict");
    } finally {
      await workspace.cleanup();
    }
  });
});

describe("node-script runner approval preview", () => {
  it("describes the honest single-file executable boundary", async () => {
    const workspace = await createTempWorkspace();
    try {
      await writeFixtureFiles(workspace.root, { "a.js": "console.log('ok');" });
      const result = ready(await prepare(workspace.root, { runner: "node-script", path: "a.js" }));
      expect(result.preview.executionNotice).toContain("executes alone");
      expect(result.preview.executionNotice).toContain("self-contained");
      expect(result.preview.executionNotice).toContain("fails closed");
      expect(result.preview.executionNotice).toContain("SHA-256");
      expect(result.preview.executionNotice).not.toContain("continue through the workspace");
    } finally {
      await workspace.cleanup();
    }
  });
});

describe("node-script runner runtime code-loading denial (real spawns)", () => {
  it("runs a trivial script with no imports through the full flag stack", async () => {
    const workspace = await createTempWorkspace();
    try {
      await writeFixtureFiles(workspace.root, {
        "trivial.mjs":
          "console.log('trivial-ok');\nconsole.log(JSON.stringify(process.argv.slice(2)));",
      });
      const request = await prepareAndExecute(workspace.root, {
        runner: "node-script",
        path: "trivial.mjs",
        arguments: ["--flag", "value with space"],
      });
      const result = await executeRequest(request);
      expect(result.exitCode).toBe(0);
      expect(result.stdout).toContain("trivial-ok");
      expect(result.stdout).toContain(JSON.stringify(["--flag", "value with space"]));
      expect(result.stderr).not.toContain("DeprecationWarning");
    } finally {
      await workspace.cleanup();
    }
  });

  it("denies a relative static ESM import and never executes the imported module", async () => {
    const workspace = await createTempWorkspace();
    try {
      const sentinelRoot = await createSentinelRoot();
      const depSentinel = join(sentinelRoot, "dep-ran");
      await writeFixtureFiles(workspace.root, {
        "main.mjs": `import "./dep.mjs";\nconsole.log("main-ran");`,
        "dep.mjs": `const fs = process.getBuiltinModule("node:fs");\nfs.writeFileSync(${JSON.stringify(depSentinel)}, "dep-ran");`,
      });
      const request = await prepareAndExecute(workspace.root, {
        runner: "node-script",
        path: "main.mjs",
      });
      const result = await executeRequest(request);
      expect(result.exitCode).not.toBe(0);
      expect(result.stderr).toContain("SOLARIS_DENIED_LOAD");
      expect(result.stdout).not.toContain("main-ran");
      expect(existsSync(depSentinel)).toBe(false);
    } finally {
      await workspace.cleanup();
    }
  });

  it("denies a CommonJS require and never executes the required module", async () => {
    const workspace = await createTempWorkspace();
    try {
      const sentinelRoot = await createSentinelRoot();
      const okSentinel = join(sentinelRoot, "main-ok");
      const depSentinel = join(sentinelRoot, "dep-ran");
      await writeFixtureFiles(workspace.root, {
        "main.cjs": `const fs = process.getBuiltinModule("node:fs");\ntry {\n  require("./dep.cjs");\n  fs.writeFileSync(${JSON.stringify(okSentinel)}, "ok");\n} catch (error) {\n  console.log("DENIED", error.code);\n}`,
        "dep.cjs": `const fs = process.getBuiltinModule("node:fs");\nfs.writeFileSync(${JSON.stringify(depSentinel)}, "dep-ran");`,
      });
      const request = await prepareAndExecute(workspace.root, {
        runner: "node-script",
        path: "main.cjs",
      });
      const result = await executeRequest(request);
      expect(result.exitCode).toBe(0);
      expect(result.stdout).toContain("DENIED SOLARIS_DENIED_MODULE_LOAD");
      expect(existsSync(okSentinel)).toBe(false);
      expect(existsSync(depSentinel)).toBe(false);
    } finally {
      await workspace.cleanup();
    }
  });

  it("denies a dynamic import() in an ESM script", async () => {
    const workspace = await createTempWorkspace();
    try {
      const sentinelRoot = await createSentinelRoot();
      const okSentinel = join(sentinelRoot, "main-ok");
      const depSentinel = join(sentinelRoot, "dep-ran");
      await writeFixtureFiles(workspace.root, {
        "main.mjs": `const fs = process.getBuiltinModule("node:fs");\ntry {\n  await import("./dep.mjs");\n  fs.writeFileSync(${JSON.stringify(okSentinel)}, "ok");\n} catch (error) {\n  console.log("DENIED", error.code);\n}`,
        "dep.mjs": `const fs = process.getBuiltinModule("node:fs");\nfs.writeFileSync(${JSON.stringify(depSentinel)}, "dep-ran");`,
      });
      const request = await prepareAndExecute(workspace.root, {
        runner: "node-script",
        path: "main.mjs",
      });
      const result = await executeRequest(request);
      expect(result.exitCode).toBe(0);
      expect(result.stdout).toContain("DENIED SOLARIS_DENIED_LOAD");
      expect(existsSync(okSentinel)).toBe(false);
      expect(existsSync(depSentinel)).toBe(false);
    } finally {
      await workspace.cleanup();
    }
  });

  it("denies a dynamic import() in a CommonJS script", async () => {
    const workspace = await createTempWorkspace();
    try {
      const sentinelRoot = await createSentinelRoot();
      const okSentinel = join(sentinelRoot, "main-ok");
      const depSentinel = join(sentinelRoot, "dep-ran");
      await writeFixtureFiles(workspace.root, {
        "main.cjs": `const fs = process.getBuiltinModule("node:fs");\nimport("./dep.mjs").then(\n  () => fs.writeFileSync(${JSON.stringify(okSentinel)}, "ok"),\n  (error) => console.log("DENIED", error.code),\n);`,
        "dep.mjs": `const fs = process.getBuiltinModule("node:fs");\nfs.writeFileSync(${JSON.stringify(depSentinel)}, "dep-ran");`,
      });
      const request = await prepareAndExecute(workspace.root, {
        runner: "node-script",
        path: "main.cjs",
      });
      const result = await executeRequest(request);
      expect(result.exitCode).toBe(0);
      expect(result.stdout).toContain("DENIED SOLARIS_DENIED_LOAD");
      expect(existsSync(okSentinel)).toBe(false);
      expect(existsSync(depSentinel)).toBe(false);
    } finally {
      await workspace.cleanup();
    }
  });

  it("denies an absolute-path import", async () => {
    const workspace = await createTempWorkspace();
    try {
      const sentinelRoot = await createSentinelRoot();
      const okSentinel = join(sentinelRoot, "main-ok");
      const depSentinel = join(sentinelRoot, "dep-ran");
      await writeFixtureFiles(workspace.root, {
        "main.mjs": `const fs = process.getBuiltinModule("node:fs");\ntry {\n  await import(process.argv[2]);\n  fs.writeFileSync(${JSON.stringify(okSentinel)}, "ok");\n} catch (error) {\n  console.log("DENIED", error.code);\n}`,
        "dep.mjs": `const fs = process.getBuiltinModule("node:fs");\nfs.writeFileSync(${JSON.stringify(depSentinel)}, "dep-ran");`,
      });
      const depUrl = pathToFileURL(join(workspace.root, "dep.mjs")).href;
      const request = await prepareAndExecute(workspace.root, {
        runner: "node-script",
        path: "main.mjs",
        arguments: [depUrl],
      });
      const result = await executeRequest(request);
      expect(result.exitCode).toBe(0);
      expect(result.stdout).toContain("DENIED SOLARIS_DENIED_LOAD");
      expect(existsSync(okSentinel)).toBe(false);
      expect(existsSync(depSentinel)).toBe(false);
    } finally {
      await workspace.cleanup();
    }
  });

  it("denies a CWD-based module load", async () => {
    const workspace = await createTempWorkspace();
    try {
      const sentinelRoot = await createSentinelRoot();
      const okSentinel = join(sentinelRoot, "main-ok");
      const depSentinel = join(sentinelRoot, "dep-ran");
      await writeFixtureFiles(workspace.root, {
        "main.cjs": `const fs = process.getBuiltinModule("node:fs");\ntry {\n  require(process.cwd() + "/cwd-helper.cjs");\n  fs.writeFileSync(${JSON.stringify(okSentinel)}, "ok");\n} catch (error) {\n  console.log("DENIED", error.code);\n}`,
        "cwd-helper.cjs": `const fs = process.getBuiltinModule("node:fs");\nfs.writeFileSync(${JSON.stringify(depSentinel)}, "dep-ran");`,
      });
      const request = await prepareAndExecute(workspace.root, {
        runner: "node-script",
        path: "main.cjs",
      });
      const result = await executeRequest(request);
      expect(result.exitCode).toBe(0);
      expect(result.stdout).toContain("DENIED SOLARIS_DENIED_MODULE_LOAD");
      expect(existsSync(okSentinel)).toBe(false);
      expect(existsSync(depSentinel)).toBe(false);
    } finally {
      await workspace.cleanup();
    }
  });

  it("denies new Worker() construction even through process.getBuiltinModule", async () => {
    const workspace = await createTempWorkspace();
    try {
      const sentinelRoot = await createSentinelRoot();
      const okSentinel = join(sentinelRoot, "main-ok");
      const workerSentinel = join(sentinelRoot, "worker-ran");
      await writeFixtureFiles(workspace.root, {
        "main.cjs": `const fs = process.getBuiltinModule("node:fs");\ntry {\n  const workerThreads = process.getBuiltinModule("node:worker_threads");\n  new workerThreads.Worker(process.argv[2]);\n  fs.writeFileSync(${JSON.stringify(okSentinel)}, "ok");\n} catch (error) {\n  console.log("DENIED", error.code);\n}`,
        "worker-target.cjs": `const fs = process.getBuiltinModule("node:fs");\nfs.writeFileSync(${JSON.stringify(workerSentinel)}, "worker-ran");`,
      });
      const request = await prepareAndExecute(workspace.root, {
        runner: "node-script",
        path: "main.cjs",
        arguments: [join(workspace.root, "worker-target.cjs")],
      });
      const result = await executeRequest(request);
      expect(result.exitCode).toBe(0);
      expect(result.stdout).toContain("DENIED SOLARIS_DENIED_MODULE_LOAD");
      expect(existsSync(okSentinel)).toBe(false);
      expect(existsSync(workerSentinel)).toBe(false);
    } finally {
      await workspace.cleanup();
    }
  });

  it("denies eval at runtime", async () => {
    const workspace = await createTempWorkspace();
    try {
      await writeFixtureFiles(workspace.root, {
        "main.cjs": `try {\n  eval("1 + 1");\n  console.log("EVAL-OK");\n} catch (error) {\n  console.log("DENIED", error.name);\n}`,
      });
      const request = await prepareAndExecute(workspace.root, {
        runner: "node-script",
        path: "main.cjs",
      });
      const result = await executeRequest(request);
      expect(result.exitCode).toBe(0);
      expect(result.stdout).toContain("DENIED EvalError");
      expect(result.stdout).not.toContain("EVAL-OK");
    } finally {
      await workspace.cleanup();
    }
  });

  it("denies new Function at runtime", async () => {
    const workspace = await createTempWorkspace();
    try {
      await writeFixtureFiles(workspace.root, {
        "main.cjs": `try {\n  new Function("return 1")();\n  console.log("FN-OK");\n} catch (error) {\n  console.log("DENIED", error.name);\n}`,
      });
      const request = await prepareAndExecute(workspace.root, {
        runner: "node-script",
        path: "main.cjs",
      });
      const result = await executeRequest(request);
      expect(result.exitCode).toBe(0);
      expect(result.stdout).toContain("DENIED EvalError");
      expect(result.stdout).not.toContain("FN-OK");
    } finally {
      await workspace.cleanup();
    }
  });
});
