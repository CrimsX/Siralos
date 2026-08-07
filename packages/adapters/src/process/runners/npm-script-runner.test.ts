import { describe, expect, it } from "vitest";
import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import {
  COMMAND_LIMITS,
  type CommandExecutionContext,
  type CommandPreparationResult,
  type CommandRunner,
} from "@solaris/core";
import { createSha256CommandDigestService } from "../command-digest.js";
import { createNpmScriptRunner } from "./npm-script-runner.js";
import {
  createFile,
  createSymlink,
  createTempWorkspace,
  SYMLINKS_SUPPORTED,
} from "../../tools/workspace/workspace-fixtures.js";

const RUN_PATHS: CommandExecutionContext["runPaths"] = {
  runId: "run-1",
  home: "/run/home",
  temp: "/run/tmp",
  npmCache: "/run/npm-cache",
  npmUserConfig: "/run/npmrc",
};

const PACKAGE_JSON = {
  name: "fixture-package",
  scripts: {
    check: "npm run format:check && npm run lint",
    test: "vitest run",
    empty: "",
  },
};

function createRunner(
  npmResolver?: () => Promise<
    | { status: "resolved"; cliPath: string; version: string }
    | { status: "unavailable"; message: string }
  >,
): CommandRunner {
  return createNpmScriptRunner({
    digest: createSha256CommandDigestService(),
    ...(npmResolver === undefined ? {} : { npmResolver }),
  });
}

function fakeResolver(
  cliPath = "/trusted/npm-cli.js",
  version = "11.0.0",
): () => Promise<{ status: "resolved"; cliPath: string; version: string }> {
  return () => Promise.resolve({ status: "resolved" as const, cliPath, version });
}

async function prepare(
  runner: CommandRunner,
  workspaceRoot: string,
  input: unknown,
): Promise<CommandPreparationResult> {
  return runner.prepare(input, { workspaceRoot });
}

function ready(
  result: CommandPreparationResult,
): Extract<CommandPreparationResult, { status: "ready" }> {
  if (result.status !== "ready") {
    throw new Error(`Expected ready, got ${result.status}: ${result.message}`);
  }
  return result;
}

async function createPackageWorkspace(scripts: Record<string, string> = PACKAGE_JSON.scripts) {
  const workspace = await createTempWorkspace();
  await writeFile(
    join(workspace.root, "package.json"),
    JSON.stringify({ name: "fixture-package", scripts }),
  );
  return workspace;
}

