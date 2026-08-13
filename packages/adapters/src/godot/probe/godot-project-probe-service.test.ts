import { afterEach, describe, expect, it } from "vitest";
import {
  chmod,
  mkdtemp,
  mkdir,
  rm,
  writeFile,
  readdir,
  readFile,
  stat,
  utimes,
  link,
  symlink,
} from "node:fs/promises";
import { mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import type {
  GitStatusResult,
  GitWorkspaceStatus,
  GodotProjectProbe,
  GitInspector,
  SandboxBackend,
  SandboxBackendStatus,
  SandboxedProcessResult,
} from "@siralos/core";
import { createFakeGodotProbeRunner } from "../testing/fake-godot-probe-runner.js";
import { createProjectMirror } from "../mirror/project-mirror.js";
import { createRunDirectoryProvider } from "../../process/run-directories.js";
import {
  createGodotProjectProbeService,
  GODOT_RECOVERY_EXECUTION_UNAVAILABLE_MESSAGE,
} from "./godot-project-probe-service.js";
import { type UserGodotConfig } from "../../config/user-config.js";

const tempRoots: string[] = [];

async function withTempRoot(): Promise<string> {
  const root = await mkdtemp(path.join(tmpdir(), "siralos-probe-test-"));
  tempRoots.push(root);
  return root;
}

function probeSymlinkSupport(): boolean {
  let supported = false;
  let probeDir: string | undefined;
  try {
    probeDir = mkdtempSync(path.join(tmpdir(), "siralos-probe-symlink-"));
    const target = path.join(probeDir, "target.txt");
    writeFileSync(target, "x");
    symlinkSync(target, path.join(probeDir, "link.txt"));
    supported = true;
  } catch {
    supported = false;
  } finally {
    if (probeDir !== undefined) {
      rmSync(probeDir, { recursive: true, force: true });
    }
  }
  return supported;
}

const SYMLINKS_SUPPORTED = probeSymlinkSupport();

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

function createScriptedBackend(status: SandboxBackendStatus = availableStatus()): {
  backend: SandboxBackend;
  requests: () => SandboxedProcessResult[];
  executes: () => number;
} {
  let executeCount = 0;
  return {
    backend: {
      id: "scripted-backend",
      inspect(): Promise<SandboxBackendStatus> {
        return Promise.resolve(status);
      },
      execute(): Promise<SandboxedProcessResult> {
        executeCount += 1;
        return Promise.resolve({
          status: "completed",
          exitCode: 0,
          signal: null,
          stdout: "",
          stderr: "",
          stdoutTruncated: false,
          stderrTruncated: false,
          durationMs: 1,
          violations: [],
        });
      },
      close(): Promise<void> {
        return Promise.resolve();
      },
    },
    requests: () => [],
    executes: () => executeCount,
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
  readonly helpText?: string;
  readonly noProject?: boolean;
  readonly probeRunnerAvailable?: boolean;
  readonly versionText?: string;
  readonly backendStatus?: SandboxBackendStatus;
  readonly preference?: import("@siralos/core").GodotSelectionPreference;
  readonly preparedStoreConfig?: {
    readonly maxProbes?: number;
    readonly maxStateBytes?: number;
    readonly ttlMs?: number;
    readonly now?: () => number;
  };
  readonly git?: GitInspector;
}

interface ProbeHarness {
  readonly service: GodotProjectProbe;
  readonly workspaceRoot: string;
  readonly runsRoot: string;
  readonly executable: string;
  readonly backendExecutes: () => number;
}

async function createHarness(options: ProbeHarnessOptions = {}): Promise<ProbeHarness> {
  const workspaceRoot = await withTempRoot();
  const runsRoot = await withTempRoot();
  const executableRoot = await withTempRoot();
  const executable = path.join(executableRoot, "godot-test.exe");
  await writeFile(executable, "#!/bin/sh\necho fixture\n");
  if (process.platform !== "win32") {
    await chmod(executable, 0o755);
  }
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
  const scripted = createScriptedBackend(options.backendStatus);
  const config: UserGodotConfig = {
    activeInstallation: null,
    installations: {
      "test-install": { path: executable, editionHint: "standard" },
    },
    discoverOnPath: false,
  };
  const cache: import("../cache/engine-profile-cache.js").GodotEngineProfileCache = {
    load: (): Promise<null> => Promise.resolve(null),
    store: (): Promise<import("../cache/engine-profile-cache.js").EngineProfileStoreOutcome> =>
      Promise.resolve({
        ok: false,
        reason: "unavailable",
        message: "unavailable",
      }),
    count: (): Promise<0> => Promise.resolve(0),
  };
  const fakeProbe = createFakeGodotProbeRunner({
    helpText: options.helpText ?? DEFAULT_HELP_TEXT,
    ...(options.probeRunnerAvailable === undefined
      ? {}
      : { available: options.probeRunnerAvailable }),
    ...(options.versionText === undefined ? {} : { versionText: options.versionText }),
  });
  const service = createGodotProjectProbeService({
    workspaceRoot,
    config,
    preference: options.preference ?? { kind: "installation-id", installationId: "test-install" },
    overrideSource: "cli",
    backend: scripted.backend,
    probeRunner: fakeProbe.runner,
    cache,
    hostPath: null,
    hostPathExt: null,
    platform: process.platform,
    runDirectories: createRunDirectoryProvider({ workspaceRoot, runsRoot }),
    mirror: createProjectMirror(),
    checkpointRoot: null,
    ...(options.git === undefined ? {} : { git: options.git }),
    parentEnvironment: { PATH: "/usr/bin" },
    ...(options.preparedStoreConfig === undefined
      ? {}
      : { preparedStoreConfig: options.preparedStoreConfig }),
  });
  return {
    service,
    workspaceRoot,
    runsRoot,
    executable,
    backendExecutes: scripted.executes,
  };
}

async function prepareReady(service: GodotProjectProbe): Promise<{
  readonly probe: import("@siralos/core").PreparedGodotProbe;
  readonly digest: string;
  readonly preview: import("@siralos/core").GodotProbePreview;
}> {
  const prepared = await service.prepare();
  if (prepared.status !== "ready") {
    throw new Error(`Expected a ready preparation, got ${prepared.status}: ${prepared.message}`);
  }
  return { probe: prepared.probe, digest: prepared.digest, preview: prepared.preview };
}

async function assertNoRuns(runsRoot: string): Promise<void> {
  const entries = await readdir(runsRoot).catch(() => []);
  for (const entry of entries) {
    const sub = await readdir(path.join(runsRoot, entry)).catch(() => []);
    expect(sub).toEqual([]);
  }
}

function gitInspector(gitStatus: GitStatusResult): GitInspector {
  const workspace: GitWorkspaceStatus = {
    gitAvailable: true,
    gitVersion: "2.40.0",
    repositoryState: "repository",
    repositoryRoot: null,
  };
  return {
    workspaceStatus(): Promise<GitWorkspaceStatus> {
      return Promise.resolve(workspace);
    },
    getStatus(): Promise<GitStatusResult> {
      return Promise.resolve(gitStatus);
    },
    getDiff(): Promise<never> {
      return Promise.reject(new Error("not used"));
    },
  } as unknown as GitInspector;
}

function cleanGitStatus(): GitStatusResult {
  return {
    repository: true,
    branch: {
      head: "main",
      detached: false,
      unborn: false,
      oid: "abc",
      upstream: null,
      ahead: 0,
      behind: 0,
    },
    changes: [],
    conflicts: [],
    untracked: [],
    truncated: false,
  };
}

afterEach(async () => {
  await Promise.all(
    tempRoots.map((root) => rm(root, { recursive: true, force: true }).catch(() => undefined)),
  );
  tempRoots.length = 0;
});

describe("probe support state", () => {
  it("reports unavailable on this platform with a precise reason", async () => {
    const { service, backendExecutes } = await createHarness();
    const support = await service.support();
    expect(support.state).toBe("unavailable");
    expect(support.reason).toContain("exec-by-handle");
    expect(support.platform).toBe(process.platform);
    expect(backendExecutes()).toBe(0);
  });
});

describe("preparation", () => {
  it("prepares a ready probe with a sanitized preview and bound digest", async () => {
    const { service, workspaceRoot } = await createHarness({
      workspaceFiles: {
        "tools/tool.gd": "@tool\nextends Node\n",
      },
    });
    const { preview, digest } = await prepareReady(service);
    expect(preview.engineVersion).toBe("4.7.1.stable.official");
    expect(preview.isolation.recoveryMode).toBe(true);
    expect(preview.isolation.sourceWorkspace).toBe("not-used-as-project");
    expect(preview.risks.toolScripts).toBe(1);
    expect(preview.mirror.estimatedFileCount).toBeGreaterThan(0);
    expect(digest).toMatch(/^[0-9a-f]{64}$/);
    const rendered = JSON.stringify(preview);
    expect(rendered).not.toContain(workspaceRoot);
    expect(rendered).not.toContain("C:\\");
    expect(rendered).not.toContain("/Users/");
  });

  it("reports unsupported without a project", async () => {
    const { service } = await createHarness({ noProject: true });
    const prepared = await service.prepare();
    expect(prepared.status).toBe("failed");
    if (prepared.status === "failed") {
      expect(prepared.message).toContain("project.godot");
    }
  });

  it("reports unsupported when no engine is selected", async () => {
    const { service } = await createHarness({
      probeRunnerAvailable: false,
      preference: { kind: "auto" },
    });
    const prepared = await service.prepare();
    expect(prepared.status).toBe("unsupported");
  });

  it("reports unsupported for engines that do not advertise recovery mode", async () => {
    const helpText = [
      "--editor  Starts the editor.",
      "--headless  Runs headless.",
      "--path <directory>  Sets the project path.",
    ].join("\n");
    const { service } = await createHarness({ helpText });
    const prepared = await service.prepare();
    expect(prepared.status).toBe("unsupported");
    if (prepared.status === "unsupported") {
      expect(prepared.message).toContain("--recovery-mode");
    }
  });

  it("refuses a ninth concurrent prepared probe (count bound)", async () => {
    const { service } = await createHarness({ preparedStoreConfig: { maxProbes: 2 } });
    expect((await service.prepare()).status).toBe("ready");
    expect((await service.prepare()).status).toBe("ready");
    const third = await service.prepare();
    expect(third.status).toBe("failed");
    if (third.status === "failed") {
      expect(third.message).toContain("limit");
    }
  });

  it("throws when cancelled during preparation", async () => {
    const { service } = await createHarness();
    const controller = new AbortController();
    controller.abort();
    await expect(service.prepare(controller.signal)).rejects.toMatchObject({ name: "AbortError" });
  });
});

describe("execution refusal (unavailable platform)", () => {
  it("reports unavailable before creating anything or launching anything", async () => {
    const { service, runsRoot, workspaceRoot, backendExecutes } = await createHarness();
    const { probe, digest } = await prepareReady(service);
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("unavailable");
    expect(result.message).toBe(GODOT_RECOVERY_EXECUTION_UNAVAILABLE_MESSAGE);
    expect(result.recoveryMode).toBe(true);
    expect(result.cleanup.completed).toBe(true);
    expect(result.process.exitCode).toBeNull();
    expect(result.engine.installationId).toBe("test-install");
    expect(backendExecutes()).toBe(0);
    await assertNoRuns(runsRoot);
    expect(await readdir(workspaceRoot)).toContain("project.godot");
  });

  it("reports unavailable without leaking private paths", async () => {
    const { service, runsRoot, workspaceRoot } = await createHarness();
    const { probe, digest } = await prepareReady(service);
    const result = await service.execute(probe, { approvedDigest: digest });
    const rendered = JSON.stringify(result);
    expect(rendered).not.toContain(runsRoot);
    expect(rendered).not.toContain(workspaceRoot);
    expect(rendered).not.toContain("godot-test.exe");
  });

  it("reports unavailable when the sandbox cannot enforce the boundaries", async () => {
    const { service, backendExecutes } = await createHarness({
      backendStatus: {
        backendId: "scripted-backend",
        state: "available",
        platform: "linux",
        version: "0.0.0",
        capabilities: {
          filesystemReadRestriction: true,
          filesystemWriteRestriction: false,
          networkRestriction: true,
          processTreeRestriction: true,
          violationReporting: true,
        },
      },
    });
    const { probe, digest } = await prepareReady(service);
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("unavailable");
    expect(backendExecutes()).toBe(0);
  });
});

describe("approval binding and conflict detection", () => {
  it("rejects a mismatched approval digest", async () => {
    const { service } = await createHarness();
    const { probe } = await prepareReady(service);
    const result = await service.execute(probe, { approvedDigest: "0".repeat(64) });
    expect(result.status).toBe("conflict");
    if (result.status === "conflict") {
      expect(result.message).toContain("does not match");
    }
    expect(service.status().state).toBe("probe-invalidated");
  });

  it("rejects an unknown prepared handle", async () => {
    const { service } = await createHarness();
    const { probe, digest } = await prepareReady(service);
    await service.execute(probe, { approvedDigest: digest });
    const second = await service.execute(probe, { approvedDigest: digest });
    expect(second.status).toBe("failed");
    if (second.status === "failed") {
      expect(second.message).toContain("not valid for this session");
    }
  });

  it("detects a file changed after approval", async () => {
    const { service, workspaceRoot } = await createHarness();
    const { probe, digest } = await prepareReady(service);
    await writeFile(path.join(workspaceRoot, "src", "main.gd"), "extends Node\n# changed\n");
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("conflict");
    if (result.status === "conflict") {
      expect(result.message).toContain("changed after approval");
    }
  });

  it("detects a file added after approval", async () => {
    const { service, workspaceRoot } = await createHarness();
    const { probe, digest } = await prepareReady(service);
    await writeFile(path.join(workspaceRoot, "late.gd"), "extends Node\n");
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("conflict");
  });

  it("detects a file deleted after approval", async () => {
    const { service, workspaceRoot } = await createHarness();
    const { probe, digest } = await prepareReady(service);
    await rm(path.join(workspaceRoot, "src", "main.gd"));
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("conflict");
  });

  it("detects a rename after approval", async () => {
    const { service, workspaceRoot } = await createHarness();
    const { probe, digest } = await prepareReady(service);
    await rm(path.join(workspaceRoot, "src", "main.gd"));
    await writeFile(
      path.join(workspaceRoot, "moved.gd"),
      "extends Node\nfunc _ready():\n    pass\n",
    );
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("conflict");
  });

  it("detects a same-size replacement with a restored timestamp", async () => {
    const { service, workspaceRoot } = await createHarness();
    const { probe, digest } = await prepareReady(service);
    const target = path.join(workspaceRoot, "src", "main.gd");
    const before = await stat(target);
    const original = await readFile(target, "utf8");
    await writeFile(target, original.replace("pass", "fail"));
    const replaced = await stat(target);
    await utimes(target, before.atime, before.mtime);
    // The replacement is the same size and the timestamp is restored to
    // millisecond precision; content hashing must still see the change.
    expect(replaced.size).toBe(before.size);
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("conflict");
  });

  it("detects a tool script changed after approval", async () => {
    const { service, workspaceRoot } = await createHarness({
      workspaceFiles: { "tools/tool.gd": "@tool\nextends Node\n" },
    });
    const { probe, digest } = await prepareReady(service);
    await writeFile(
      path.join(workspaceRoot, "tools", "tool.gd"),
      "@tool\nextends Node\n# hostile\n",
    );
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("conflict");
  });

  it("detects a project.godot change after approval", async () => {
    const { service, workspaceRoot } = await createHarness();
    const { probe, digest } = await prepareReady(service);
    await writeFile(
      path.join(workspaceRoot, "project.godot"),
      '[application]\nconfig/name="Changed"\n',
    );
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("conflict");
  });

  it("detects an engine executable change after approval", async () => {
    const { service, executable } = await createHarness();
    const { probe, digest } = await prepareReady(service);
    await writeFile(executable, "#!/bin/sh\necho replaced\n");
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("conflict");
    if (result.status === "conflict") {
      expect(result.message).toContain("executable changed after approval");
    }
  });

  it("detects an engine disappearance after approval", async () => {
    const { service, executable } = await createHarness();
    const { probe, digest } = await prepareReady(service);
    await rm(executable);
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("conflict");
  });

  it("detects a hard-link substitution after approval", async () => {
    const { service, workspaceRoot } = await createHarness();
    const { probe, digest } = await prepareReady(service);
    const target = path.join(workspaceRoot, "src", "main.gd");
    const substitute = path.join(workspaceRoot, "src", "other.gd");
    await writeFile(substitute, "extends Node\nfunc _ready():\n    pass\n# hard-linked\n");
    await rm(target);
    await link(substitute, target);
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("conflict");
  });

  it("detects a symlinked leaf planted after approval", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const { service, workspaceRoot } = await createHarness();
    const { probe, digest } = await prepareReady(service);
    const outside = await withTempRoot();
    await writeFile(path.join(outside, "secret.gd"), "extends Node # secret\n");
    await rm(path.join(workspaceRoot, "src", "main.gd"));
    await symlink(path.join(outside, "secret.gd"), path.join(workspaceRoot, "src", "main.gd"));
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("conflict");
  });

  it(
    "detects a symlinked intermediate parent planted after approval",
    { skip: !SYMLINKS_SUPPORTED },
    async () => {
      const { service, workspaceRoot } = await createHarness();
      const { probe, digest } = await prepareReady(service);
      const outside = await withTempRoot();
      await writeFile(path.join(outside, "main.gd"), "extends Node # outside\n");
      await rm(path.join(workspaceRoot, "src"), { recursive: true });
      await symlink(outside, path.join(workspaceRoot, "src"));
      const result = await service.execute(probe, { approvedDigest: digest });
      expect(result.status).toBe("conflict");
    },
  );

  it("never reads outside content for an escaping plugin script", async () => {
    const { service } = await createHarness({
      workspaceFiles: {
        "addons/evil/plugin.cfg": [
          "[plugin]",
          'name="Evil"',
          'description="Escapes"',
          'author="x"',
          'version="1.0"',
          'script="../../../../../outside.gd"',
          "",
        ].join("\n"),
        "addons/evil/plugin.gd": "@tool\nextends EditorPlugin\n",
      },
    });
    const prepared = await service.prepare();
    expect(prepared.status).toBe("ready");
    if (prepared.status === "ready") {
      expect(prepared.preview.risks.enabledEditorPlugins).toBe(0);
    }
  });

  it("expires prepared probes after the TTL", async () => {
    let now = 1_000;
    const { service } = await createHarness({
      preparedStoreConfig: { ttlMs: 500, now: () => now },
    });
    const { probe, digest } = await prepareReady(service);
    now = 2_000;
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("failed");
  });

  it("disposes all prepared probes on session shutdown", async () => {
    const { service } = await createHarness();
    const { probe, digest } = await prepareReady(service);
    service.disposeAll();
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("failed");
    if (result.status === "failed") {
      expect(result.message).toContain("not valid for this session");
    }
  });
});

describe("status reporting", () => {
  it("starts untrusted and reports invalidated after a conflict", async () => {
    const { service } = await createHarness();
    expect(service.status().state).toBe("untrusted");
    const { probe } = await prepareReady(service);
    const result = await service.execute(probe, { approvedDigest: "x".repeat(64) });
    expect(result.status).toBe("conflict");
    const status = service.status();
    expect(status.state).toBe("probe-invalidated");
    expect(status.lastResult).toBeNull();
    expect(status.lastManifestDigest).toBeNull();
    expect(status.lastEngineVersion).toBeNull();
  });
});

describe("workspace integrity harness", () => {
  it("accepts a bounded git baseline without changing behavior", async () => {
    const { service, backendExecutes } = await createHarness({
      git: gitInspector(cleanGitStatus()),
    });
    const { probe, digest } = await prepareReady(service);
    const result = await service.execute(probe, { approvedDigest: digest });
    expect(result.status).toBe("unavailable");
    expect(backendExecutes()).toBe(0);
  });
});
