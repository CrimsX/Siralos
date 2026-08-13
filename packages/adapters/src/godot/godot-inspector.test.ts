import { chmod, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  type GodotApiDumpProbe,
  type GodotHelpProbe,
  type GodotProbeRunner,
  type GodotVersionProbe,
  type SandboxBackend,
} from "@siralos/core";
import { createGodotInspector } from "./godot-inspector.js";
import { createEngineProfileCache } from "./cache/engine-profile-cache.js";
import { createFakeGodotProbeRunner } from "./testing/fake-godot-probe-runner.js";

const tempDirectories: string[] = [];

afterEach(async () => {
  for (const directory of tempDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
});

async function withTemp(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "siralos-godot-inspector-"));
  tempDirectories.push(directory);
  return directory;
}

async function executableFixture(directory: string, name = "godot-test.exe"): Promise<string> {
  await mkdir(directory, { recursive: true });
  const path = join(directory, name);
  await writeFile(path, `#!/bin/sh\necho "4.7.1.stable.official"\n`);
  if (process.platform !== "win32") {
    await chmod(path, 0o755);
  }
  return path;
}

function enforcingBackend(): SandboxBackend {
  return {
    id: "enforcing-test-backend",
    inspect() {
      return Promise.resolve({
        backendId: "enforcing-test-backend",
        state: "available",
        platform: "linux",
        version: "0.0.0-test",
        capabilities: {
          filesystemReadRestriction: true,
          filesystemWriteRestriction: true,
          networkRestriction: true,
          processTreeRestriction: true,
          violationReporting: true,
        },
      });
    },
    execute(): Promise<never> {
      return Promise.reject(new Error("must not execute"));
    },
    close() {
      return Promise.resolve();
    },
  };
}

/**
 * Scripted runner whose version probe succeeds on the first call and fails
 * on later calls, simulating an engine that changes or disappears between
 * two discovery generations.
 */
function disappearingRunner(): {
  runner: GodotProbeRunner;
  versionCalls: () => number;
} {
  let versionCalls = 0;
  const versionProbe: GodotVersionProbe = {
    status: "success",
    version: {
      raw: "4.7.1.stable.official",
      major: 4,
      minor: 7,
      patch: 1,
      status: "stable",
      statusNumber: null,
      build: "official",
      commit: null,
    },
  };
  const helpProbe: GodotHelpProbe = {
    status: "success",
    capabilities: {
      editor: true,
      projectManager: true,
      recoveryMode: true,
      headless: true,
      projectPath: true,
      scene: true,
      script: true,
      checkOnly: true,
      import: true,
      quit: true,
      quitAfter: true,
      lsp: true,
      dap: true,
      debugServer: true,
      buildSolutions: true,
      extensionApiDump: true,
      extensionApiWithDocsDump: true,
      extensionApiValidation: true,
      docTool: true,
      movieWriting: true,
    },
    unknownOptionCount: 0,
  };
  const apiDumpProbe: GodotApiDumpProbe = {
    status: "success",
    summary: {
      headerVersion: "4.7.1",
      apiHash: "abc",
      classCount: 1,
      builtinClassCount: 1,
      globalEnumCount: 0,
      utilityFunctionCount: 0,
      configurationVersion: 5,
      fileSizeBytes: 10,
      sha256: "c".repeat(64),
    },
  };
  const runner: GodotProbeRunner = {
    isAvailable() {
      return Promise.resolve(true);
    },
    probeVersion(): Promise<GodotVersionProbe> {
      versionCalls += 1;
      if (versionCalls > 1) {
        return Promise.resolve({
          status: "failed",
          message: "The Godot executable disappeared between discoveries.",
        });
      }
      return Promise.resolve(versionProbe);
    },
    probeHelp(): Promise<GodotHelpProbe> {
      return Promise.resolve(helpProbe);
    },
    dumpExtensionApi(): Promise<GodotApiDumpProbe> {
      return Promise.resolve(apiDumpProbe);
    },
  };
  return { runner, versionCalls: () => versionCalls };
}

describe("Godot doctor snapshot consistency", () => {
  it("produces the lightweight status snapshot from one discovery generation", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    await mkdir(workspace, { recursive: true });
    const executable = await executableFixture(join(root, "bin"));
    const { runner, versionCalls } = disappearingRunner();
    const inspector = createGodotInspector({
      config: {
        activeInstallation: null,
        installations: { primary: { path: executable, editionHint: "standard" } },
        discoverOnPath: false,
      },
      preference: { kind: "installation-id", installationId: "primary" },
      overrideSource: null,
      workspaceRoot: workspace,
      backend: enforcingBackend(),
      probeRunner: runner,
      cache: createEngineProfileCache({ rootDirectory: join(root, "cache") }),
      hostPath: null,
      hostPathExt: null,
      platform: "win32",
    });

    const snapshot = await inspector.statusSnapshot!();
    expect(versionCalls()).toBe(1);
    expect(snapshot.selected?.version.raw).toBe("4.7.1.stable.official");
    expect(snapshot.project.detected).toBe(false);
    expect(snapshot.compatibility.status).toBe("no-project");
  });

  it("produces the doctor report from ONE discovery generation", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    await mkdir(workspace, { recursive: true });
    const bin = join(root, "bin");
    const executable = await executableFixture(bin);
    const { runner, versionCalls } = disappearingRunner();
    const config = {
      activeInstallation: null,
      installations: { primary: { path: executable, editionHint: "standard" as const } },
      discoverOnPath: false,
    };
    const inspector = createGodotInspector({
      config,
      preference: { kind: "installation-id", installationId: "primary" },
      overrideSource: null,
      workspaceRoot: workspace,
      backend: enforcingBackend(),
      probeRunner: runner,
      cache: createEngineProfileCache({ rootDirectory: join(root, "cache") }),
      hostPath: null,
      hostPathExt: null,
      platform: "win32",
    });
    const report = await inspector.doctor();
    // Exactly one discovery generation ran: the doctor never re-discovers
    // for the selected profile, so an engine that would have disappeared
    // between two generations cannot produce an incompatible snapshot.
    expect(versionCalls()).toBe(1);
    // The report is internally consistent: the discovery and the selected
    // profile (and its degraded capabilities) come from the same run.
    if (report.discovery.selected !== null) {
      expect(report.discovery.selected.profiled).toBe(true);
      expect(report.degradedCapabilities).toEqual([]);
    } else {
      expect(report.degradedCapabilities).toEqual([]);
    }
  });

  it("produces a consistent unavailable report when the engine cannot be probed", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    await mkdir(workspace, { recursive: true });
    const bin = join(root, "bin");
    const executable = await executableFixture(bin);
    const fake = createFakeGodotProbeRunner({ available: false });
    const config = {
      activeInstallation: null,
      installations: { primary: { path: executable, editionHint: "standard" as const } },
      discoverOnPath: false,
    };
    const inspector = createGodotInspector({
      config,
      preference: { kind: "auto" },
      overrideSource: null,
      workspaceRoot: workspace,
      backend: enforcingBackend(),
      probeRunner: fake.runner,
      cache: createEngineProfileCache({ rootDirectory: join(root, "cache") }),
      hostPath: null,
      hostPathExt: null,
      platform: "win32",
    });
    const report = await inspector.doctor();
    expect(fake.calls()).toEqual({ version: 0, help: 0, api: 0 });
    expect(report.discovery.selected).toBeNull();
    expect(report.discovery.candidates[0]?.profiled).toBe(false);
    expect(report.degradedCapabilities).toEqual([]);
    expect(report.project.detected).toBe(false);
  });
});