describe("npm-script runner package validation", () => {
  it("prepares a valid script with the exact preview", async () => {
    const workspace = await createPackageWorkspace();
    const runner = createRunner(fakeResolver());
    try {
      const result = ready(
        await prepare(runner, workspace.root, {
          runner: "npm-script",
          script: "check",
        }),
      );
      expect(result.preview.runnerId).toBe("npm-script");
      expect(result.preview.displayName).toBe("npm run check");
      expect(result.preview.packageName).toBe("fixture-package");
      expect(result.preview.scriptName).toBe("check");
      expect(result.preview.repositoryScript).toBe("npm run format:check && npm run lint");
      expect(result.preview.workingDirectory).toBe(".");
      expect(result.preview.arguments).toEqual(["run", "check", "--"]);
      expect(result.preview.workspaceAccess).toBe("read-only");
      expect(result.preview.networkAccess).toBe("denied");
      expect(result.preview.stdinPolicy).toBe("closed");
      expect(result.preview.timeoutMs).toBe(COMMAND_LIMITS.defaultTimeoutMs);
      expect(result.preview.hooksNotice).toBe(
        "Automatically associated precheck and postcheck scripts are disabled.",
      );
    } finally {
      await workspace.cleanup();
    }
  });

  it("fails when no package.json exists", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner(fakeResolver());
    try {
      const result = await prepare(runner, workspace.root, {
        runner: "npm-script",
        script: "check",
      });
      expect(result.status).toBe("invalid_input");
    } finally {
      await workspace.cleanup();
    }
  });

  it("fails when the package JSON is invalid", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner(fakeResolver());
    try {
      await writeFile(join(workspace.root, "package.json"), "{ not json");
      const result = await prepare(runner, workspace.root, {
        runner: "npm-script",
        script: "check",
      });
      expect(result.status).toBe("invalid_input");
    } finally {
      await workspace.cleanup();
    }
  });

  it("fails when the scripts object is missing", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner(fakeResolver());
    try {
      await writeFile(join(workspace.root, "package.json"), JSON.stringify({ name: "x" }));
      const result = await prepare(runner, workspace.root, {
        runner: "npm-script",
        script: "check",
      });
      expect(result.status).toBe("invalid_input");
    } finally {
      await workspace.cleanup();
    }
  });

  it("fails for unknown, missing, and empty scripts", async () => {
    const workspace = await createPackageWorkspace();
    const runner = createRunner(fakeResolver());
    try {
      const unknown = await prepare(runner, workspace.root, {
        runner: "npm-script",
        script: "nope",
      });
      expect(unknown.status).toBe("invalid_input");
      const missing = await prepare(runner, workspace.root, { runner: "npm-script" });
      expect(missing.status).toBe("invalid_input");
      const empty = await prepare(runner, workspace.root, {
        runner: "npm-script",
        script: "empty",
      });
      expect(empty.status).toBe("invalid_input");
    } finally {
      await workspace.cleanup();
    }
  });

  it("fails for a symbolic-linked package.json", async () => {
    if (!SYMLINKS_SUPPORTED) {
      return;
    }
    const workspace = await createTempWorkspace();
    const runner = createRunner(fakeResolver());
    try {
      await writeFile(
        join(workspace.root, "real-package.json"),
        JSON.stringify({ name: "x", scripts: { check: "true" } }),
      );
      await createSymlink(
        join(workspace.root, "real-package.json"),
        join(workspace.root, "package.json"),
      );
      const result = await prepare(runner, workspace.root, {
        runner: "npm-script",
        script: "check",
      });
      expect(result.status).toBe("invalid_input");
    } finally {
      await workspace.cleanup();
    }
  });

  it("fails for an oversized package.json", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner(fakeResolver());
    try {
      await writeFile(
        join(workspace.root, "package.json"),
        JSON.stringify({
          name: "big",
          scripts: { check: "true" },
          padding: "x".repeat(COMMAND_LIMITS.maxPackageJsonBytes),
        }),
      );
      const result = await prepare(runner, workspace.root, {
        runner: "npm-script",
        script: "check",
      });
      expect(result.status).toBe("invalid_input");
    } finally {
      await workspace.cleanup();
    }
  });

  it("fails for an oversized script body", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner(fakeResolver());
    try {
      await writeFile(
        join(workspace.root, "package.json"),
        JSON.stringify({
          name: "big",
          scripts: { check: "x".repeat(COMMAND_LIMITS.maxNpmScriptBytes + 1) },
        }),
      );
      const result = await prepare(runner, workspace.root, {
        runner: "npm-script",
        script: "check",
      });
      expect(result.status).toBe("invalid_input");
    } finally {
      await workspace.cleanup();
    }
  });

  it("rejects arbitrary npm subcommands and registry overrides", async () => {
    const workspace = await createPackageWorkspace();
    const runner = createRunner(fakeResolver());
    try {
      for (const input of [
        { runner: "npm-script", script: "check", command: "install" },
        { runner: "npm-script", script: "check", subcommand: "exec" },
        { runner: "npm-script", script: "check", registry: "https://evil.example" },
        { runner: "npm-script", script: "check", prefix: "/tmp/x" },
        { runner: "npm-script", script: "check", global: true },
        { runner: "npm-script", script: "check", environment: { NPM_CONFIG_REGISTRY: "x" } },
        { runner: "npm-script", script: "check", network: true },
        { runner: "npm-script", script: "check", writablePaths: ["/tmp"] },
        { runner: "npm-script", script: "check", shell: "cmd.exe" },
      ]) {
        const result = await prepare(runner, workspace.root, input);
        expect(result.status).toBe("invalid_input");
      }
    } finally {
      await workspace.cleanup();
    }
  });

  it("records a deterministic digest over the package hash", async () => {
    const workspace = await createPackageWorkspace();
    const runner = createRunner(fakeResolver());
    try {
      const first = ready(
        await prepare(runner, workspace.root, { runner: "npm-script", script: "check" }),
      );
      const second = ready(
        await prepare(runner, workspace.root, { runner: "npm-script", script: "check" }),
      );
      expect(first.digest).toBe(second.digest);
      await writeFile(
        join(workspace.root, "package.json"),
        JSON.stringify({ name: "fixture-package", scripts: { check: "changed" } }),
      );
      const changed = ready(
        await prepare(runner, workspace.root, { runner: "npm-script", script: "check" }),
      );
      expect(changed.digest).not.toBe(first.digest);
    } finally {
      await workspace.cleanup();
    }
  });

  it("marks the runner unavailable when npm resolution fails", async () => {
    const workspace = await createPackageWorkspace();
    const runner = createRunner(() =>
      Promise.resolve({ status: "unavailable" as const, message: "npm is missing." }),
    );
    try {
      const result = await prepare(runner, workspace.root, {
        runner: "npm-script",
        script: "check",
      });
      expect(result.status).toBe("unavailable");
      expect(await runner.isAvailable()).toBe(false);
    } finally {
      await workspace.cleanup();
    }
  });
});

