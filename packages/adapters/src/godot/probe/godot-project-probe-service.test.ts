import { afterEach, describe, expect, it } from "vitest";
import { mkdtemp, mkdir, rm, writeFile, readFile, readdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import {
  type GodotProjectProbe,
  type GitInspector,
  type GitStatusResult,
  type GitWorkspaceStatus,
  type SandboxBackend,
  type SandboxBackendStatus,
  type SandboxedProcessRequest,
  type SandboxedProcessResult,
} from "@solaris/core";
import { completedResult } from "../../sandbox/fake-sandbox-backend.js";
import { createFakeGodotProbeRunner } from "../testing/fake-godot-probe-runner.js";
import { createProjectMirror } from "../mirror/project-mirror.js";
import { createRunDirectoryProvider } from "../../process/run-directories.js";
import { createGodotProjectProbeService } from "./godot-project-probe-service.js";
import { type UserGodotConfig } from "../../config/user-config.js";
import type { ProjectMirror } from "@solaris/core";

const tempRoots: string[] = [];

async function withTempRoot(): Promise<string> {
  const root = await mkdtemp(path.join(tmpdir(), "solaris-probe-test-"));
  tempRoots.push(root);
  return root;
}

async function writeFiles(root: string, files: Readonly<Record<string, string>>): Promise<void> {
  for (const [relativePath, content] of Object.entries(files)) {
    const target = path.join(root, relativePath);
    await mkdir(path.dirname(target), { recursive: true });
    await writeFile(target, content);
  }
}

function availableStatus(): SandboxBackendStatus {
  return {
    backendId: "scripted-backend",
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
  };
}

function createScriptedBackend(
  results: readonly SandboxedProcessResult[],
  onExecute?: (request: SandboxedProcessRequest) => Promise<void>,
): {
  backend: SandboxBackend;
  requests: () => SandboxedProcessRequest[];
} {
  const requests: SandboxedProcessRequest[] = [];
  let index = 0;
  return {
    backend: {
      id: "scripted-backend",
      inspect(): Promise<SandboxBackendStatus> {
        return Promise.resolve(availableStatus());
      },
      async execute(request: SandboxedProcessRequest): Promise<SandboxedProcessResult> {
        requests.push(request);
        if (onExecute !== undefined) {
          await onExecute(request);
        }
        const scripted = results[index];
        index += 1;
        return scripted ?? completedResult();
      },
      close(): Promise<void> {
        return Promise.resolve();
      },
    },
    requests: () => requests,
  };
}

const DEFAULT_HELP_TEXT = [
  "--editor  Starts the editor.",
  "--project-manager  Starts the project manager.",
  "--headless  Runs headless.",
  "--recovery-mode  Runs the editor in recovery mode.",
  "--path <directory>  Sets the project path.",
  "--quit-after <int>  Quit after N iterations.",
  "--quit  Quit after the first iteration.",
].join("\n");

interface ProbeHarnessOptions {
  readonly workspaceFiles?: Readonly<Record<string, string>>;
  readonly backendResults?: readonly SandboxedProcessResult[];
  readonly onExecute?: (request: SandboxedProcessRequest) => Promise<void>;
  readonly mirror?: ProjectMirror;
  readonly git?: GitInspector;
  readonly helpText?: string;
  readonly noProject?: boolean;
}

interface ProbeHarness {
  readonly service: GodotProjectProbe;
  readonly workspaceRoot: string;
  readonly runsRoot: string;
  readonly backendRequests: () => SandboxedProcessRequest[];
  readonly mirror: ProjectMirror;
}

async function createHarness(options: ProbeHarnessOptions = {}): Promise<ProbeHarness> {
  const workspaceRoot = await withTempRoot();
  const runsRoot = await withTempRoot();
  const executableRoot = await withTempRoot();
  const executable = path.join(executableRoot, "godot-test.exe");
  const executableContent = "#!/bin/sh\necho fixture\n";
  await writeFile(executable, executableContent);
  if (options.noProject !== true) {
    await writeFiles(workspaceRoot, {
      "project.godot":
        '[application]\nconfig/name="Fixture"\nconfig/features=PackedStringArray("4.7")\n',
      "src/main.gd": "extends Node\nfunc _ready():\n    pass\n",
      ...options.workspaceFiles,
    });
  } else {
    await writeFiles(workspaceRoot, options.workspaceFiles ?? {});
  }
  const backendState = createScriptedBackend(options.backendResults ?? [], options.onExecute);
  const config: UserGodotConfig = {
    activeInstallation: null,
    installations: {
      "test-install": { path: executable, editionHint: "standard" },
    },
    discoverOnPath: false,
  };
  const cache = {
    load: (): Promise<null> => Promise.resolve(null),
    store: (): Promise<void> => Promise.resolve(),
    count: (): Promise<number> => Promise.resolve(0),
  };
  const fakeProbe = createFakeGodotProbeRunner({
    helpText: options.helpText ?? DEFAULT_HELP_TEXT,
  });
  const runDirectories = createRunDirectoryProvider({ workspaceRoot, runsRoot });
  const mirror = options.mirror ?? createProjectMirror();
  const service = createGodotProjectProbeService({
    workspaceRoot,
    config,
    preference: { kind: "installation-id", installationId: "test-install" },
    overrideSource: "cli",
    backend: backendState.backend,
    probeRunner: fakeProbe.runner,
    cache,
    hostPath: null,
    hostPathExt: null,
    platform: process.platform,
    runDirectories,
    mirror,
    checkpointRoot: null,
    ...(options.git === undefined ? {} : { git: options.git }),
    parentEnvironment: { PATH: "/usr/bin" },
  });
  return {
    service,
    workspaceRoot,
    runsRoot,
    backendRequests: backendState.requests,
    mirror,
  };
}

async function prepareAndApprove(service: GodotProjectProbe): Promise<{
  readonly prepared: Awaited<ReturnType<GodotProjectProbe["prepare"]>>;
  readonly digest: string;
  readonly probe: import("@solaris/core").PreparedGodotProbe;
}> {
  const prepared = await service.prepare();
  if (prepared.status !== "ready") {
    return { prepared, digest: "", probe: undefined as never };
  }
  return { prepared, digest: prepared.digest, probe: prepared.probe };
}

function cleanRunDirs(runsRoot: string): Promise<string[]> {
  return readdir(runsRoot).catch(() => []);
}

async function assertNoRuns(runsRoot: string): Promise<void> {
  const entries = await cleanRunDirs(runsRoot);
  for (const entry of entries) {
    // The fingerprint directory may remain; it must contain no run
    // directories after cleanup.
    const sub = await readdir(path.join(runsRoot, entry));
    expect(sub).toEqual([]);
  }
}

afterEach(async () => {
  await Promise.all(
    tempRoots.map((root) => rm(root, { recursive: true, force: true }).catch(() => undefined)),
  );
  tempRoots.length = 0;
});

describe("recovery probe results", () => {
  it("reports completed for a clean startup", async () => {
    const { service, backendRequests, runsRoot } = await createHarness({
      backendResults: [
        completedResult({
          stdout: "Godot Engine v4.7.1.stable.official - https://godotengine.org\n",
        }),
      ],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    expect(prepared.preview.engineVersion).toBe("4.7.1.stable.official");
    expect(prepared.preview.isolation.recoveryMode).toBe(true);
    expect(prepared.preview.risks.toolScripts).toBe(0);
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("completed");
    expect(result.recoveryMode).toBe(true);
    expect(result.process.exitCode).toBe(0);
    expect(result.workspaceIntegrity.unchanged).toBe(true);
    expect(result.cleanup.completed).toBe(true);
    expect(backendRequests().length).toBe(1);
    expect(backendRequests()[0]?.arguments).toContain("--recovery-mode");
    await assertNoRuns(runsRoot);
  });

  it("represents startup warnings as diagnostics", async () => {
    const { service } = await createHarness({
      backendResults: [
        completedResult({ stderr: "WARNING: Some resource property is deprecated.\n" }),
      ],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("completed_with_diagnostics");
    expect(result.diagnostics.warnings).toHaveLength(1);
    expect(result.diagnostics.warnings[0]?.message).toContain("deprecated");
    expect(result.diagnostics.errors).toHaveLength(0);
  });

  it("represents parser errors", async () => {
    const { service } = await createHarness({
      backendResults: [
        completedResult({
          stderr: "SCRIPT ERROR: Parse Error: Unexpected token '}' in script.\n",
        }),
      ],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("completed_with_diagnostics");
    const error = result.diagnostics.errors[0];
    expect(error?.category).toBe("parser");
    expect(error?.severity).toBe("error");
  });

  it("represents missing resources and import warnings", async () => {
    const { service } = await createHarness({
      backendResults: [
        completedResult({
          stderr: [
            "ERROR: Failed loading resource: res://missing.tscn",
            "WARNING: Failed to import resource: res://assets/logo.png",
          ].join("\n"),
        }),
      ],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("completed_with_diagnostics");
    expect(result.diagnostics.errors[0]?.category).toBe("resource");
    expect(result.diagnostics.warnings[0]?.category).toBe("import");
  });

  it("represents a nonzero engine exit truthfully", async () => {
    const { service } = await createHarness({
      backendResults: [completedResult({ exitCode: 2 })],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("failed");
    expect(result.process.exitCode).toBe(2);
  });

  it("never reports a timeout as completion", async () => {
    const { service } = await createHarness({
      backendResults: [{ ...completedResult(), status: "timed-out" }],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("timed_out");
    expect(result.process.timedOut).toBe(true);
    expect(result.process.exitCode).toBeNull();
  });

  it("never reports a cancellation as success", async () => {
    const { service } = await createHarness({
      backendResults: [{ ...completedResult(), status: "cancelled" }],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("cancelled");
  });

  it("enforces diagnostic limits and reports truncation", async () => {
    const lines = Array.from({ length: 150 }, (_, index) => `ERROR: error number ${index}`).join(
      "\n",
    );
    const { service } = await createHarness({
      backendResults: [completedResult({ stderr: lines })],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.diagnostics.errors.length).toBeLessThanOrEqual(100);
    expect(result.diagnostics.truncated).toBe(true);
  });

  it("sanitizes terminal escapes in diagnostics", async () => {
    const { service } = await createHarness({
      backendResults: [
        completedResult({ stderr: "ERROR: \u001b[31mred text\u001b[0m and a bell \u0007\n" }),
      ],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    const message = result.diagnostics.errors[0]?.message ?? "";
    expect(message).not.toContain("\u001b");
    expect(message).not.toContain("\u0007");
    expect(message).toContain("red text");
  });

  it("reports generated .godot state inside the mirror only", async () => {
    const { service, runsRoot } = await createHarness({
      backendResults: [completedResult({ stdout: "Godot Engine v4.7.1.stable.official\n" })],
      onExecute: async (request) => {
        const projectPath = request.workingDirectory;
        await mkdir(path.join(projectPath, ".godot", "imported"), { recursive: true });
        await mkdir(path.join(projectPath, ".godot", "editor"), { recursive: true });
        await writeFile(
          path.join(projectPath, ".godot", "imported", "logo.png.import"),
          "imported",
        );
        await writeFile(path.join(projectPath, ".godot", "editor", "cache.log"), "cache");
      },
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.mirror.generatedGodotDirectory).toBe(true);
    expect(result.mirror.generatedFiles).toBeGreaterThanOrEqual(2);
    expect(result.mirror.generatedBytes).toBeGreaterThan(0);
    expect(result.mirror.importState).toBe("imports observed");
    await assertNoRuns(runsRoot);
  });

  it("reports that no import was observed when no imported files exist", async () => {
    const { service } = await createHarness({
      backendResults: [completedResult({})],
      onExecute: async (request) => {
        await mkdir(path.join(request.workingDirectory, ".godot"), { recursive: true });
      },
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.mirror.generatedGodotDirectory).toBe(true);
    expect(result.mirror.importState).toBe("project opened");
  });

  it("reports import state unknown when no .godot was generated", async () => {
    const { service } = await createHarness({ backendResults: [completedResult({})] });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.mirror.generatedGodotDirectory).toBe(false);
    expect(result.mirror.importState).toBe("import state unknown");
  });

  it("fails closed with no engine selected", async () => {
    const workspaceRoot = await withTempRoot();
    const runsRoot = await withTempRoot();
    await writeFiles(workspaceRoot, { "project.godot": "[application]\n" });
    const backendState = createScriptedBackend([]);
    const config: UserGodotConfig = {
      activeInstallation: null,
      installations: {},
      discoverOnPath: false,
    };
    const fakeProbe = createFakeGodotProbeRunner();
    const service = createGodotProjectProbeService({
      workspaceRoot,
      config,
      preference: { kind: "auto" },
      overrideSource: null,
      backend: backendState.backend,
      probeRunner: fakeProbe.runner,
      cache: {
        load: () => Promise.resolve(null),
        store: () => Promise.resolve(),
        count: () => Promise.resolve(0),
      },
      hostPath: null,
      hostPathExt: null,
      platform: process.platform,
      runDirectories: createRunDirectoryProvider({ workspaceRoot, runsRoot }),
      mirror: createProjectMirror(),
      checkpointRoot: null,
      parentEnvironment: {},
    });
    const prepared = await service.prepare();
    expect(prepared.status).toBe("unsupported");
    expect(backendState.requests().length).toBe(0);
  });
});

describe("workspace integrity", () => {
  it("leaves the source workspace untouched and creates no artifacts", async () => {
    const { service, workspaceRoot, runsRoot } = await createHarness({
      backendResults: [completedResult({ stdout: "Godot Engine v4.7.1.stable.official\n" })],
      onExecute: async (request) => {
        await mkdir(path.join(request.workingDirectory, ".godot"), { recursive: true });
      },
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const before = await readdir(workspaceRoot);
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("completed");
    const after = await readdir(workspaceRoot);
    expect(after.sort()).toEqual(before.sort());
    expect(after).not.toContain(".godot");
    expect(after).not.toContain("extension_api.json");
    expect(after.some((name) => name.endsWith(".log"))).toBe(false);
    expect(after.some((name) => name.startsWith(".solaris"))).toBe(false);
    await assertNoRuns(runsRoot);
  });

  it("detects unexpected source mutation during the probe", async () => {
    const { service, workspaceRoot } = await createHarness({
      backendResults: [completedResult({})],
      onExecute: async () => {
        await writeFile(path.join(workspaceRoot, "src", "main.gd"), "tampered during probe\n");
      },
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("workspace_changed");
    expect(result.workspaceIntegrity.unchanged).toBe(false);
    // Solaris never auto-reverts external changes.
    expect(await readFile(path.join(workspaceRoot, "src", "main.gd"), "utf8")).toBe(
      "tampered during probe\n",
    );
  });

  it("detects a project change after approval as a conflict before running", async () => {
    const { service, workspaceRoot, backendRequests } = await createHarness({
      backendResults: [completedResult({})],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    await writeFile(
      path.join(workspaceRoot, "project.godot"),
      '[application]\nconfig/name="Changed"\n',
    );
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("conflict");
    expect(backendRequests().length).toBe(0);
    expect(service.status().state).toBe("probe-invalidated");
  });

  it("detects an executable change after approval as a conflict", async () => {
    const workspaceRoot = await withTempRoot();
    const runsRoot = await withTempRoot();
    const executableRoot = await withTempRoot();
    const executable = path.join(executableRoot, "godot-test.exe");
    await writeFile(executable, "#!/bin/sh\nversion one\n");
    await writeFiles(workspaceRoot, { "project.godot": "[application]\n" });
    const backendState = createScriptedBackend([]);
    const config: UserGodotConfig = {
      activeInstallation: null,
      installations: { "test-install": { path: executable, editionHint: "standard" } },
      discoverOnPath: false,
    };
    const service = createGodotProjectProbeService({
      workspaceRoot,
      config,
      preference: { kind: "installation-id", installationId: "test-install" },
      overrideSource: "cli",
      backend: backendState.backend,
      probeRunner: createFakeGodotProbeRunner({ helpText: DEFAULT_HELP_TEXT }).runner,
      cache: {
        load: () => Promise.resolve(null),
        store: () => Promise.resolve(),
        count: () => Promise.resolve(0),
      },
      hostPath: null,
      hostPathExt: null,
      platform: process.platform,
      runDirectories: createRunDirectoryProvider({ workspaceRoot, runsRoot }),
      mirror: createProjectMirror(),
      checkpointRoot: null,
      parentEnvironment: {},
    });
    const prepared = await service.prepare();
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    await writeFile(executable, "#!/bin/sh\nversion two\n");
    const result = await service.execute(prepared.probe, {
      approvedDigest: prepared.digest,
    });
    expect(result.status).toBe("conflict");
    expect(backendState.requests().length).toBe(0);
  });

  it("rejects execution under a mismatched approval digest", async () => {
    const { service, backendRequests } = await createHarness({
      backendResults: [completedResult({})],
    });
    const prepared = await service.prepare();
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(prepared.probe, {
      approvedDigest: "b".repeat(64),
    });
    expect(result.status).toBe("conflict");
    expect(backendRequests().length).toBe(0);
  });

  it("rejects a reused prepared probe", async () => {
    const { service, backendRequests } = await createHarness({
      backendResults: [completedResult({}), completedResult({})],
    });
    const prepared = await service.prepare();
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const first = await service.execute(prepared.probe, { approvedDigest: prepared.digest });
    expect(first.status).toBe("completed");
    const second = await service.execute(prepared.probe, { approvedDigest: prepared.digest });
    expect(second.status).toBe("failed");
    expect(backendRequests().length).toBe(1);
  });

  it("verifies non-Git workspaces with the authored manifest alone", async () => {
    const { service, workspaceRoot } = await createHarness({
      backendResults: [completedResult({})],
      onExecute: async () => {
        await writeFile(path.join(workspaceRoot, "src", "main.gd"), "changed\n");
      },
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("workspace_changed");
  });

  it("includes Git status in the integrity check when Git is available", async () => {
    const { service } = await createHarness({
      backendResults: [completedResult({})],
      git: createStubGitInspector(
        [],
        [
          {
            path: "src/main.gd",
            originalPath: null,
            indexStatus: "unmodified",
            worktreeStatus: "modified",
            kind: "ordinary",
          },
        ],
      ),
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    // Authored manifest unchanged; only the Git view differs.
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("workspace_changed");
    expect(result.workspaceIntegrity.unchanged).toBe(false);
  });

  it("reports bounded integrity when the baseline is truncated", async () => {
    const { service } = await createHarness({
      backendResults: [completedResult({})],
      workspaceFiles: { "big.bin": "x".repeat(64 * 1024) },
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("completed");
    expect(result.workspaceIntegrity.unchanged).toBe(true);
    expect(result.workspaceIntegrity.bounded).toBe(false);
  });
});

describe("mirror lifecycle and cleanup", () => {
  it("deletes the mirror after a successful probe", async () => {
    const { service, workspaceRoot, runsRoot } = await createHarness({
      backendResults: [completedResult({})],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("completed");
    expect(result.cleanup.completed).toBe(true);
    await assertNoRuns(runsRoot);
    expect(await readdir(workspaceRoot)).not.toContain(".godot");
  });

  it("creates no mirror when the probe is denied", async () => {
    const { service, runsRoot } = await createHarness({ backendResults: [] });
    const prepared = await service.prepare();
    expect(prepared.status).toBe("ready");
    await assertNoRuns(runsRoot);
  });

  it("deletes the mirror on cancellation", async () => {
    const { service, runsRoot } = await createHarness({
      backendResults: [{ ...completedResult(), status: "cancelled" }],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("cancelled");
    expect(result.cleanup.completed).toBe(true);
    await assertNoRuns(runsRoot);
  });

  it("deletes the mirror on timeout", async () => {
    const { service, runsRoot } = await createHarness({
      backendResults: [{ ...completedResult(), status: "timed-out" }],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("timed_out");
    expect(result.cleanup.completed).toBe(true);
    await assertNoRuns(runsRoot);
  });

  it("deletes the mirror when the engine crashes", async () => {
    const { service, runsRoot } = await createHarness({
      backendResults: [completedResult({ exitCode: 139 })],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("failed");
    expect(result.process.exitCode).toBe(139);
    expect(result.cleanup.completed).toBe(true);
    await assertNoRuns(runsRoot);
  });

  it("cleans up after mirror preparation failure", async () => {
    const workspaceRoot = await withTempRoot();
    const runsRoot = await withTempRoot();
    const executableRoot = await withTempRoot();
    const executable = path.join(executableRoot, "godot-test.exe");
    await writeFile(executable, "#!/bin/sh\nx\n");
    await writeFiles(workspaceRoot, { "project.godot": "[application]\n" });
    // A symlinked directory makes mirroring unsupported; the partial state
    // must be cleaned and no engine may launch.
    const outside = await withTempRoot();
    await writeFiles(outside, { "secret.txt": "secret" });
    const { symlink } = await import("node:fs/promises");
    try {
      await symlink(
        outside,
        path.join(workspaceRoot, "linked"),
        process.platform === "win32" ? "junction" : "dir",
      );
    } catch {
      return;
    }
    const backendState = createScriptedBackend([]);
    const config: UserGodotConfig = {
      activeInstallation: null,
      installations: { "test-install": { path: executable, editionHint: "standard" } },
      discoverOnPath: false,
    };
    const service = createGodotProjectProbeService({
      workspaceRoot,
      config,
      preference: { kind: "installation-id", installationId: "test-install" },
      overrideSource: "cli",
      backend: backendState.backend,
      probeRunner: createFakeGodotProbeRunner({ helpText: DEFAULT_HELP_TEXT }).runner,
      cache: {
        load: () => Promise.resolve(null),
        store: () => Promise.resolve(),
        count: () => Promise.resolve(0),
      },
      hostPath: null,
      hostPathExt: null,
      platform: process.platform,
      runDirectories: createRunDirectoryProvider({ workspaceRoot, runsRoot }),
      mirror: createProjectMirror(),
      checkpointRoot: null,
      parentEnvironment: {},
    });
    const prepared = await service.prepare();
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(prepared.probe, { approvedDigest: prepared.digest });
    expect(result.status).toBe("unsupported");
    expect(backendState.requests().length).toBe(0);
    await assertNoRuns(runsRoot);
    expect(await readFile(path.join(outside, "secret.txt"), "utf8")).toBe("secret");
  });

  it("reports a cleanup failure instead of claiming success", async () => {
    const stubMirror: ProjectMirror = {
      prepare() {
        return Promise.resolve({
          status: "ready",
          mirror: {
            projectPath: "stub",
            sourceRoot: "stub",
            parentDirectory: "stub",
            entries: [],
            copiedBytes: 0,
          },
        });
      },
      verify() {
        return Promise.resolve({ ok: true });
      },
      destroy() {
        return Promise.resolve({ ok: false, message: "The stub mirror could not be removed." });
      },
    };
    const { service } = await createHarness({
      backendResults: [completedResult({})],
      mirror: stubMirror,
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("failed");
    expect(result.cleanup.completed).toBe(false);
    expect(result.cleanup.message).toContain("could not be removed");
  });

  it("leaves no run directories behind after a conflict", async () => {
    const { service, workspaceRoot, runsRoot } = await createHarness({
      backendResults: [completedResult({})],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    await writeFile(path.join(workspaceRoot, "project.godot"), "changed");
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("conflict");
    await assertNoRuns(runsRoot);
  });
});

describe("probe status state machine", () => {
  it("starts untrusted with no result", async () => {
    const { service } = await createHarness({ backendResults: [] });
    expect(service.status().state).toBe("untrusted");
    expect(service.status().lastResult).toBeNull();
    expect(service.status().lastManifestDigest).toBeNull();
  });

  it("records the last result and engine after a completed probe", async () => {
    const { service } = await createHarness({
      backendResults: [completedResult({})],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    await service.execute(probe, { approvedDigest: digest });
    const status = service.status();
    expect(status.state).toBe("untrusted");
    expect(status.lastResult?.status).toBe("completed");
    expect(status.lastEngineVersion).toBe("4.7.1.stable.official");
    expect(status.lastManifestDigest).toMatch(/^[0-9a-f]{64}$/);
  });

  it("becomes probe-invalidated after a conflict", async () => {
    const { service, workspaceRoot } = await createHarness({
      backendResults: [completedResult({})],
    });
    const { prepared, digest, probe } = await prepareAndApprove(service);
    if (prepared.status !== "ready") {
      return;
    }
    await writeFile(path.join(workspaceRoot, "project.godot"), "changed");
    await service.execute(probe, { approvedDigest: digest });
    expect(service.status().state).toBe("probe-invalidated");
  });
});

describe("risk manifest coverage", () => {
  it("hashes tool scripts, enabled plugins, autoloads, and dotnet projects into the digest", async () => {
    const { service } = await createHarness({
      workspaceFiles: {
        "tools/editor.gd": "@tool\nextends EditorPlugin\n",
        "addons/demo/plugin.cfg": '[plugin]\nname="Demo"\nscript="plugin.gd"\n',
        "addons/demo/plugin.gd": "@tool\nextends EditorPlugin\n",
        "state.gd": "extends Node\n",
        "MyGame.csproj": "<Project />\n",
      },
      backendResults: [completedResult({})],
    });
    const prepared = await service.prepare();
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    expect(prepared.preview.risks.toolScripts).toBeGreaterThanOrEqual(1);
    expect(prepared.preview.risks.dotnetProjects).toBe(1);
    expect(prepared.preview.manifestDigest).toMatch(/^[0-9a-f]{64}$/);
    expect(prepared.digest).toMatch(/^[0-9a-f]{64}$/);
    expect(prepared.digest).not.toBe(prepared.preview.manifestDigest);
  });

  it("never reads escaping plugin scripts or GDExtension library targets", async () => {
    const workspaceRoot = await withTempRoot();
    const runsRoot = await withTempRoot();
    const executableRoot = await withTempRoot();
    const executable = path.join(executableRoot, "godot-test.exe");
    await writeFile(executable, "#!/bin/sh\necho fixture\n");
    // The outside sentinel: content-inventory and the probe service must
    // never open it, even when project-controlled paths escape the root.
    const outside = await withTempRoot();
    const sentinel = path.join(outside, "secret.gd");
    await writeFile(sentinel, "extends Node\n# SECRET\n");
    await writeFiles(workspaceRoot, {
      "project.godot":
        '[application]\nconfig/name="Fixture"\nconfig/features=PackedStringArray("4.7")\n',
      "addons/evil/plugin.cfg": '[plugin]\nname="Evil"\nscript="../../../secret.gd"\n',
      "lib.gdextension":
        '[configuration]\ncompatibility_minimum="4.7"\n[entry]\nlinux="../../secret.gd"\n',
      "src/main.gd": "extends Node\n",
    });
    const config: UserGodotConfig = {
      activeInstallation: null,
      installations: { "test-install": { path: executable, editionHint: "standard" } },
      discoverOnPath: false,
    };
    const service = createGodotProjectProbeService({
      workspaceRoot,
      config,
      preference: { kind: "installation-id", installationId: "test-install" },
      overrideSource: "cli",
      backend: createScriptedBackend([]).backend,
      probeRunner: createFakeGodotProbeRunner({ helpText: DEFAULT_HELP_TEXT }).runner,
      cache: {
        load: () => Promise.resolve(null),
        store: () => Promise.resolve(),
        count: () => Promise.resolve(0),
      },
      hostPath: null,
      hostPathExt: null,
      platform: process.platform,
      runDirectories: createRunDirectoryProvider({ workspaceRoot, runsRoot }),
      mirror: createProjectMirror(),
      checkpointRoot: null,
      parentEnvironment: {},
    });
    try {
      const prepared = await service.prepare();
      expect(prepared.status).toBe("ready");
      if (prepared.status !== "ready") {
        return;
      }
      // The escaping script cannot be hashed, so the plugin contributes no
      // manifest entry, and the outside sentinel is never read.
      expect(prepared.preview.risks.enabledEditorPlugins).toBe(0);
      expect(prepared.preview.risks.gdextensions).toBeGreaterThanOrEqual(1);
      expect(await readFile(sentinel, "utf8")).toBe("extends Node\n# SECRET\n");
    } finally {
      await rm(workspaceRoot, { recursive: true, force: true }).catch(() => undefined);
      await rm(runsRoot, { recursive: true, force: true }).catch(() => undefined);
      await rm(executableRoot, { recursive: true, force: true }).catch(() => undefined);
    }
  });
});

function createStubGitInspector(
  before: GitStatusResult["changes"],
  after: GitStatusResult["changes"],
): GitInspector {
  let calls = 0;
  const result = (changes: GitStatusResult["changes"]): GitStatusResult => ({
    repository: true,
    branch: {
      head: "main",
      detached: false,
      upstream: null,
      ahead: 0,
      behind: 0,
      unborn: false,
      oid: "abc",
    },
    changes,
    conflicts: [],
    untracked: [],
    truncated: false,
  });
  return {
    inspectRepository(): Promise<GitWorkspaceStatus> {
      return Promise.resolve({
        gitAvailable: true,
        gitVersion: "stub",
        repositoryState: "repository",
        repositoryRoot: "/stub",
      });
    },
    getStatus(): Promise<GitStatusResult> {
      calls += 1;
      return Promise.resolve(calls === 1 ? result(before) : result(after));
    },
    getDiff(): Promise<never> {
      return Promise.reject(new Error("Not used."));
    },
  };
}
