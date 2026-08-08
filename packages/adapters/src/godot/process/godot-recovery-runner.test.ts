import { describe, expect, it } from "vitest";
import { mkdtemp, writeFile, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { createHash } from "node:crypto";
import {
  GODOT_LIMITS,
  GODOT_RECOVERY_PROBE_OFFLINE_PROFILE,
  createEmptyGodotCommandCapabilities,
} from "@solaris/core";
import type { GodotEngineProfile, GodotInstallation, SandboxBackendStatus } from "@solaris/core";
import { completedResult, createFakeSandboxBackend } from "../../sandbox/fake-sandbox-backend.js";
import {
  computeGodotRecoveryCommandDigest,
  createGodotRecoveryRunner,
  godotRecoveryArguments,
  godotRecoveryArgumentTemplate,
} from "./godot-recovery-runner.js";

const MIRROR_PATH = "C:/solaris-runs/fingerprint/run-1/project";

let realExecutable: {
  readonly path: string;
  readonly size: number;
  readonly mtimeMs: number;
} | null = null;

async function executablePath(): Promise<string> {
  if (realExecutable !== null) {
    return realExecutable.path;
  }
  const directory = await mkdtemp(path.join(tmpdir(), "solaris-recovery-runner-"));
  const executable = path.join(directory, "godot-test.exe");
  const content = "#!/bin/sh\nexit 0\n";
  await writeFile(executable, content);
  const metadata = await stat(executable);
  realExecutable = { path: executable, size: metadata.size, mtimeMs: metadata.mtimeMs };
  return executable;
}

async function validInstallation(): Promise<GodotInstallation> {
  const executable = await executablePath();
  return {
    id: "path-1",
    sourceLabel: "user config",
    source: "user-config",
    canonicalPath: executable,
    sizeBytes: executable === null ? 0 : (await stat(executable)).size,
    modifiedAtMs: executable === null ? 0 : (await stat(executable)).mtimeMs,
    sha256: createHash("sha256").update("#!/bin/sh\nexit 0\n", "utf8").digest("hex"),
    editionHint: "standard",
    status: "valid",
  };
}

function engineProfile(
  overrides: Partial<Omit<GodotEngineProfile, "capabilities">> & {
    readonly capabilities?: Partial<ReturnType<typeof createEmptyGodotCommandCapabilities>>;
  } = {},
): GodotEngineProfile {
  const capabilities = createEmptyGodotCommandCapabilities();
  (capabilities as unknown as Record<string, boolean>)["editor"] = true;
  (capabilities as unknown as Record<string, boolean>)["headless"] = true;
  (capabilities as unknown as Record<string, boolean>)["recoveryMode"] = true;
  (capabilities as unknown as Record<string, boolean>)["projectPath"] = true;
  (capabilities as unknown as Record<string, boolean>)["quit"] = true;
  (capabilities as unknown as Record<string, boolean>)["quitAfter"] = true;
  for (const [name, value] of Object.entries(overrides.capabilities ?? {})) {
    (capabilities as unknown as Record<string, boolean>)[name] = Boolean(value);
  }
  const { capabilities: _capabilityOverrides, ...profileOverrides } = overrides;
  return {
    installationId: "path-1",
    fingerprint: "abc123",
    version: {
      raw: "4.7.1.stable.official",
      major: 4,
      minor: 7,
      patch: 1,
      status: "stable",
      statusNumber: 0,
      build: null,
      commit: null,
    },
    edition: "standard",
    editionConfidence: "high",
    releaseChannel: "stable",
    capabilities,
    verifiedCapabilities: ["version", "help"],
    degradedCapabilities: [],
    executableSha256: "a".repeat(64),
    apiDumpSha256: null,
    support: "verified",
    diagnostics: [],
    ...profileOverrides,
  };
}

function runPaths() {
  return {
    root: "C:/solaris-runs/fingerprint/run-1",
    home: "C:/solaris-runs/fingerprint/run-1/home",
    temp: "C:/solaris-runs/fingerprint/run-1/tmp",
  };
}

function availableStatus(): SandboxBackendStatus {
  return {
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
}

describe("recovery-mode command shape", () => {
  it("passes --headless, --editor, and --recovery-mode", async () => {
    const fake = createFakeSandboxBackend({
      status: availableStatus(),
      results: [completedResult()],
    });
    const runner = createGodotRecoveryRunner({
      backend: fake.backend,
      parentEnvironment: { PATH: "/usr/bin" },
    });
    const outcome = await runner.run({
      installation: await validInstallation(),
      engineProfile: engineProfile(),
      mirrorProjectPath: MIRROR_PATH,
      runPaths: runPaths(),
    });
    expect(outcome.status).toBe("completed");
    const request = fake.requests()[0];
    expect(request?.arguments).toContain("--headless");
    expect(request?.arguments).toContain("--editor");
    expect(request?.arguments).toContain("--recovery-mode");
  });

  it("points --path only at the disposable mirror", async () => {
    const fake = createFakeSandboxBackend({
      status: availableStatus(),
      results: [completedResult()],
    });
    const runner = createGodotRecoveryRunner({ backend: fake.backend, parentEnvironment: {} });
    const outcome = await runner.run({
      installation: await validInstallation(),
      engineProfile: engineProfile(),
      mirrorProjectPath: MIRROR_PATH,
      runPaths: runPaths(),
    });
    expect(outcome.status).toBe("completed");
    const request = fake.requests()[0];
    const pathIndex = request?.arguments.indexOf("--path") ?? -1;
    expect(pathIndex).toBeGreaterThanOrEqual(0);
    expect(request?.arguments[pathIndex + 1]).toBe(MIRROR_PATH);
    expect(request?.workingDirectory).toBe(MIRROR_PATH);
  });

  it("never carries the source workspace path anywhere", async () => {
    const fake = createFakeSandboxBackend({
      status: availableStatus(),
      results: [completedResult()],
    });
    const runner = createGodotRecoveryRunner({
      backend: fake.backend,
      parentEnvironment: {
        PATH: "/usr/bin",
        HOME: "C:/Users/developer/SolarisSourceWorkspace",
        USERPROFILE: "C:/Users/developer/SolarisSourceWorkspace",
      },
    });
    await runner.run({
      installation: await validInstallation(),
      engineProfile: engineProfile(),
      mirrorProjectPath: MIRROR_PATH,
      runPaths: runPaths(),
    });
    const request = fake.requests()[0];
    const sourcePath = "SolarisSourceWorkspace";
    for (const argument of request?.arguments ?? []) {
      expect(argument).not.toContain(sourcePath);
    }
    expect(request?.workingDirectory).not.toContain(sourcePath);
    for (const value of Object.values(request?.environment ?? {})) {
      expect(value).not.toContain(sourcePath);
    }
  });

  it("binds a bounded --quit-after iteration count", () => {
    const arguments_ = godotRecoveryArguments(MIRROR_PATH);
    const quitIndex = arguments_.indexOf("--quit-after");
    expect(quitIndex).toBeGreaterThanOrEqual(0);
    expect(Number(arguments_[quitIndex + 1])).toBe(GODOT_LIMITS.recoveryQuitAfterIterations);
  });

  it("never passes --upwards, --scene, --script, or --import", () => {
    const arguments_ = godotRecoveryArguments(MIRROR_PATH);
    for (const forbidden of ["--upwards", "--scene", "--script", "--import"]) {
      expect(arguments_).not.toContain(forbidden);
    }
  });

  it("never passes export, LSP, DAP, debug-server, movie, or benchmark options", () => {
    const arguments_ = godotRecoveryArguments(MIRROR_PATH);
    for (const forbidden of [
      "--export",
      "--export-pack",
      "--build-solutions",
      "--lsp",
      "--lsp-port",
      "--dap",
      "--debug-server",
      "--write-movie",
      "--benchmark",
      "--doctool",
    ]) {
      expect(arguments_).not.toContain(forbidden);
    }
  });

  it("accepts no user arguments after the fixed set", () => {
    const arguments_ = godotRecoveryArguments(MIRROR_PATH);
    expect(arguments_).toEqual([
      "--headless",
      "--editor",
      "--recovery-mode",
      "--path",
      MIRROR_PATH,
      "--quit-after",
      String(GODOT_LIMITS.recoveryQuitAfterIterations),
    ]);
  });

  it("runs without a shell (separate executable and argument array)", async () => {
    const fake = createFakeSandboxBackend({
      status: availableStatus(),
      results: [completedResult()],
    });
    const runner = createGodotRecoveryRunner({ backend: fake.backend, parentEnvironment: {} });
    await runner.run({
      installation: await validInstallation(),
      engineProfile: engineProfile(),
      mirrorProjectPath: MIRROR_PATH,
      runPaths: runPaths(),
    });
    const request = fake.requests()[0];
    expect(typeof request?.executable).toBe("string");
    expect(Array.isArray(request?.arguments)).toBe(true);
  });

  it("requires a fully enforcing sandbox backend", async () => {
    const fake = createFakeSandboxBackend({
      status: { ...availableStatus(), state: "setup-required" },
    });
    const runner = createGodotRecoveryRunner({ backend: fake.backend, parentEnvironment: {} });
    const outcome = await runner.run({
      installation: await validInstallation(),
      engineProfile: engineProfile(),
      mirrorProjectPath: MIRROR_PATH,
      runPaths: runPaths(),
    });
    expect(outcome.status).toBe("sandbox-unavailable");
    expect(fake.requests().length).toBe(0);
  });

  it("runs under the recovery profile with network denied and minimal environment", async () => {
    const fake = createFakeSandboxBackend({
      status: availableStatus(),
      results: [completedResult()],
    });
    const runner = createGodotRecoveryRunner({ backend: fake.backend, parentEnvironment: {} });
    await runner.run({
      installation: await validInstallation(),
      engineProfile: engineProfile(),
      mirrorProjectPath: MIRROR_PATH,
      runPaths: runPaths(),
    });
    const request = fake.requests()[0];
    expect(request?.profile.id).toBe(GODOT_RECOVERY_PROBE_OFFLINE_PROFILE.id);
    expect(request?.profile.network.outbound).toBe("deny");
    expect(request?.profile.environment.policy).toBe("minimal");
    expect(request?.runDirectory).toBe(runPaths().root);
  });

  it("strips provider credentials and injection-capable variables from the environment", async () => {
    const fake = createFakeSandboxBackend({
      status: availableStatus(),
      results: [completedResult()],
    });
    const runner = createGodotRecoveryRunner({
      backend: fake.backend,
      parentEnvironment: {
        OPENROUTER_API_KEY: "sk-fake",
        DEEPSEEK_API_KEY: "sk-fake",
        GITHUB_TOKEN: "gh-fake",
        AWS_SECRET_ACCESS_KEY: "aws-fake",
        GODOT_EDITOR_PATH: "/evil/godot",
        LD_PRELOAD: "/lib/evil.so",
        DYLD_INSERT_LIBRARIES: "/lib/evil.dylib",
        PATH: "/usr/bin",
      },
    });
    await runner.run({
      installation: await validInstallation(),
      engineProfile: engineProfile(),
      mirrorProjectPath: MIRROR_PATH,
      runPaths: runPaths(),
    });
    const request = fake.requests()[0];
    const environment = request?.environment ?? {};
    for (const [name, value] of Object.entries(environment)) {
      expect(name).not.toMatch(/_API_KEY$|_TOKEN$|_SECRET$|_PASSWORD$/i);
      expect(value).not.toMatch(/sk-|gh-fake|aws-fake/i);
    }
    expect(environment["GODOT_EDITOR_PATH"]).toBeUndefined();
    expect(environment["LD_PRELOAD"]).toBeUndefined();
    expect(environment["DYLD_INSERT_LIBRARIES"]).toBeUndefined();
    expect(environment["PATH"]).toBe("/usr/bin");
  });

  it("enforces its timeout on the sandboxed request", async () => {
    const fake = createFakeSandboxBackend({
      status: availableStatus(),
      results: [completedResult()],
    });
    const runner = createGodotRecoveryRunner({
      backend: fake.backend,
      parentEnvironment: {},
      timeoutMs: 42_000,
    });
    await runner.run({
      installation: await validInstallation(),
      engineProfile: engineProfile(),
      mirrorProjectPath: MIRROR_PATH,
      runPaths: runPaths(),
    });
    const request = fake.requests()[0];
    expect(request?.timeoutMs).toBe(42_000);
  });

  it("honours pre-aborted cancellation without starting the engine", async () => {
    const fake = createFakeSandboxBackend({ status: availableStatus() });
    const runner = createGodotRecoveryRunner({ backend: fake.backend, parentEnvironment: {} });
    const controller = new AbortController();
    controller.abort();
    await expect(
      runner.run({
        installation: await validInstallation(),
        engineProfile: engineProfile(),
        mirrorProjectPath: MIRROR_PATH,
        runPaths: runPaths(),
        signal: controller.signal,
      }),
    ).rejects.toThrow(/aborted/i);
    expect(fake.requests().length).toBe(0);
  });

  it("maps a cancelled sandbox result to cancellation", async () => {
    const fake = createFakeSandboxBackend({
      status: availableStatus(),
      results: [{ ...completedResult(), status: "cancelled" }],
    });
    const runner = createGodotRecoveryRunner({ backend: fake.backend, parentEnvironment: {} });
    const outcome = await runner.run({
      installation: await validInstallation(),
      engineProfile: engineProfile(),
      mirrorProjectPath: MIRROR_PATH,
      runPaths: runPaths(),
    });
    expect(outcome.status).toBe("cancelled");
  });

  it("maps a timeout to a timeout, never a completion", async () => {
    const fake = createFakeSandboxBackend({
      status: availableStatus(),
      results: [{ ...completedResult(), status: "timed-out" }],
    });
    const runner = createGodotRecoveryRunner({ backend: fake.backend, parentEnvironment: {} });
    const outcome = await runner.run({
      installation: await validInstallation(),
      engineProfile: engineProfile(),
      mirrorProjectPath: MIRROR_PATH,
      runPaths: runPaths(),
    });
    expect(outcome.status).toBe("timed-out");
  });

  it("rejects an invalidated installation before launching", async () => {
    const fake = createFakeSandboxBackend({ status: availableStatus() });
    const runner = createGodotRecoveryRunner({ backend: fake.backend, parentEnvironment: {} });
    const outcome = await runner.run({
      installation: { ...(await validInstallation()), status: "invalid", error: "stale" },
      engineProfile: engineProfile(),
      mirrorProjectPath: MIRROR_PATH,
      runPaths: runPaths(),
    });
    expect(outcome.status).toBe("unsupported");
    expect(fake.requests().length).toBe(0);
  });
});

describe("recovery capability requirements", () => {
  async function runWithCapabilities(
    capabilities: Partial<ReturnType<typeof createEmptyGodotCommandCapabilities>>,
    edition: GodotEngineProfile["edition"] = "standard",
  ) {
    const fake = createFakeSandboxBackend({
      status: availableStatus(),
      results: [completedResult()],
    });
    const runner = createGodotRecoveryRunner({ backend: fake.backend, parentEnvironment: {} });
    const outcome = await runner.run({
      installation: await validInstallation(),
      engineProfile: engineProfile({ edition, capabilities }),
      mirrorProjectPath: MIRROR_PATH,
      runPaths: runPaths(),
    });
    return { outcome, executeCount: () => fake.requests().length };
  }

  it("rejects an engine missing --recovery-mode", async () => {
    const { outcome, executeCount } = await runWithCapabilities({ recoveryMode: false });
    expect(outcome.status).toBe("unsupported");
    if (outcome.status === "unsupported") {
      expect(outcome.message).toContain("recovery-mode");
    }
    expect(executeCount()).toBe(0);
  });

  it("rejects an engine missing --editor", async () => {
    const { outcome, executeCount } = await runWithCapabilities({ editor: false });
    expect(outcome.status).toBe("unsupported");
    expect(executeCount()).toBe(0);
  });

  it("rejects an engine missing --headless", async () => {
    const { outcome, executeCount } = await runWithCapabilities({ headless: false });
    expect(outcome.status).toBe("unsupported");
    expect(executeCount()).toBe(0);
  });

  it("rejects an engine missing --path", async () => {
    const { outcome, executeCount } = await runWithCapabilities({ projectPath: false });
    expect(outcome.status).toBe("unsupported");
    expect(executeCount()).toBe(0);
  });

  it("rejects a runtime-only executable", async () => {
    const { outcome, executeCount } = await runWithCapabilities({}, "runtime-only");
    expect(outcome.status).toBe("unsupported");
    if (outcome.status === "unsupported") {
      expect(outcome.message).toContain("runtime-only");
    }
    expect(executeCount()).toBe(0);
  });

  it("keeps Godot 3 unsupported (no recovery mode, never launched weaker)", async () => {
    const { outcome, executeCount } = await runWithCapabilities({
      editor: true,
      headless: true,
      projectPath: true,
      recoveryMode: false,
    });
    expect(outcome.status).toBe("unsupported");
    expect(executeCount()).toBe(0);
  });

  it("never launches an unsupported engine in a weaker mode", async () => {
    const cases: Partial<ReturnType<typeof createEmptyGodotCommandCapabilities>>[] = [
      { recoveryMode: false },
      { editor: false },
      { headless: false },
      { projectPath: false },
    ];
    for (const capabilities of cases) {
      const { outcome, executeCount } = await runWithCapabilities(capabilities);
      expect(outcome.status).toBe("unsupported");
      expect(executeCount()).toBe(0);
    }
  });
});

describe("recovery command digest", () => {
  it("is deterministic over the fixed template", () => {
    const parts = {
      executableSha256: "a".repeat(64),
      argumentTemplate: godotRecoveryArgumentTemplate(),
      workingDirectoryPolicy: "disposable-mirror" as const,
      profileId: GODOT_RECOVERY_PROBE_OFFLINE_PROFILE.id,
      environmentPolicy: "minimal" as const,
      stdinPolicy: "closed" as const,
      networkPolicy: "denied" as const,
      timeoutMs: GODOT_LIMITS.recoveryProbeTimeoutMs,
      stdoutLimitBytes: GODOT_LIMITS.maxRecoveryStreamBytes,
      stderrLimitBytes: GODOT_LIMITS.maxRecoveryStreamBytes,
    };
    expect(computeGodotRecoveryCommandDigest(parts)).toMatch(/^[0-9a-f]{64}$/);
    expect(computeGodotRecoveryCommandDigest(parts)).toBe(computeGodotRecoveryCommandDigest(parts));
  });

  it("is independent of the actual mirror path (canonicalized to the marker)", async () => {
    const { createHash } = await import("node:crypto");
    const sortDeep = (value: unknown): unknown => {
      if (Array.isArray(value)) {
        return value.map(sortDeep);
      }
      if (value !== null && typeof value === "object") {
        const record = value as Record<string, unknown>;
        const sorted: Record<string, unknown> = {};
        for (const key of Object.keys(record).sort()) {
          sorted[key] = sortDeep(record[key]);
        }
        return sorted;
      }
      return value;
    };
    const parts = {
      executableSha256: "a".repeat(64),
      argumentTemplate: godotRecoveryArgumentTemplate(),
      workingDirectoryPolicy: "disposable-mirror" as const,
      profileId: "godot-recovery-probe-offline",
      environmentPolicy: "minimal" as const,
      stdinPolicy: "closed" as const,
      networkPolicy: "denied" as const,
      timeoutMs: 60_000,
      stdoutLimitBytes: 1024 * 1024,
      stderrLimitBytes: 1024 * 1024,
    };
    const reference = createHash("sha256")
      .update(JSON.stringify(sortDeep(parts)), "utf8")
      .digest("hex");
    expect(computeGodotRecoveryCommandDigest(parts)).toBe(reference);
  });

  it("changes when the executable, profile, or limits change", () => {
    const base = {
      executableSha256: "a".repeat(64),
      argumentTemplate: godotRecoveryArgumentTemplate(),
      workingDirectoryPolicy: "disposable-mirror" as const,
      profileId: GODOT_RECOVERY_PROBE_OFFLINE_PROFILE.id,
      environmentPolicy: "minimal" as const,
      stdinPolicy: "closed" as const,
      networkPolicy: "denied" as const,
      timeoutMs: GODOT_LIMITS.recoveryProbeTimeoutMs,
      stdoutLimitBytes: GODOT_LIMITS.maxRecoveryStreamBytes,
      stderrLimitBytes: GODOT_LIMITS.maxRecoveryStreamBytes,
    };
    const first = computeGodotRecoveryCommandDigest(base);
    expect(
      computeGodotRecoveryCommandDigest({ ...base, executableSha256: "b".repeat(64) }),
    ).not.toBe(first);
    expect(
      computeGodotRecoveryCommandDigest({ ...base, profileId: "godot-probe-offline" }),
    ).not.toBe(first);
    expect(computeGodotRecoveryCommandDigest({ ...base, timeoutMs: 30_000 })).not.toBe(first);
  });
});