describe("npm-script runner execution", () => {
  it("invokes npm-cli.js through the trusted Node executable with separate arguments", async () => {
    const workspace = await createPackageWorkspace();
    const runner = createRunner(fakeResolver("C:\\trusted\\npm-cli.js", "11.7.0"));
    try {
      const prepared = ready(
        await prepare(runner, workspace.root, {
          runner: "npm-script",
          script: "check",
          arguments: ["--runInBand", "tests/example test.ts"],
        }),
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
      expect(execution.request.arguments[0]).toBe("C:\\trusted\\npm-cli.js");
      expect(execution.request.arguments.slice(1)).toEqual([
        "run",
        "check",
        "--",
        "--runInBand",
        "tests/example test.ts",
      ]);
      expect(execution.request.executableIdentity).toContain("npm 11.7.0");
      expect(execution.request.digest).toBe(prepared.digest);
    } finally {
      await workspace.cleanup();
    }
  });

  it("never uses a .cmd wrapper and disables pre/post hooks via npm config", async () => {
    const workspace = await createPackageWorkspace();
    const runner = createRunner(fakeResolver());
    try {
      const prepared = ready(
        await prepare(runner, workspace.root, { runner: "npm-script", script: "test" }),
      );
      const execution = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths: RUN_PATHS,
      });
      expect(execution.status).toBe("ready");
      if (execution.status !== "ready") {
        return;
      }
      expect(
        execution.request.arguments.some((argument) => argument.toLowerCase().endsWith(".cmd")),
      ).toBe(false);
      const environment = execution.request.environment;
      expect(environment["NPM_CONFIG_IGNORE_SCRIPTS"]).toBe("true");
      expect(environment["NPM_CONFIG_AUDIT"]).toBe("false");
      expect(environment["NPM_CONFIG_FUND"]).toBe("false");
      expect(environment["NPM_CONFIG_UPDATE_NOTIFIER"]).toBe("false");
      expect(environment["NPM_CONFIG_COLOR"]).toBe("false");
      expect(environment["NPM_CONFIG_SCRIPT_SHELL"]).toBeUndefined();
      expect(environment["NPM_CONFIG_USERCONFIG"]).toBe(RUN_PATHS.npmUserConfig);
      expect(environment["NPM_CONFIG_CACHE"]).toBe(RUN_PATHS.npmCache);
      expect(environment["HOME"]).toBe(RUN_PATHS.home);
      expect(environment["NO_COLOR"]).toBe("1");
      expect(environment["NODE_OPTIONS"]).toBeUndefined();
      expect(environment["NPM_TOKEN"]).toBeUndefined();
    } finally {
      await workspace.cleanup();
    }
  });

  it("conflicts when the package.json changes after approval", async () => {
    const workspace = await createPackageWorkspace();
    const runner = createRunner(fakeResolver());
    try {
      const prepared = ready(
        await prepare(runner, workspace.root, { runner: "npm-script", script: "check" }),
      );
      await writeFile(
        join(workspace.root, "package.json"),
        JSON.stringify({ name: "fixture-package", scripts: { check: "changed" } }),
      );
      const execution = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths: RUN_PATHS,
      });
      expect(execution.status).toBe("conflict");
    } finally {
      await workspace.cleanup();
    }
  });

  it("conflicts when the script value changes after approval", async () => {
    const workspace = await createPackageWorkspace();
    const runner = createRunner(fakeResolver());
    try {
      const prepared = ready(
        await prepare(runner, workspace.root, { runner: "npm-script", script: "test" }),
      );
      await writeFile(
        join(workspace.root, "package.json"),
        JSON.stringify({
          name: "fixture-package",
          scripts: { check: "x", test: "vitest run --changed" },
        }),
      );
      const execution = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths: RUN_PATHS,
      });
      expect(execution.status).toBe("conflict");
    } finally {
      await workspace.cleanup();
    }
  });

  it("conflicts when the trusted npm CLI changes after approval", async () => {
    const workspace = await createPackageWorkspace();
    let current = fakeResolver("C:\\trusted\\npm-cli.js", "11.7.0");
    const runner = createNpmScriptRunner({
      digest: createSha256CommandDigestService(),
      npmResolver: () => current(),
    });
    try {
      const prepared = ready(
        await prepare(runner, workspace.root, { runner: "npm-script", script: "check" }),
      );
      current = fakeResolver("C:\\trusted\\npm-cli.js", "11.8.0");
      const execution = await runner.toExecutionRequest(prepared.command, {
        approvedDigest: prepared.digest,
        runPaths: RUN_PATHS,
      });
      expect(execution.status).toBe("conflict");
    } finally {
      await workspace.cleanup();
    }
  });

  it("refuses to execute a plan that did not originate from the runner", async () => {
    const workspace = await createPackageWorkspace();
    const runner = createRunner(fakeResolver());
    try {
      const { createPreparedCommand } = await import("@solaris/core");
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

describe("npm-script runner working directory", () => {
  it("validates a package in a subdirectory", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner(fakeResolver());
    try {
      await createFile(
        workspace.root,
        "packages/core/package.json",
        JSON.stringify({ name: "@solaris/core", scripts: { check: "tsc -b" } }),
      );
      const result = ready(
        await prepare(runner, workspace.root, {
          runner: "npm-script",
          script: "check",
          workingDirectory: "packages/core",
        }),
      );
      expect(result.preview.packageName).toBe("@solaris/core");
      expect(result.preview.workingDirectory).toBe("packages/core");
    } finally {
      await workspace.cleanup();
    }
  });

  it("does not search parent directories for a package", async () => {
    const workspace = await createTempWorkspace();
    const runner = createRunner(fakeResolver());
    try {
      await createFile(workspace.root, "sub/placeholder.txt", "x");
      await writeFile(
        join(workspace.root, "package.json"),
        JSON.stringify({ name: "root", scripts: { check: "true" } }),
      );
      const result = await prepare(runner, workspace.root, {
        runner: "npm-script",
        script: "check",
        workingDirectory: "sub",
      });
      expect(result.status).toBe("invalid_input");
    } finally {
      await workspace.cleanup();
    }
  });
});
