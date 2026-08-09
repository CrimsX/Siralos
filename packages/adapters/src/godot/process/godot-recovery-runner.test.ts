import { describe, expect, it, vi } from "vitest";
import {
  createGodotRecoveryRunner,
  GODOT_RECOVERY_BASE_ARGUMENTS,
  GODOT_RECOVERY_MIRROR_PATH_MARKER,
  GODOT_RECOVERY_RUN_UNAVAILABLE_MESSAGE,
  godotRecoveryArgumentTemplate,
  godotRecoveryArguments,
  computeGodotRecoveryCommandDigest,
  type GodotRecoveryRunRequest,
} from "./godot-recovery-runner.js";
import type { GodotEngineProfile, GodotInstallation } from "@solaris/core";
import { createEmptyGodotCommandCapabilities } from "@solaris/core";

function installation(): GodotInstallation {
  return {
    id: "path-1",
    sourceLabel: "explicit path",
    source: "cli-path",
    canonicalPath: "C:\\godot\\Godot.exe",
    sizeBytes: 1000,
    modifiedAtMs: 1000,
    sha256: "a".repeat(64),
    editionHint: "unknown",
    status: "valid",
  };
}

function engineProfile(): GodotEngineProfile {
  const capabilities = createEmptyGodotCommandCapabilities();
  return {
    installationId: "path-1",
    version: {
      major: 4,
      minor: 7,
      patch: 1,
      status: "stable",
      statusNumber: null,
      build: "official",
      commit: null,
      raw: "4.7.1.stable.official",
    },
    edition: "standard",
    editionConfidence: "high",
    releaseChannel: "stable",
    capabilities: {
      ...capabilities,
      editor: true,
      headless: true,
      recoveryMode: true,
      projectPath: true,
    },
    fingerprint: "b".repeat(64),
    verifiedCapabilities: [],
    degradedCapabilities: [],
    executableSha256: "a".repeat(64),
    apiDumpSha256: null,
    support: "verified",
    diagnostics: [],
  };
}

function request(): GodotRecoveryRunRequest {
  return {
    installation: installation(),
    engineProfile: engineProfile(),
    mirrorProjectPath: "C:\\solaris\\runs\\run-1\\project",
    runPaths: {
      root: "C:\\solaris\\runs\\run-1",
      home: "C:\\solaris\\runs\\run-1\\home",
      temp: "C:\\solaris\\runs\\run-1\\tmp",
    },
  };
}

describe("fixed recovery invocation", () => {
  it("is a fixed headless recovery-mode editor tuple", () => {
    expect(GODOT_RECOVERY_BASE_ARGUMENTS).toEqual(["--headless", "--editor", "--recovery-mode"]);
    expect(godotRecoveryArguments("C:\\mirror\\project")).toEqual([
      "--headless",
      "--editor",
      "--recovery-mode",
      "--path",
      "C:\\mirror\\project",
      "--quit-after",
      "120",
    ]);
  });

  it("canonicalizes the mirror path to the marker in the template", () => {
    expect(godotRecoveryArgumentTemplate()).toEqual([
      "--headless",
      "--editor",
      "--recovery-mode",
      "--path",
      GODOT_RECOVERY_MIRROR_PATH_MARKER,
      "--quit-after",
      "120",
    ]);
  });

  it("never carries project execution, import, export, or debug options", () => {
    for (const argument of godotRecoveryArgumentTemplate()) {
      expect(
        ["--script", "--scene", "--import", "--upwards", "--export", "--lsp", "--dap"].includes(
          argument,
        ),
      ).toBe(false);
    }
  });
});

describe("computeGodotRecoveryCommandDigest", () => {
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

  it("is deterministic", () => {
    expect(computeGodotRecoveryCommandDigest(parts)).toBe(computeGodotRecoveryCommandDigest(parts));
  });

  it("binds the executable, arguments, profile, and limits", () => {
    const base = computeGodotRecoveryCommandDigest(parts);
    expect(
      computeGodotRecoveryCommandDigest({ ...parts, executableSha256: "b".repeat(64) }),
    ).not.toBe(base);
    expect(
      computeGodotRecoveryCommandDigest({
        ...parts,
        argumentTemplate: [...parts.argumentTemplate, "--extra"],
      }),
    ).not.toBe(base);
    expect(
      computeGodotRecoveryCommandDigest({ ...parts, profileId: "godot-probe-offline" }),
    ).not.toBe(base);
    expect(computeGodotRecoveryCommandDigest({ ...parts, timeoutMs: 30_000 })).not.toBe(base);
  });
});

describe("createGodotRecoveryRunner", () => {
  it("is never available on this stage", async () => {
    const runner = createGodotRecoveryRunner({ backend: {} });
    expect(await runner.isAvailable()).toBe(false);
  });

  it("reports unavailable without ever invoking the backend", async () => {
    const backend = { execute: vi.fn() };
    const runner = createGodotRecoveryRunner({ backend });
    const outcome = await runner.run(request());
    expect(outcome.status).toBe("unavailable");
    if (outcome.status !== "unavailable") {
      throw new Error("unreachable");
    }
    expect(outcome.message).toBe(GODOT_RECOVERY_RUN_UNAVAILABLE_MESSAGE);
    expect(backend.execute).not.toHaveBeenCalled();
  });

  it("rejects runtime-only engines as unsupported before the unavailable gate", async () => {
    const backend = { execute: vi.fn() };
    const runner = createGodotRecoveryRunner({ backend });
    const outcome = await runner.run({
      ...request(),
      engineProfile: { ...engineProfile(), edition: "runtime-only" },
    });
    expect(outcome.status).toBe("unsupported");
    expect(backend.execute).not.toHaveBeenCalled();
  });

  it("rejects engines without recovery mode as unsupported", async () => {
    const backend = { execute: vi.fn() };
    const runner = createGodotRecoveryRunner({ backend });
    const outcome = await runner.run({
      ...request(),
      engineProfile: {
        ...engineProfile(),
        capabilities: { ...engineProfile().capabilities, recoveryMode: false },
      },
    });
    expect(outcome.status).toBe("unsupported");
    if (outcome.status === "unsupported") {
      expect(outcome.message).toContain("--recovery-mode");
    }
    expect(backend.execute).not.toHaveBeenCalled();
  });

  it("rejects invalid installations without any launch", async () => {
    const backend = { execute: vi.fn() };
    const runner = createGodotRecoveryRunner({ backend });
    const outcome = await runner.run({
      ...request(),
      installation: { ...installation(), status: "invalid" },
    });
    expect(outcome.status).toBe("unsupported");
    expect(backend.execute).not.toHaveBeenCalled();
  });

  it("throws an abort error when cancelled before refusal", async () => {
    const controller = new AbortController();
    controller.abort();
    const runner = createGodotRecoveryRunner({ backend: {} });
    await expect(runner.run({ ...request(), signal: controller.signal })).rejects.toMatchObject({
      name: "AbortError",
    });
  });
});
