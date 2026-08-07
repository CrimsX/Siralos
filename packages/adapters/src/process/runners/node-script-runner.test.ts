import { describe, expect, it } from "vitest";
import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import {
  COMMAND_LIMITS,
  createPreparedCommand,
  type CommandExecutionContext,
  type CommandPreparationResult,
} from "@solaris/core";
import { createSha256CommandDigestService } from "../command-digest.js";
import { createNodeScriptRunner } from "./node-script-runner.js";
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

const RUN_PATHS: CommandExecutionContext["runPaths"] = {
  runId: "run-1",
  home: "/run/home",
  temp: "/run/tmp",
  npmCache: "/run/npm-cache",
  npmUserConfig: "/run/npmrc",
};

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

  it("uses the trusted process.execPath with separate arguments", async () => {
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
      const execution = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths: RUN_PATHS,
      });
      expect(execution.status).toBe("ready");
      if (execution.status !== "ready") {
        return;
      }
      expect(execution.request.executable).toBe(process.execPath);
      expect(execution.request.executableVersion).toBe(process.version);
      expect(execution.request.arguments).toEqual([
        join(workspace.root, "a.mjs"),
        "--flag",
        "tests/example test.ts",
      ]);
      expect(execution.request.digest).toBe(prepared.digest);
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
      const execution = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths: RUN_PATHS,
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
      expect(environment["HOME"]).toBe(RUN_PATHS.home);
      expect(environment["TEMP"]).toBe(RUN_PATHS.temp);
      expect(environment["NO_COLOR"]).toBe("1");
      expect(environment["NPM_CONFIG_CACHE"]).toBeUndefined();
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
        runPaths: RUN_PATHS,
      });
      expect(execution.status).toBe("conflict");
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
        runPaths: RUN_PATHS,
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
        runPaths: RUN_PATHS,
      });
      expect(execution.status).toBe("conflict");
    } finally {
      await workspace.cleanup();
    }
  });
});
