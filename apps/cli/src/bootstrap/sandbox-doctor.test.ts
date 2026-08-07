import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { runSandboxDoctor } from "./sandbox-doctor.js";

const tempDirectories: string[] = [];

async function withTempDirectory(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "solaris-doctor-test-"));
  tempDirectories.push(directory);
  return directory;
}

afterEach(async () => {
  for (const directory of tempDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
});

describe("runSandboxDoctor", () => {
  it("reports profile, backend, and status without running probes", async () => {
    const directory = await withTempDirectory();
    const report = await runSandboxDoctor({
      includeProbes: false,
      configPath: join(directory, "missing-config.json"),
      workspaceRoot: directory,
    });
    expect(report.profileId).toBe("inspect");
    expect(report.backendId).toBe("anthropic-runtime");
    expect(report.backendVersion).toMatch(/^\d+\.\d+\.\d+$/);
    expect(typeof report.platform).toBe("string");
    expect([
      "available",
      "setup-required",
      "dependency-missing",
      "unsupported",
      "degraded",
      "failed",
    ]).toContain(report.state);
    expect(Object.values(report.capabilities).every((value) => typeof value === "boolean")).toBe(
      true,
    );
    expect(report.probesRun).toBe(false);
    expect(report.conformance).toBeNull();
  });

  it("does not execute probes without an explicit request", async () => {
    const directory = await withTempDirectory();
    const report = await runSandboxDoctor({
      includeProbes: false,
      configPath: join(directory, "missing-config.json"),
      workspaceRoot: directory,
    });
    expect(report.probesRun).toBe(false);
    expect(report.conformance).toBeNull();
  });

  it("reflects a develop-offline user configuration", async () => {
    const directory = await withTempDirectory();
    const configPath = join(directory, "config.json");
    await writeFile(
      configPath,
      JSON.stringify({ sandbox: { profile: "develop-offline", backend: "auto" } }),
    );
    const report = await runSandboxDoctor({
      includeProbes: false,
      configPath,
      workspaceRoot: directory,
    });
    expect(report.profileId).toBe("develop-offline");
  });
});
