import { mkdtemp, mkdir, rm, stat, utimes, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { DEFAULT_USER_CONFIG, type UserGodotConfig } from "../../config/user-config.js";
import { createEngineProfileCache } from "../cache/engine-profile-cache.js";
import { createGodotEngineProfiler, deduplicateCandidates } from "./engine-profiler.js";
import { createFakeGodotProbeRunner } from "../testing/fake-godot-probe-runner.js";
import type {
  GodotApplicationEvent,
  GodotInstallation,
  GodotSelectionPreference,
} from "@solaris/core";
import type { SandboxBackend } from "@solaris/core";

const tempDirectories: string[] = [];

async function withTemp(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "solaris-godot-profiler-"));
  tempDirectories.push(directory);
  return directory;
}

afterEach(async () => {
  for (const directory of tempDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
});

async function executableFixture(directory: string, name = "godot.exe"): Promise<string> {
  await mkdir(directory, { recursive: true });
  const path = join(directory, name);
  await writeFile(path, `fake executable ${name}`);
  return path;
}

function availableBackend(): SandboxBackend {
  return {
    id: "fake",
    inspect() {
      return Promise.resolve({
        backendId: "fake",
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
      });
    },
    execute() {
      return Promise.reject(new Error("the fake backend never executes"));
    },
    close() {
      return Promise.resolve();
    },
  };
}

interface ProfilerSetup {
  readonly config: UserGodotConfig;
  readonly preference: GodotSelectionPreference;
  readonly overrideSource: "cli" | "environment" | null;
  readonly hostPath: string | null;
  readonly hostPathExt: string | null;
  readonly platform: NodeJS.Platform;
  readonly versionText?: string;
  readonly helpText?: string;
  readonly advertiseApiDump?: boolean;
  readonly available?: boolean;
  readonly backend?: SandboxBackend;
}

async function runDiscovery(
  setup: Partial<ProfilerSetup> & {
    readonly workspaceRoot: string;
    readonly events?: GodotApplicationEvent[];
  },
) {
  const config = setup.config ?? DEFAULT_USER_CONFIG.godot;
  const fake = createFakeGodotProbeRunner({
    versionText: setup.versionText ?? "4.7.1.stable.official",
    ...(setup.helpText === undefined ? {} : { helpText: setup.helpText }),
    advertiseApiDump: setup.advertiseApiDump ?? true,
    ...(setup.available === undefined ? {} : { available: setup.available }),
  });
  const events: GodotApplicationEvent[] = [];
  const cacheRoot = join(setup.workspaceRoot, "..", "cache");
  const profiler = createGodotEngineProfiler({
    config,
    preference: setup.preference ?? { kind: "auto" },
    overrideSource: setup.overrideSource ?? null,
    workspaceRoot: setup.workspaceRoot,
    backend: setup.backend ?? availableBackend(),
    probeRunner: fake.runner,
    cache: createEngineProfileCache({ rootDirectory: cacheRoot }),
    hostPath: setup.hostPath ?? null,
    hostPathExt: setup.hostPathExt ?? null,
    platform: setup.platform ?? "win32",
    onEvent: (event) => {
      events.push(event);
      setup.events?.push(event);
    },
  });
  const result = await profiler.discover();
  return { result, fake, events };
}

describe("createGodotEngineProfiler", () => {
  it("reports every candidate unprofiled when probing is unavailable", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const bin = join(root, "bin");
    await mkdir(bin, { recursive: true });
    const executable = await executableFixture(bin);
    const { result, fake } = await runDiscovery({
      workspaceRoot: workspace,
      config: {
        activeInstallation: null,
        installations: { primary: { path: executable, editionHint: "standard" } },
        discoverOnPath: false,
      },
      available: false,
    });
    // No probe was attempted and no candidate was profiled: the cache is
    // bypassed and every candidate reports the fail-closed reason.
    expect(fake.calls()).toEqual({ version: 0, help: 0, api: 0 });
    expect(result.selected).toBeNull();
    const candidate = result.candidates[0];
    expect(candidate?.profiled).toBe(false);
    expect(candidate?.invalid).toContain("probing is unavailable");
  });

  it("selects a configured verified baseline installation", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const bin = join(root, "bin");
    const executable = await executableFixture(bin);
    const { result } = await runDiscovery({
      workspaceRoot: workspace,
      config: {
        activeInstallation: "primary",
        installations: { primary: { path: executable, editionHint: "standard" } },
        discoverOnPath: false,
      },
    });
    expect(result.selected?.installationId).toBe("primary");
    expect(result.selected?.version?.raw).toBe("4.7.1.stable.official");
    expect(result.selected?.support).toBe("verified");
    expect(result.selected?.edition).toBe("standard");
    expect(result.candidates.length).toBe(1);
  });

  it("keeps invalid configured installations visible with an error", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const { result } = await runDiscovery({
      workspaceRoot: workspace,
      config: {
        activeInstallation: null,
        installations: { broken: { path: join(root, "missing.exe"), editionHint: "standard" } },
        discoverOnPath: false,
      },
    });
    expect(result.candidates.length).toBe(1);
    expect(result.candidates[0]?.installationId).toBe("broken");
    expect(result.candidates[0]?.invalid).not.toBeNull();
    expect(result.selected).toBeNull();
  });

  it("reports an unknown configured active installation and falls back with a diagnostic", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const bin = join(root, "bin");
    const executable = await executableFixture(bin);
    const { result } = await runDiscovery({
      workspaceRoot: workspace,
      preference: { kind: "config-active" },
      config: {
        activeInstallation: "missing-id",
        installations: { primary: { path: executable, editionHint: "standard" } },
        discoverOnPath: false,
      },
    });
    expect(result.selected?.installationId).toBe("primary");
    expect(result.diagnostics.some((d) => d.message.includes("missing-id"))).toBe(true);
  });

  it("fails hard on an invalid explicit path override", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const promise = runDiscovery({
      workspaceRoot: workspace,
      preference: { kind: "path", path: join(root, "missing.exe") },
      config: { activeInstallation: null, installations: {}, discoverOnPath: false },
    });
    await expect(promise).rejects.toThrow(/explicit Godot path/);
  });

  it("selects an explicit CLI path even when nothing is configured", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const bin = join(root, "bin");
    const executable = await executableFixture(bin);
    const { result, events } = await runDiscovery({
      workspaceRoot: workspace,
      preference: { kind: "path", path: executable },
      overrideSource: "cli",
      config: { activeInstallation: null, installations: {}, discoverOnPath: false },
    });
    expect(result.selected?.installationId).toBe("explicit");
    expect(result.selected?.source).toBe("cli-path");
    expect(events.some((event) => event.type === "godot_discovery_started")).toBe(true);
    expect(events.some((event) => event.type === "godot_probe_started")).toBe(true);
  });

  it("selects an explicit installation id and fails clearly for unknown ids", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const bin = join(root, "bin");
    const executable = await executableFixture(bin);
    const config: UserGodotConfig = {
      activeInstallation: null,
      installations: { primary: { path: executable, editionHint: "standard" } },
      discoverOnPath: false,
    };
    const selected = await runDiscovery({
      workspaceRoot: workspace,
      preference: { kind: "installation-id", installationId: "primary" },
      config,
    });
    expect(selected.result.selected?.installationId).toBe("primary");
    const profiler = createGodotEngineProfiler({
      config,
      preference: { kind: "installation-id", installationId: "nope" },
      overrideSource: null,
      workspaceRoot: workspace,
      backend: availableBackend(),
      probeRunner: createFakeGodotProbeRunner().runner,
      cache: createEngineProfileCache({ rootDirectory: join(root, "cache") }),
      hostPath: null,
      hostPathExt: null,
      platform: "win32",
    });
    await expect(profiler.discover()).rejects.toThrow(/installation id/);
  });

  it("prefers stable standard over prerelease and dotnet", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const bin = join(root, "bin");
    await mkdir(bin, { recursive: true });
    const standard = await executableFixture(bin, "godot-stable.exe");
    const dotnet = await executableFixture(bin, "godot-mono.exe");
    const rc = await executableFixture(bin, "godot-rc.exe");
    const { result } = await runDiscovery({
      workspaceRoot: workspace,
      versionText: "4.7.1.stable.official",
      config: {
        activeInstallation: null,
        installations: {
          standard: { path: standard, editionHint: "standard" },
          dotnet: { path: dotnet, editionHint: "dotnet" },
          rc: { path: rc, editionHint: "unknown" },
        },
        discoverOnPath: false,
      },
    });
    expect(result.selected?.installationId).toBe("standard");
  });

  it("reports prerelease versions truthfully", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const bin = join(root, "bin");
    const executable = await executableFixture(bin);
    const { result } = await runDiscovery({
      workspaceRoot: workspace,
      versionText: "4.7.2.rc1.official",
      config: {
        activeInstallation: null,
        installations: { rc: { path: executable, editionHint: "standard" } },
        discoverOnPath: false,
      },
    });
    expect(result.selected?.support).toBe("prerelease-untested");
    expect(result.selected?.releaseChannel).toBe("release-candidate");
  });

  it("never auto-selects Godot 3 or runtime-only binaries", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const bin = join(root, "bin");
    const godot3 = await executableFixture(bin, "godot3.exe");
    const runtime = await executableFixture(bin, "godot-runtime.exe");
    const { result } = await runDiscovery({
      workspaceRoot: workspace,
      versionText: "3.6.1.stable.official",
      helpText: "--headless\n",
      config: {
        activeInstallation: null,
        installations: {
          godot3: { path: godot3, editionHint: "standard" },
          runtime: { path: runtime, editionHint: "unknown" },
        },
        discoverOnPath: false,
      },
    });
    expect(result.selected).toBeNull();
  });

  it("reports version probe failures as unprofiled candidates", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const bin = join(root, "bin");
    const executable = await executableFixture(bin);
    const { result } = await runDiscovery({
      workspaceRoot: workspace,
      versionText: "garbage output",
      config: {
        activeInstallation: null,
        installations: { broken: { path: executable, editionHint: "standard" } },
        discoverOnPath: false,
      },
    });
    expect(result.candidates[0]?.profiled).toBe(false);
    expect(result.candidates[0]?.invalid).not.toBeNull();
  });

  it("deduplicates canonical paths across sources", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const bin = join(root, "bin");
    await mkdir(bin, { recursive: true });
    const executable = await executableFixture(bin);
    const { result } = await runDiscovery({
      workspaceRoot: workspace,
      config: {
        activeInstallation: null,
        installations: { primary: { path: executable, editionHint: "standard" } },
        discoverOnPath: true,
      },
      hostPath: bin,
      hostPathExt: ".EXE",
    });
    const valid = result.candidates.filter((candidate) => candidate.invalid === null);
    expect(valid.length).toBe(1);
    expect(result.diagnostics).toEqual([]);
  });

  it("is deterministic across repeated discovery calls", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const bin = join(root, "bin");
    const executable = await executableFixture(bin);
    const config: UserGodotConfig = {
      activeInstallation: null,
      installations: { primary: { path: executable, editionHint: "standard" } },
      discoverOnPath: false,
    };
    const first = await runDiscovery({ workspaceRoot: workspace, config });
    const second = await runDiscovery({ workspaceRoot: workspace, config });
    expect(second.result).toEqual(first.result);
  });

  it("emits project-independent probe events with bounded surface", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const bin = join(root, "bin");
    const executable = await executableFixture(bin);
    const events: GodotApplicationEvent[] = [];
    await runDiscovery({
      workspaceRoot: workspace,
      config: {
        activeInstallation: null,
        installations: { primary: { path: executable, editionHint: "standard" } },
        discoverOnPath: false,
      },
      events,
    });
    expect(events.some((event) => event.type === "godot_candidate_found")).toBe(true);
    const probes = events.filter((event) => event.type === "godot_probe_started");
    expect(probes.length).toBeGreaterThanOrEqual(2);
  });

  it("serves unchanged executables from the cache without re-probing", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const bin = join(root, "bin");
    const executable = await executableFixture(bin);
    const fake = createFakeGodotProbeRunner({
      versionText: "4.7.1.stable.official",
      advertiseApiDump: true,
    });
    const profiler = createGodotEngineProfiler({
      config: {
        activeInstallation: null,
        installations: { primary: { path: executable, editionHint: "standard" } },
        discoverOnPath: false,
      },
      preference: { kind: "auto" },
      overrideSource: null,
      workspaceRoot: workspace,
      backend: availableBackend(),
      probeRunner: fake.runner,
      cache: createEngineProfileCache({ rootDirectory: join(root, "cache") }),
      hostPath: null,
      hostPathExt: null,
      platform: "win32",
    });
    const first = await profiler.discover();
    expect(fake.calls().version).toBe(1);
    expect(first.selected?.fingerprint).toBeTruthy();
    const second = await profiler.discover();
    expect(fake.calls().version).toBe(1);
    expect(second.selected?.fingerprint).toBe(first.selected?.fingerprint);
  });

  it("never serves a stale profile after a same-size content replacement with restored mtime", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const bin = join(root, "bin");
    const executable = await executableFixture(bin);
    const fake = createFakeGodotProbeRunner({
      versionText: "4.7.1.stable.official",
      advertiseApiDump: true,
    });
    const profiler = createGodotEngineProfiler({
      config: {
        activeInstallation: null,
        installations: { primary: { path: executable, editionHint: "standard" } },
        discoverOnPath: false,
      },
      preference: { kind: "auto" },
      overrideSource: null,
      workspaceRoot: workspace,
      backend: availableBackend(),
      probeRunner: fake.runner,
      cache: createEngineProfileCache({ rootDirectory: join(root, "cache") }),
      hostPath: null,
      hostPathExt: null,
      platform: "win32",
    });
    const first = await profiler.discover();
    expect(fake.calls().version).toBe(1);
    const metadata = await stat(executable);
    await writeFile(executable, "Y".repeat(metadata.size));
    await utimes(executable, metadata.atime, metadata.mtime);
    const second = await profiler.discover();
    expect(fake.calls().version).toBe(2);
    expect(second.selected?.fingerprint).not.toBe(first.selected?.fingerprint);
  });

  it("notices candidate additions and removals on every discover", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    const bin = join(root, "bin");
    const first = await executableFixture(bin, "godot-a.exe");
    const config: UserGodotConfig = {
      activeInstallation: null,
      installations: { a: { path: first, editionHint: "standard" } },
      discoverOnPath: false,
    };
    const fake = createFakeGodotProbeRunner({
      versionText: "4.7.1.stable.official",
      advertiseApiDump: true,
    });
    const profiler = createGodotEngineProfiler({
      config,
      preference: { kind: "auto" },
      overrideSource: null,
      workspaceRoot: workspace,
      backend: availableBackend(),
      probeRunner: fake.runner,
      cache: createEngineProfileCache({ rootDirectory: join(root, "cache") }),
      hostPath: null,
      hostPathExt: null,
      platform: "win32",
    });
    const firstResult = await profiler.discover();
    expect(firstResult.candidates).toHaveLength(1);
    const second = await executableFixture(bin, "godot-b.exe");
    const mutableInstallations = config.installations as Record<
      string,
      { readonly path: string; readonly editionHint: "standard" | "dotnet" | "unknown" }
    >;
    mutableInstallations["b"] = { path: second, editionHint: "standard" };
    const secondResult = await profiler.discover();
    expect(secondResult.candidates).toHaveLength(2);
    await rm(second, { force: true });
    const thirdResult = await profiler.discover();
    expect(thirdResult.candidates).toHaveLength(2);
    const valid = thirdResult.candidates.filter((candidate) => candidate.invalid === null);
    expect(valid).toHaveLength(1);
  });
});

