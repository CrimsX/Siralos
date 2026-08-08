import { readFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  DEVELOP_OFFLINE_PROFILE,
  GODOT_PROBE_OFFLINE_PROFILE,
  INSPECT_PROFILE,
  VALIDATION_OFFLINE_PROFILE,
  SandboxError,
  type SandboxBackendStatus,
  type SandboxedProcessRequest,
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
  type AnthropicSandboxRuntimeBackendHooks,
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

function createBackend(workspaceRoot: string, hooks?: AnthropicSandboxRuntimeBackendHooks) {
  return createAnthropicSandboxRuntimeBackend({
    workspaceRoot,
    ...(hooks === undefined ? {} : { hooks }),
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

  it("excludes the workspace from readable roots for engine probe profiles", async () => {
    const runDirectory = "/runs/fingerprint/run-1";
    const probeConfig = await buildPerExecutionConfig(
      options,
      GODOT_PROBE_OFFLINE_PROFILE,
      runDirectory,
    );
    const readable = probeConfig.filesystem?.allowRead ?? [];
    expect(readable).not.toContain("/workspace");
    expect(readable).toContain(runDirectory);
    const writable = probeConfig.filesystem?.allowWrite ?? [];
    expect(writable).not.toContain("/workspace");
    expect(writable).toContain(runDirectory);
  });

  it("grants the run directory and never the shared sandbox directories", async () => {
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
    const writable = config.filesystem?.allowWrite ?? [];
    for (const roots of [readable, writable]) {
      expect(roots).toContain(runDirectory);
      expect(roots.some((root) => root.includes("sandbox-home"))).toBe(false);
      expect(roots.some((root) => root.includes("sandbox-temp"))).toBe(false);
    }
  });

  it("grants each run exactly its own run directory across executions", async () => {
    const first = await buildPerExecutionConfig(
      options,
      {
        ...DEVELOP_OFFLINE_PROFILE,
        filesystem: { ...DEVELOP_OFFLINE_PROFILE.filesystem, workspaceAccess: "read-only" },
      },
      "/runs/fingerprint/run-1",
    );
    const second = await buildPerExecutionConfig(
      options,
      {
        ...DEVELOP_OFFLINE_PROFILE,
        filesystem: { ...DEVELOP_OFFLINE_PROFILE.filesystem, workspaceAccess: "read-only" },
      },
      "/runs/fingerprint/run-2",
    );
    const firstReadable = first.filesystem?.allowRead ?? [];
    const firstWritable = first.filesystem?.allowWrite ?? [];
    const secondReadable = second.filesystem?.allowRead ?? [];
    const secondWritable = second.filesystem?.allowWrite ?? [];
    for (const roots of [firstReadable, firstWritable]) {
      expect(roots).toContain("/runs/fingerprint/run-1");
      expect(roots.some((root) => root === "/runs/fingerprint/run-2")).toBe(false);
      expect(roots.some((root) => root === "/runs/fingerprint")).toBe(false);
      expect(roots.some((root) => root === "/runs")).toBe(false);
    }
    for (const roots of [secondReadable, secondWritable]) {
      expect(roots).toContain("/runs/fingerprint/run-2");
      expect(roots.some((root) => root === "/runs/fingerprint/run-1")).toBe(false);
      expect(roots.some((root) => root === "/runs/fingerprint")).toBe(false);
      expect(roots.some((root) => root === "/runs")).toBe(false);
    }
  });

  it("replaces the workspace and trusted-runner surface with explicit read roots", async () => {
    const runDirectory = "/runs/fingerprint/run-1";
    const explicitRoots = ["/opt/godot-engine"];
    const config = await buildPerExecutionConfig(
      options,
      DEVELOP_OFFLINE_PROFILE,
      runDirectory,
      explicitRoots,
    );
    const readable = config.filesystem?.allowRead ?? [];
    expect(readable).toContain(runDirectory);
    expect(readable).toContain("/opt/godot-engine");
    expect(readable).not.toContain("/workspace");
    expect(readable).not.toContain(process.execPath);
    expect(readable).not.toContain(dirname(process.execPath));
  });

  it("keeps the profile workspace and trusted-runner surface without explicit read roots", async () => {
    const runDirectory = "/runs/fingerprint/run-1";
    const config = await buildPerExecutionConfig(options, DEVELOP_OFFLINE_PROFILE, runDirectory);
    const readable = config.filesystem?.allowRead ?? [];
    expect(readable).toContain(runDirectory);
    expect(readable).toContain("/workspace");
    expect(readable).toContain(process.execPath);
  });

  it("keeps the workspace readable for every profile except the probe profiles", async () => {
    for (const profile of [INSPECT_PROFILE, DEVELOP_OFFLINE_PROFILE, VALIDATION_OFFLINE_PROFILE]) {
      const config = await buildPerExecutionConfig(options, profile, "/runs/fingerprint/run-1");
      expect(config.filesystem?.allowRead).toContain("/workspace");
    }
    // The Godot probe profile excludes the workspace from readable roots:
    // the probed engine must not read the real project at all.
    expect(GODOT_PROBE_OFFLINE_PROFILE.filesystem.excludeWorkspaceRead).toBe(true);
    const config = await buildPerExecutionConfig(
      options,
      GODOT_PROBE_OFFLINE_PROFILE,
      "/runs/fingerprint/run-1",
    );
    expect(config.filesystem?.allowRead).not.toContain("/workspace");
  });

  it("collapses profiles with identical effective configuration into the same key", () => {
    expect(effectiveConfigKey(options, DEVELOP_OFFLINE_PROFILE)).toBe(
      effectiveConfigKey(options, DEVELOP_OFFLINE_PROFILE),
    );
    expect(effectiveConfigKey(options, DEVELOP_OFFLINE_PROFILE)).not.toBe(
      effectiveConfigKey(options, {
        ...DEVELOP_OFFLINE_PROFILE,
        filesystem: { ...DEVELOP_OFFLINE_PROFILE.filesystem, workspaceAccess: "read-only" },
      }),
    );
  });
});

describe("sandbox lifecycle serialization", () => {
  /**
   * Test seam backend: the onExecuteStarted hook is the stand-in for the
   * SandboxManager interaction point. It records entry order, blocks the
   * first request on a barrier, and always throws so no test ever reaches
   * the real SandboxManager; ordering is therefore deterministic without
   * sleeps.
   */
  function createSeamBackend(workspace: string) {
    let releaseFirst: (() => void) | undefined;
    const gate = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    const started: string[] = [];
    const backend = createBackend(workspace, {
      onExecuteStarted: async (request) => {
        started.push(request.executable);
        if (request.executable === "blocked") {
          await gate;
        }
        throw new Error("seam barrier");
      },
    });
    return {
      backend,
      started: () => started,
      releaseFirst: () => releaseFirst?.(),
      request: (
        executable: string,
        extra: Partial<SandboxedProcessRequest> = {},
      ): SandboxedProcessRequest => ({
        executable,
        arguments: [],
        workingDirectory: workspace,
        profile: DEVELOP_OFFLINE_PROFILE,
        environment: {},
        ...extra,
      }),
    };
  }

  it("serializes concurrent executions so the second cannot start until the first finishes", async () => {
    const workspace = await withTempWorkspace();
    const seam = createSeamBackend(workspace);
    const first = seam.backend.execute(seam.request("blocked"));
    await waitFor(() => seam.started().includes("blocked"));
    const second = seam.backend.execute(seam.request("second"));
    // The second request is queued; its serialized body must not have started.
    expect(seam.started()).toEqual(["blocked"]);
    seam.releaseFirst();
    await expect(first).rejects.toThrow("seam barrier");
    await expect(second).rejects.toThrow("seam barrier");
    expect(seam.started()).toEqual(["blocked", "second"]);
  });

  it("queues a profile-change reset behind the active execution", async () => {
    const workspace = await withTempWorkspace();
    const seam = createSeamBackend(workspace);
    const first = seam.backend.execute(seam.request("blocked"));
    await waitFor(() => seam.started().includes("blocked"));
    // A different effective configuration would force reset-before-reinit;
    // it must be queued, never interleaved with the active request.
    const second = seam.backend.execute(
      seam.request("reconfigure", {
        profile: {
          ...DEVELOP_OFFLINE_PROFILE,
          filesystem: { ...DEVELOP_OFFLINE_PROFILE.filesystem, workspaceAccess: "read-only" },
        },
      }),
    );
    expect(seam.started()).toEqual(["blocked"]);
    seam.releaseFirst();
    await expect(first).rejects.toThrow("seam barrier");
    await expect(second).rejects.toThrow("seam barrier");
    expect(seam.started()).toEqual(["blocked", "reconfigure"]);
    expect(effectiveConfigKey({ workspaceRoot: workspace }, DEVELOP_OFFLINE_PROFILE)).not.toBe(
      effectiveConfigKey(
        { workspaceRoot: workspace },
        {
          ...DEVELOP_OFFLINE_PROFILE,
          filesystem: { ...DEVELOP_OFFLINE_PROFILE.filesystem, workspaceAccess: "read-only" },
        },
      ),
    );
  });

  it("returns a cancelled result promptly when a queued request is aborted while the active request hangs", async () => {
    const workspace = await withTempWorkspace();
    const seam = createSeamBackend(workspace);
    const first = seam.backend.execute(seam.request("blocked"));
    await waitFor(() => seam.started().includes("blocked"));
    const controller = new AbortController();
    const queued = seam.backend.execute(seam.request("queued", { signal: controller.signal }));
    controller.abort();
    // The queued request must settle without waiting for the hung active
    // request: wait a bounded window while the first is still blocked.
    const result = await Promise.race([
      queued,
      new Promise<never>((_, reject) => {
        setTimeout(() => reject(new Error("queued cancellation was not prompt")), 2000);
      }),
    ]);
    expect(result.status).toBe("cancelled");
    // The active request is still hung and has not been disturbed.
    expect(seam.started()).toEqual(["blocked"]);
    seam.releaseFirst();
    await expect(first).rejects.toThrow("seam barrier");
    // The cancelled slot skipped its task: nothing else ever started.
    expect(seam.started()).toEqual(["blocked"]);
  });

  it("returns a timed-out result promptly when a queued request expires while the active request hangs", async () => {
    const workspace = await withTempWorkspace();
    const seam = createSeamBackend(workspace);
    const first = seam.backend.execute(seam.request("blocked"));
    await waitFor(() => seam.started().includes("blocked"));
    // A tiny timeout expires while the first request is still hung; the
    // queued request must settle promptly without starting anything.
    const queued = seam.backend.execute(seam.request("queued", { timeoutMs: 50 }));
    const result = await Promise.race([
      queued,
      new Promise<never>((_, reject) => {
        setTimeout(() => reject(new Error("queued expiry was not prompt")), 2000);
      }),
    ]);
    expect(result.status).toBe("timed-out");
    expect(seam.started()).toEqual(["blocked"]);
    seam.releaseFirst();
    await expect(first).rejects.toThrow("seam barrier");
    expect(seam.started()).toEqual(["blocked"]);
  });

  it("close() waits for the active execution before resetting and rejects later executions", async () => {
    const workspace = await withTempWorkspace();
    const seam = createSeamBackend(workspace);
    const first = seam.backend.execute(seam.request("blocked"));
    await waitFor(() => seam.started().includes("blocked"));
    let closingSettled = false;
    const closing = seam.backend.close().then(() => {
      closingSettled = true;
    });
    // Execute after close rejects immediately, even while draining.
    await expect(seam.backend.execute(seam.request("late"))).rejects.toThrow(SandboxError);
    await expect(seam.backend.execute(seam.request("late"))).rejects.toThrow("closed");
    // The reset task is queued behind the active execution: synchronously
    // after releasing the barrier it still cannot have run.
    seam.releaseFirst();
    expect(closingSettled).toBe(false);
    await expect(first).rejects.toThrow("seam barrier");
    await closing;
    expect(closingSettled).toBe(true);
    await expect(seam.backend.execute(seam.request("after"))).rejects.toThrow(SandboxError);
  });
});

describe("Windows status truthfulness", () => {
  it.skipIf(process.platform !== "win32")(
    "never reports available and never claims host-read enforcement",
    async () => {
      const workspace = await withTempWorkspace();
      const backend = createBackend(workspace);
      const status = await backend.inspect();
      expect(status.state).not.toBe("available");
      expect(status.capabilities.filesystemReadRestriction).toBe(false);
      if (status.state === "degraded") {
        expect(status.message).toContain("host-read");
      }
    },
  );
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
