import { afterEach, describe, expect, it } from "vitest";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { GODOT_LIMITS } from "@siralos/core";
import type {
  GitInspector,
  GodotDiagnostics,
  SandboxBackend,
  SandboxBackendStatus,
} from "@siralos/core";
import { createFakeGodotProbeRunner } from "../testing/fake-godot-probe-runner.js";
import { createProjectMirror } from "../mirror/project-mirror.js";
import { createRunDirectoryProvider } from "../../process/run-directories.js";
import { createGodotDiagnosticsService } from "./godot-diagnostics-service.js";
import type { UserGodotConfig } from "../../config/user-config.js";

const tempRoots: string[] = [];

async function withTempRoot(): Promise<string> {
  const root = await mkdtemp(path.join(tmpdir(), "siralos-check-test-"));
  tempRoots.push(root);
  return root;
}

afterEach(async () => {
  await Promise.all(tempRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

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
  executes: () => number;
} {
  let executeCount = 0;
  return {
    backend: {
      id: "scripted-backend",
      inspect(): Promise<SandboxBackendStatus> {
        return Promise.resolve(status);
      },
      execute(): Promise<never> {
        executeCount += 1;
        throw new Error("the diagnostics service must never execute a process");
      },
      close(): Promise<void> {
        return Promise.resolve();
      },
    },
    executes: () => executeCount,
  };
}

const DEFAULT_HELP_TEXT = [
  "--editor  Starts the editor.",
  "--project-manager  Starts the project manager.",
  "--headless  Runs headless.",
  "--recovery-mode  Runs the editor in recovery mode.",
  "--path <directory>  Sets the project path.",
  "--script <script>  Runs the script.",
  "--check-only  Only check the script, do not run it.",
  "--quit-after <int>  Quit after N iterations.",
  "--quit  Quit after the first iteration.",
].join("\n");

interface HarnessOptions {
  readonly workspaceFiles?: Readonly<Record<string, string>>;
  readonly noProject?: boolean;
  readonly helpText?: string;
  readonly versionText?: string;
  readonly backendStatus?: SandboxBackendStatus;
  readonly git?: GitInspector;
}

interface Harness {
  readonly service: GodotDiagnostics;
  readonly workspaceRoot: string;
  readonly backendExecutes: () => number;
}

async function createHarness(options: HarnessOptions = {}): Promise<Harness> {
  const workspaceRoot = await withTempRoot();
  const runsRoot = await withTempRoot();
  const executableRoot = await withTempRoot();
  const executable = path.join(executableRoot, "godot-test.exe");
  await writeFile(executable, "#!/bin/sh\necho fixture\n");
  if (options.noProject !== true) {
    await writeFiles(workspaceRoot, {
      "project.godot":
        '[application]\nconfig/name="Fixture"\nconfig/features=PackedStringArray("4.7")\n',
      "src/player/player.gd": "extends Node\nfunc _ready():\n    pass\n",
      "src/ui/menu.gd": "extends Control\nfunc _ready():\n    pass\n",
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
    load: () => Promise.resolve(null),
    store: () => Promise.resolve({ ok: false, reason: "unavailable", message: "unavailable" }),
    count: () => Promise.resolve(0),
  };
  const fakeProbe = createFakeGodotProbeRunner({
    helpText: options.helpText ?? DEFAULT_HELP_TEXT,
    ...(options.versionText === undefined ? {} : { versionText: options.versionText }),
  });
  const service = createGodotDiagnosticsService({
    workspaceRoot,
    config,
    preference: { kind: "installation-id", installationId: "test-install" },
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
    parentEnvironment: {},
  });
  return { service, workspaceRoot, backendExecutes: scripted.executes };
}

describe("createGodotDiagnosticsService", () => {
  it("reports support unavailable with an exact reason", async () => {
    const { service } = await createHarness();
    const support = await service.support();
    expect(support.state).toBe("unavailable");
    expect(support.reason).toContain("unavailable");
  });

  it("prepares a single-script check with the exact relative path and a bounded preview", async () => {
    const { service } = await createHarness();
    const prepared = await service.prepare({ paths: ["src/player/player.gd"] });
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    expect(prepared.preview.operation).toBe("parse-only");
    expect(prepared.preview.scripts).toMatchObject({
      count: 1,
      paths: ["src/player/player.gd"],
    });
    expect(prepared.preview.isolation).toMatchObject({
      sourceWorkspace: "not-used-as-project",
      disposableMirror: true,
      checkOnly: true,
      headless: true,
      sceneExecution: "disabled",
      gameExecution: "disabled",
      network: "denied",
    });
    expect(prepared.preview.engineVersion).toBeTruthy();
    expect(prepared.preview.manifestDigest).toMatch(/^[0-9a-f]{64}$/);
    expect(prepared.digest).toMatch(/^[0-9a-f]{64}$/);
    // The preview never contains absolute paths or the workspace root.
    expect(JSON.stringify(prepared.preview)).not.toMatch(/[a-z]:[\\/]/i);
    expect(JSON.stringify(prepared.preview)).not.toContain("/tmp/siralos");
  });

  it("prepares a project-wide check with deterministic enumeration", async () => {
    const { service } = await createHarness();
    const prepared = await service.prepare({});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    expect(prepared.preview.scripts.count).toBe(2);
    expect(prepared.preview.scripts.paths).toBeNull();
  });

  it("supports an explicit bounded paths subset", async () => {
    const { service } = await createHarness();
    const prepared = await service.prepare({ paths: ["src/ui/menu.gd"] });
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    expect(prepared.preview.scripts).toMatchObject({ count: 1, paths: ["src/ui/menu.gd"] });
  });

  it("bounds the explicit paths subset to the immutable limits", async () => {
    const { service } = await createHarness();
    const many = Array.from(
      { length: GODOT_LIMITS.maxGDScriptFilesPerProject + 1 },
      (_, index) => `src/f${index}.gd`,
    );
    const prepared = await service.prepare({ paths: many });
    expect(prepared.status).toBe("failed");
    if (prepared.status === "failed") {
      expect(prepared.message).toContain("bound");
    }
  });

  it("rejects invalid, absolute, traversing, non-gd, and missing script paths", async () => {
    const { service } = await createHarness();
    expect((await service.prepare({ paths: ["../escape.gd"] })).status).toBe("invalid_input");
    expect((await service.prepare({ paths: ["C:\\abs.gd"] })).status).toBe("invalid_input");
    expect((await service.prepare({ paths: ["src/ui/menu.tscn"] })).status).toBe("invalid_input");
    expect((await service.prepare({ paths: ["src/nope.gd"] })).status).toBe("invalid_input");
    expect((await service.prepare({ paths: ["src/player/player.gd", "src/nope.gd"] })).status).toBe(
      "invalid_input",
    );
  });

  it("refuses without a Godot project", async () => {
    const { service } = await createHarness({ noProject: true });
    const prepared = await service.prepare({ paths: ["src/main.gd"] });
    expect(prepared.status).toBe("failed");
    if (prepared.status === "failed") {
      expect(prepared.message).toContain("project.godot");
    }
  });

  it("refuses as unsupported when the engine does not advertise --check-only", async () => {
    const { service } = await createHarness({
      helpText: DEFAULT_HELP_TEXT.replace(
        "--check-only  Only check the script, do not run it.\n",
        "",
      ),
    });
    const prepared = await service.prepare({ paths: ["src/player/player.gd"] });
    expect(prepared.status).toBe("unsupported");
  });

  it("refuses as unsupported without a selected engine", async () => {
    const workspaceRoot = await withTempRoot();
    const runsRoot = await withTempRoot();
    const scripted = createScriptedBackend();
    const config: UserGodotConfig = {
      activeInstallation: null,
      installations: {},
      discoverOnPath: false,
    };
    const fakeProbe = createFakeGodotProbeRunner({ helpText: DEFAULT_HELP_TEXT, available: false });
    const cache: import("../cache/engine-profile-cache.js").GodotEngineProfileCache = {
      load: () => Promise.resolve(null),
      store: () => Promise.resolve({ ok: false, reason: "unavailable", message: "unavailable" }),
      count: () => Promise.resolve(0),
    };
    const service = createGodotDiagnosticsService({
      workspaceRoot,
      config,
      preference: { kind: "auto" },
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
      parentEnvironment: {},
    });
    const prepared = await service.prepare({ paths: ["src/player/player.gd"] });
    expect(prepared.status).toBe("unsupported");
  });

  it("execute requires the approved digest and refuses under any other digest", async () => {
    const { service } = await createHarness();
    const prepared = await service.prepare({ paths: ["src/player/player.gd"] });
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const wrong = await service.execute(prepared.check, {
      approvedDigest: "f".repeat(64),
    });
    expect(wrong.status).toBe("conflict");
    expect(service.status().state).toBe("check-invalidated");
  });

  it("execute returns a typed unavailable result after revalidation and never spawns", async () => {
    const { service, backendExecutes } = await createHarness();
    const prepared = await service.prepare({ paths: ["src/player/player.gd"] });
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(prepared.check, {
      approvedDigest: prepared.digest,
    });
    expect(result.status).toBe("unavailable");
    if (result.status === "unavailable") {
      expect(result.message).toContain("unavailable");
      expect(result.message).toContain("no engine was launched");
    }
    expect(backendExecutes()).toBe(0);
  });

  it("prepared checks are single-use", async () => {
    const { service } = await createHarness();
    const prepared = await service.prepare({ paths: ["src/player/player.gd"] });
    if (prepared.status !== "ready") {
      return;
    }
    const first = await service.execute(prepared.check, { approvedDigest: prepared.digest });
    expect(first.status).toBe("unavailable");
    const second = await service.execute(prepared.check, { approvedDigest: prepared.digest });
    expect(second.status).toBe("failed");
  });

  it("a changed script after preparation conflicts with the approval", async () => {
    const { service, workspaceRoot } = await createHarness();
    const prepared = await service.prepare({ paths: ["src/player/player.gd"] });
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    await writeFile(
      path.join(workspaceRoot, "src", "player", "player.gd"),
      'extends Node\nfunc _ready():\n    print("changed")\n',
    );
    const result = await service.execute(prepared.check, { approvedDigest: prepared.digest });
    expect(result.status).toBe("conflict");
    if (result.status === "conflict") {
      expect(result.message).toContain("changed after approval");
    }
  });

  it("a changed engine or project manifest after preparation conflicts", async () => {
    const { service, workspaceRoot } = await createHarness();
    const prepared = await service.prepare({ paths: ["src/player/player.gd"] });
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    await writeFile(
      path.join(workspaceRoot, "project.godot"),
      '[application]\nconfig/name="Changed"\n',
    );
    const result = await service.execute(prepared.check, { approvedDigest: prepared.digest });
    expect(result.status).toBe("conflict");
  });

  it("execute on an unknown check fails without claiming success", async () => {
    const { service } = await createHarness();
    const prepared = await service.prepare({ paths: ["src/player/player.gd"] });
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(prepared.check, { approvedDigest: prepared.digest });
    expect(result.status).not.toBe("checked");
  });

  it("propagates cancellation as an abort error and leaves the workspace unchanged", async () => {
    const { service, workspaceRoot, backendExecutes } = await createHarness();
    const controller = new AbortController();
    controller.abort();
    await expect(
      service.prepare({ paths: ["src/player/player.gd"] }, controller.signal),
    ).rejects.toMatchObject({
      name: "AbortError",
    });
    expect(backendExecutes()).toBe(0);
    const { readdir } = await import("node:fs/promises");
    const entries = await readdir(workspaceRoot);
    expect(entries.includes(".godot")).toBe(false);
  });

  it("status is untrusted until an approval conflict invalidates it", async () => {
    const { service } = await createHarness();
    expect(service.status().state).toBe("untrusted");
    const prepared = await service.prepare({ paths: ["src/player/player.gd"] });
    if (prepared.status !== "ready") {
      return;
    }
    await service.execute(prepared.check, { approvedDigest: "z".repeat(64) });
    expect(service.status().state).toBe("check-invalidated");
  });

  it("disposeAll clears prepared checks", async () => {
    const { service } = await createHarness();
    const prepared = await service.prepare({ paths: ["src/player/player.gd"] });
    expect(prepared.status).toBe("ready");
    service.disposeAll();
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.execute(prepared.check, { approvedDigest: prepared.digest });
    expect(result.status).toBe("failed");
  });
});
