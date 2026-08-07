import { describe, expect, it } from "vitest";
import type { CommandPreparationResult } from "@solaris/core";
import { createSha256CommandDigestService } from "../command-digest.js";
import { createNpmScriptRunner } from "./npm-script-runner.js";

function createRunner() {
  return createNpmScriptRunner({ digest: createSha256CommandDigestService() });
}

async function prepare(input: unknown): Promise<CommandPreparationResult> {
  return createRunner().prepare(input, { workspaceRoot: "/workspace" });
}

describe("npm-script runner fail-closed contract", () => {
  it("refuses every request because npm cannot be bound to the approved package bytes", async () => {
    const result = await prepare({
      runner: "npm-script",
      script: "check",
      arguments: [],
      workingDirectory: ".",
    });
    expect(result.status).toBe("unavailable");
    if (result.status === "unavailable") {
      expect(result.message).toContain("npm-script runner is unavailable");
      expect(result.message).toContain("package.json");
    }
  });

  it("refuses regardless of the requested script or arguments", async () => {
    for (const input of [
      { runner: "npm-script", script: "check" },
      { runner: "npm-script", script: "test", arguments: ["--watch"] },
      { runner: "npm-script", script: "deploy", workingDirectory: "packages/app" },
    ]) {
      const result = await prepare(input);
      expect(result.status).toBe("unavailable");
    }
  });

  it("returns unavailable from execution requests and availability checks", async () => {
    const runner = createRunner();
    expect(await runner.isAvailable()).toBe(false);
    const execution = await runner.toExecutionRequest(
      { ["plan-brand"]: true } as never,
      { approvedDigest: "deadbeef", runPaths: {} } as never,
    );
    expect(execution.status).toBe("unavailable");
  });

  it("still respects cancellation during preparation", async () => {
    const controller = new AbortController();
    controller.abort();
    const result = await createRunner().prepare(
      { runner: "npm-script", script: "check" },
      { workspaceRoot: "/workspace", signal: controller.signal },
    );
    expect(result.status).toBe("cancelled");
  });
});