describe("deduplicateCandidates", () => {
  function candidate(id: string, canonicalPath: string): GodotInstallation {
    return {
      id,
      sourceLabel: "user config",
      source: "user-config",
      canonicalPath,
      sizeBytes: 1,
      modifiedAtMs: 0,
      sha256: "a".repeat(64),
      editionHint: "unknown",
      status: "valid",
    };
  }

  it("does not deduplicate distinct case-sensitive paths on Linux", () => {
    const { deduped, duplicates } = deduplicateCandidates(
      [candidate("upper", "/opt/godot/Godot"), candidate("lower", "/opt/godot/godot")],
      "linux",
    );
    expect(deduped).toHaveLength(2);
    expect(duplicates.size).toBe(0);
  });

  it("folds case on Windows and macOS", () => {
    const windows = deduplicateCandidates(
      [
        candidate("one", "C:\\Program Files\\Godot\\godot.exe"),
        candidate("two", "c:\\program files\\godot\\GODOT.EXE"),
      ],
      "win32",
    );
    expect(windows.deduped).toHaveLength(1);
    expect(windows.duplicates.has("two")).toBe(true);
    const mac = deduplicateCandidates(
      [
        candidate("one", "/Applications/Godot.app/Contents/MacOS/Godot"),
        candidate("two", "/APPLICATIONS/godot.app/CONTENTS/macos/godot"),
      ],
      "darwin",
    );
    expect(mac.deduped).toHaveLength(1);
    expect(mac.duplicates.has("two")).toBe(true);
  });

  it("keeps invalid candidates visible and never folds them away", () => {
    const invalidCandidate: GodotInstallation = {
      ...candidate("broken", "/opt/godot/broken"),
      status: "invalid",
      error: "rejected",
    };
    const { deduped, duplicates } = deduplicateCandidates(
      [invalidCandidate, candidate("broken2", "/opt/godot/broken")],
      "linux",
    );
    expect(deduped).toHaveLength(2);
    expect(duplicates.size).toBe(0);
  });
});
