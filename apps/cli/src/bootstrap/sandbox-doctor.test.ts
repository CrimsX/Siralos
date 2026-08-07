import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { doctorExitCode, runSandboxDoctor } from "./sandbox-doctor.js";
import { createFakeSandboxBackend } from "@solaris/adapters";
import type { ConformanceReport } from "@solaris/adapters";

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

function fakeReport(
  overrides: Partial<{ probesRun: boolean; failed: number; passed: number; skipped: number }> = {},
): ReturnType<typeof runSandboxDoctor> extends Promise<infer T> ? T : never {
  return {
    profileId: "develop-offline",
    backendId: "fake-backend",
    backendVersion: "0.0.0",
    platform: "linux",
    state: "available",
    capabilities: {
      filesystemReadRestriction: true,
      filesystemWriteRestriction: true,
      networkRestriction: true,
      processTreeRestriction: true,
      violationReporting: true,
    },
    statusMessage: null,
    probesRun: overrides.probesRun ?? false,
    conformance:
      overrides.probesRun === true
        ? {
            backendId: "fake-backend",
            platform: "linux",
            profileId: "develop-offline",
            results: [],
            passed: overrides.passed ?? 0,
            failed: overrides.failed ?? 0,
            skipped: overrides.skipped ?? 0,
          }
        : null,
  };
}

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

  it("configures the backend with the disposable root used by live probes", async () => {
    const directory = await withTempDirectory();
    const configPath = join(directory, "config.json");
    await writeFile(
      configPath,
      JSON.stringify({ sandbox: { profile: "develop-offline", backend: "auto" } }),
    );
    const backendRoots: string[] = [];
    const probeRoots: string[] = [];
    const backend = createFakeSandboxBackend({
      status: {
        backendId: "fake-backend",
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
      },
    });
    const conformanceRunner = (
      _target: unknown,
      options: { workspaceRoot: string },
    ): Promise<ConformanceReport> => {
      probeRoots.push(options.workspaceRoot);
      return Promise.resolve({
        backendId: "fake-backend",
        platform: "linux",
        profileId: "develop-offline",
        results: [],
        passed: 0,
        failed: 0,
        skipped: 0,
      });
    };
    const report = await runSandboxDoctor({
      includeProbes: true,
      configPath,
      backendFactory: (root) => {
        backendRoots.push(root);
        return backend.backend;
      },
      conformanceRunner,
    });
    expect(report.probesRun).toBe(true);
    expect(backendRoots).toHaveLength(1);
    expect(probeRoots).toHaveLength(1);
    expect(backendRoots[0]).toBe(probeRoots[0]);
    expect(backendRoots[0]).toMatch(/solaris-doctor-probes-/);
  });

  it("does not run probes when the backend is not available", async () => {
    const directory = await withTempDirectory();
    const configPath = join(directory, "config.json");
    await writeFile(
      configPath,
      JSON.stringify({ sandbox: { profile: "develop-offline", backend: "auto" } }),
    );
    const backend = createFakeSandboxBackend({
      status: {
        backendId: "fake-backend",
        state: "setup-required",
        platform: "linux",
        version: "0.0.0",
        capabilities: {
          filesystemReadRestriction: false,
          filesystemWriteRestriction: false,
          networkRestriction: false,
          processTreeRestriction: false,
          violationReporting: false,
        },
        message: "setup required",
      },
    });
    const report = await runSandboxDoctor({
      includeProbes: true,
      configPath,
      backendFactory: () => backend.backend,
    });
    expect(report.probesRun).toBe(false);
    expect(report.conformance).toBeNull();
    expect(report.state).toBe("setup-required");
  });
});

describe("doctorExitCode", () => {
  it("returns zero when probes were not requested", () => {
    expect(doctorExitCode(fakeReport({ probesRun: false }), false)).toBe(0);
  });

  it("returns three when requested probes could not run", () => {
    expect(doctorExitCode(fakeReport({ probesRun: false }), true)).toBe(3);
  });

  it("returns one when a requested probe failed", () => {
    expect(doctorExitCode(fakeReport({ probesRun: true, failed: 2, passed: 5 }), true)).toBe(1);
  });

  it("returns zero when all requested probes passed", () => {
    expect(doctorExitCode(fakeReport({ probesRun: true, failed: 0, passed: 8 }), true)).toBe(0);
  });
});
