import { describe, expect, it } from "vitest";
import { createSha256CommandDigestService } from "../command-digest.js";
import { createNodeScriptRunner } from "./node-script-runner.js";

describe("node-script runner fail-closed availability", () => {
  function runner() {
    return createNodeScriptRunner({ digest: createSha256CommandDigestService() });
  }

  it("reports unavailable", async () => {
    expect(await runner().isAvailable()).toBe(false);
  });

  it("refuses every preparation before any approval", async () => {
    const result = await runner().prepare(
      { runner: "node-script", path: "scripts/validate.mjs", arguments: [] },
      { workspaceRoot: "/workspace" },
    );
    expect(result.status).toBe("unavailable");
    if (result.status === "unavailable") {
      expect(result.message).toContain("process.binding");
      expect(result.message).toContain("final verification and launch");
    }
  });

  it("refuses execution requests", async () => {
    const result = await runner().toExecutionRequest({} as never, {
      approvedDigest: "d".repeat(64),
      runPaths: {} as never,
    });
    expect(result.status).toBe("unavailable");
  });

  it("honours cancellation before the unavailable decision", async () => {
    const controller = new AbortController();
    controller.abort();
    const result = await runner().prepare(
      { runner: "node-script", path: "scripts/validate.mjs" },
      { workspaceRoot: "/workspace", signal: controller.signal },
    );
    expect(result.status).toBe("cancelled");
  });
});
