import { readFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  DEVELOP_OFFLINE_PROFILE,
  INSPECT_PROFILE,
  SandboxError,
  type SandboxBackendStatus,
  type SandboxedProcessResult,
} from "@solaris/core";
import {
  ANTHROPIC_SANDBOX_RUNTIME_BACKEND_ID,
  ANTHROPIC_SANDBOX_RUNTIME_VERSION,
  createAnthropicSandboxRuntimeBackend,
} from "./anthropic-sandbox-runtime-backend.js";
import { completedResult, createFakeSandboxBackend } from "../fake-sandbox-backend.js";

const tempDirectories: string[] = [];

async function withTempWorkspace(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "solaris-sandbox-test-"));
  tempDirectories.push(directory);
  return directory;
}

afterEach(async () => {
  for (const directory of tempDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
});

function createBackend(workspaceRoot: string) {
  return createAnthropicSandboxRuntimeBackend({
    workspaceRoot,
    sandboxHome: join(workspaceRoot, ".sandbox-home"),
    sandboxTemp: join(workspaceRoot, ".sandbox-temp"),
  });
}

describe("Anthropic Sandbox Runtime backend", () => {
  it("reports a structurally valid status with the pinned version", async () => {
    const workspace = await withTempWorkspace();
    const backend = createBackend(workspace);
    const status = await backend.inspect();
    expect(status.backendId).toBe(ANTHROPIC_SANDBOX_RUNTIME_BACKEND_ID);
    expect(status.version).toBe(ANTHROPIC_SANDBOX_RUNTIME_VERSION);
    expect([
      "available",
      "setup-required",
      "dependency-missing",
      "unsupported",
      "degraded",
      "failed",
    ]).toContain(status.state);
    expect(typeof status.platform).toBe("string");
    expect(Object.values(status.capabilities).every((value) => typeof value === "boolean")).toBe(
      true,
    );
  });

  it("matches the pinned package version", () => {
    const packageJsonPath = join(
      "node_modules",
      "@anthropic-ai",
      "sandbox-runtime",
      "package.json",
    );
    const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf8")) as { version: string };
    expect(ANTHROPIC_SANDBOX_RUNTIME_VERSION).toBe(packageJson.version);
  });

  it("refuses process execution under the inspect profile before touching the backend", async () => {
    const workspace = await withTempWorkspace();
    const backend = createBackend(workspace);
    await expect(
      backend.execute({
        executable: "node",
        arguments: ["-e", "1"],
        workingDirectory: workspace,
        profile: INSPECT_PROFILE,
        environment: {},
      }),
    ).rejects.toThrow(SandboxError);
  });

  it("is idempotent across close calls", async () => {
    const workspace = await withTempWorkspace();
    const backend = createBackend(workspace);
    await backend.close();
    await expect(backend.close()).resolves.toBeUndefined();
  });
});

describe("fake sandbox backend", () => {
  it("returns deterministic scripted results in order", async () => {
    const workspace = await withTempWorkspace();
    const { backend, requests } = createFakeSandboxBackend({
      results: [
        completedResult({ stdout: "first" }),
        completedResult({ stdout: "second", status: "failed", exitCode: 1 }),
      ],
    });
    const first = await backend.execute({
      executable: "node",
      arguments: [],
      workingDirectory: workspace,
      profile: DEVELOP_OFFLINE_PROFILE,
      environment: {},
    });
    const second = await backend.execute({
      executable: "node",
      arguments: [],
      workingDirectory: workspace,
      profile: DEVELOP_OFFLINE_PROFILE,
      environment: {},
    });
    expect(first.stdout).toBe("first");
    expect(second).toMatchObject({ stdout: "second", status: "failed", exitCode: 1 });
    expect(requests()).toHaveLength(2);
  });

  it("simulates an outside-write violation result", async () => {
    const { backend } = createFakeSandboxBackend({
      results: [
        completedResult({
          status: "sandbox-denied",
          exitCode: 1,
          violations: [{ category: "filesystem", summary: "write denied outside workspace" }],
        }),
      ],
    });
    const result: SandboxedProcessResult = await backend.execute({
      executable: "node",
      arguments: [],
      workingDirectory: "/workspace",
      profile: DEVELOP_OFFLINE_PROFILE,
      environment: {},
    });
    expect(result.status).toBe("sandbox-denied");
    expect(result.violations[0]?.summary).toContain("outside workspace");
  });

  it("simulates a network violation result", async () => {
    const { backend } = createFakeSandboxBackend({
      results: [
        completedResult({
          status: "sandbox-denied",
          exitCode: 1,
          violations: [{ category: "network", summary: "outbound connection denied" }],
        }),
      ],
    });
    const result = await backend.execute({
      executable: "node",
      arguments: [],
      workingDirectory: "/workspace",
      profile: DEVELOP_OFFLINE_PROFILE,
      environment: {},
    });
    expect(result.violations[0]?.category).toBe("network");
  });

  it("rejects when configured with an execution error", async () => {
    const { backend } = createFakeSandboxBackend({
      executeError: new SandboxError("sandbox_execution_denied", "Denied."),
    });
    await expect(
      backend.execute({
        executable: "node",
        arguments: [],
        workingDirectory: "/workspace",
        profile: DEVELOP_OFFLINE_PROFILE,
        environment: {},
      }),
    ).rejects.toMatchObject({ code: "sandbox_execution_denied" });
  });

  it("reports inspect failures as rejections", async () => {
    const { backend } = createFakeSandboxBackend({
      inspectError: new Error("backend broken"),
    });
    await expect(backend.inspect()).rejects.toThrow("backend broken");
  });
});

describe("sandbox backend status contract", () => {
  it("represents an available status truthfully", () => {
    const status: SandboxBackendStatus = {
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
    expect(status.state).toBe("available");
  });

  it("represents a setup-required status with guidance", () => {
    const status: SandboxBackendStatus = {
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
    };
    expect(status.state).toBe("setup-required");
    expect(status.message).toContain("setup");
  });

  it("represents an unsupported status without capabilities", () => {
    const status: SandboxBackendStatus = {
      backendId: "fake-backend",
      state: "unsupported",
      platform: "unknown",
      version: "0.0.0-fake",
      capabilities: {
        filesystemReadRestriction: false,
        filesystemWriteRestriction: false,
        networkRestriction: false,
        processTreeRestriction: false,
        violationReporting: false,
      },
    };
    expect(status.state).toBe("unsupported");
    expect(Object.values(status.capabilities).every((value) => value === false)).toBe(true);
  });
});
