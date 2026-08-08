import { readFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  DEVELOP_OFFLINE_PROFILE,
  GODOT_PROBE_OFFLINE_PROFILE,
  GODOT_RECOVERY_PROBE_OFFLINE_PROFILE,
  INSPECT_PROFILE,
  VALIDATION_OFFLINE_PROFILE,
  SandboxError,
  type SandboxBackendStatus,
  type SandboxedProcessResult,
} from "@solaris/core";
import {
  ANTHROPIC_SANDBOX_RUNTIME_BACKEND_ID,
  ANTHROPIC_SANDBOX_RUNTIME_VERSION,
  assertHostReadBoundarySupported,
  buildPerExecutionConfig,
  createAnthropicSandboxRuntimeBackend,
  createOutputSink,
  emitChunked,
  effectiveConfigKey,
  hostReadAllowSurface,
  hostReadBoundaryPatterns,
  isWithinHostReadAllowSurface,
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

describe("host-read allowlist boundary", () => {
  it("denies the whole host root and re-allows only the approved surface on Linux and macOS", () => {
    expect(hostReadBoundaryPatterns("linux")).toEqual(["/"]);
    expect(hostReadBoundaryPatterns("macos")).toEqual(["/"]);
  });

  it("claims no partial deny surface on Windows", () => {
    expect(hostReadBoundaryPatterns("windows")).toEqual([]);
  });

  it("refuses process execution on Windows instead of claiming a boundary", () => {
    expect(() => assertHostReadBoundarySupported("linux")).not.toThrow();
    expect(() => assertHostReadBoundarySupported("macos")).not.toThrow();
    expect(() => assertHostReadBoundarySupported("windows")).toThrow(SandboxError);
    expect(() => assertHostReadBoundarySupported("windows")).toThrow("host-read allowlist");
  });

  it("exposes the trusted runner and system runtime surface", async () => {
    const surface = await hostReadAllowSurface();
    expect(surface.length).toBeGreaterThan(0);
    expect(surface.some((root) => root.includes("node"))).toBe(true);
    expect(surface.includes(process.execPath)).toBe(true);
  });

  it("classifies candidates against the allowlist surface", async () => {
    const surface = await hostReadAllowSurface();
    expect(isWithinHostReadAllowSurface(process.execPath, surface)).toBe(true);
    expect(isWithinHostReadAllowSurface(join(tmpdir(), "solaris-unapproved.txt"), surface)).toBe(
      false,
    );
  });
});

describe("effective profile configuration isolation", () => {
  const options = {
    workspaceRoot: "/workspace",
    sandboxHome: "/sandbox-home",
    sandboxTemp: "/sandbox-temp",
  };

  it("compares effective configuration, not only the profile id", () => {
    const developKey = effectiveConfigKey(options, DEVELOP_OFFLINE_PROFILE);
    const validationKey = effectiveConfigKey(options, {
      ...DEVELOP_OFFLINE_PROFILE,
      filesystem: { ...DEVELOP_OFFLINE_PROFILE.filesystem, workspaceAccess: "read-only" },
    });
    expect(developKey).not.toBe(validationKey);
    expect(effectiveConfigKey(options, DEVELOP_OFFLINE_PROFILE)).toBe(developKey);
  });

  it("never grants workspace writes to a read-only profile's execution config", async () => {
    const runDirectory = "/runs/fingerprint/run-1";
    const validationConfig = await buildPerExecutionConfig(
      options,
      {
        ...DEVELOP_OFFLINE_PROFILE,
        filesystem: { ...DEVELOP_OFFLINE_PROFILE.filesystem, workspaceAccess: "read-only" },
      },
      runDirectory,
    );
    const writeRoots = validationConfig.filesystem?.allowWrite ?? [];
    expect(writeRoots).not.toContain("/workspace");
    expect(writeRoots).toContain(runDirectory);
    expect(validationConfig.filesystem?.allowRead).toContain("/workspace");
    expect(validationConfig.filesystem?.denyRead).toEqual(hostReadBoundaryPatterns());
  });

  it("grants workspace writes only to a read-write profile's execution config", async () => {
    const runDirectory = "/runs/fingerprint/run-2";
    const developConfig = await buildPerExecutionConfig(
      options,
      DEVELOP_OFFLINE_PROFILE,
      runDirectory,
    );
    expect(developConfig.filesystem?.allowWrite).toContain("/workspace");
    expect(developConfig.filesystem?.allowWrite).toContain(runDirectory);
  });

  it("grants exactly the current run directory, never sibling runs", async () => {
    const runDirectory = "/runs/fingerprint/run-1";
    const config = await buildPerExecutionConfig(
      options,
      {
        ...DEVELOP_OFFLINE_PROFILE,
        filesystem: { ...DEVELOP_OFFLINE_PROFILE.filesystem, workspaceAccess: "read-only" },
      },
      runDirectory,
    );
    const readable = config.filesystem?.allowRead ?? [];
    expect(readable).toContain(runDirectory);
    expect(readable.some((root) => root.startsWith("/runs/fingerprint/run-2"))).toBe(false);
    expect(readable.some((root) => root === "/runs/fingerprint")).toBe(false);
    expect(readable.some((root) => root === "/runs")).toBe(false);
  });

  it("a request without a run directory never gains the shared runs root", async () => {
    const config = await buildPerExecutionConfig(
      options,
      {
        ...DEVELOP_OFFLINE_PROFILE,
        filesystem: { ...DEVELOP_OFFLINE_PROFILE.filesystem, workspaceAccess: "read-only" },
      },
      undefined,
    );
    const readable = config.filesystem?.allowRead ?? [];
    expect(readable.some((root) => root === "/runs" || root === "/runs/fingerprint")).toBe(false);
  });

  it("excludes the workspace from readable roots for recovery probes", async () => {
    const runDirectory = "/runs/fingerprint/run-1";
    const recoveryConfig = await buildPerExecutionConfig(
      options,
      GODOT_RECOVERY_PROBE_OFFLINE_PROFILE,
      runDirectory,
    );
    const readable = recoveryConfig.filesystem?.allowRead ?? [];
    expect(readable).not.toContain("/workspace");
    expect(readable).toContain(runDirectory);
    expect(readable).toContain("/sandbox-home");
    expect(readable).toContain("/sandbox-temp");
    const writable = recoveryConfig.filesystem?.allowWrite ?? [];
    expect(writable).not.toContain("/workspace");
    expect(writable).toContain(runDirectory);
  });

  it("keeps the workspace readable for every profile except recovery probes", async () => {
    for (const profile of [
      INSPECT_PROFILE,
      DEVELOP_OFFLINE_PROFILE,
      VALIDATION_OFFLINE_PROFILE,
      GODOT_PROBE_OFFLINE_PROFILE,
    ]) {
      const config = await buildPerExecutionConfig(options, profile, "/runs/fingerprint/run-1");
      expect(config.filesystem?.allowRead).toContain("/workspace");
    }
    expect(GODOT_RECOVERY_PROBE_OFFLINE_PROFILE.filesystem.excludeWorkspaceRead).toBe(true);
  });

  it("distinguishes the recovery profile in the effective configuration key", () => {
    const recoveryKey = effectiveConfigKey(options, GODOT_RECOVERY_PROBE_OFFLINE_PROFILE);
    expect(recoveryKey).not.toBe(effectiveConfigKey(options, GODOT_PROBE_OFFLINE_PROFILE));
    expect(effectiveConfigKey(options, GODOT_RECOVERY_PROBE_OFFLINE_PROFILE)).toBe(recoveryKey);
  });
});

describe("output sink hard limits", () => {
  it("accounts raw bytes and truncates inside the crossing chunk", () => {
    let reached = 0;
    const sink = createOutputSink(10, () => {
      reached += 1;
    });
    sink.push(Buffer.from("abcdef"));
    sink.push(Buffer.from("ghijklmnop"));
    expect(sink.text).toBe("abcdefghij");
    expect(sink.truncated).toBe(true);
    expect(reached).toBe(1);
    sink.push(Buffer.from("more"));
    expect(sink.text).toBe("abcdefghij");
  });

  it("a single large OS buffer cannot bypass the stream limit", () => {
    let reached = 0;
    const sink = createOutputSink(1000, () => {
      reached += 1;
    });
    sink.push(Buffer.alloc(10_000, 0x61));
    expect(sink.text.length).toBe(1000);
    expect(sink.truncated).toBe(true);
    expect(reached).toBe(1);
  });

  it("never splits a multibyte sequence at the truncation boundary", () => {
    const sink = createOutputSink(5, () => {});
    // "a" (1 byte) + \u00e9 (2 bytes) + \u00e9 (2 bytes) + "b" (1 byte):
    // the 5-byte window must keep exactly the complete sequences and drop
    // the dangling remainder without emitting a replacement character.
    sink.push(Buffer.from("a\u00e9\u00e9b", "utf8"));
    expect(sink.text).toBe("a\u00e9\u00e9");
    expect(sink.text).not.toContain("\uFFFD");
    expect(Buffer.byteLength(sink.text, "utf8")).toBe(5);
  });

  it("decodes sequences split across child-process buffers", () => {
    const sink = createOutputSink(100, () => {});
    const bytes = Buffer.from("h\u00e9llo", "utf8");
    sink.push(bytes.subarray(0, 2));
    sink.push(bytes.subarray(2));
    expect(sink.text).toBe("h\u00e9llo");
  });

  it("isolates a failing output callback without crashing", () => {
    const received: string[] = [];
    emitChunked(
      (event) => {
        received.push(event.text);
        if (event.text.length > 100) {
          throw new Error("callback exploded");
        }
      },
      "stdout",
      "y".repeat(200_000),
    );
    // Events were emitted before the failure and emission continued after it.
    expect(received.length).toBeGreaterThan(1);
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
