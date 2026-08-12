import { afterEach, describe, expect, it } from "vitest";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import type { GDScriptLanguageService, SandboxBackend, SandboxBackendStatus } from "@siralos/core";
import { createFakeGodotProbeRunner } from "../testing/fake-godot-probe-runner.js";
import { createProjectMirror } from "../mirror/project-mirror.js";
import { createRunDirectoryProvider } from "../../process/run-directories.js";
import { createGDScriptLanguageService } from "./godot-lsp-service.js";
import type { UserGodotConfig } from "../../config/user-config.js";

const tempRoots: string[] = [];

async function withTempRoot(): Promise<string> {
  const root = await mkdtemp(path.join(tmpdir(), "siralos-lsp-test-"));
  tempRoots.push(root);
  return root;
}

afterEach(async () => {
  await Promise.all(tempRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

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

function createScriptedBackend(): { backend: SandboxBackend; executes: () => number } {
  let executeCount = 0;
  return {
    backend: {
      id: "scripted-backend",
      inspect(): Promise<SandboxBackendStatus> {
        return Promise.resolve(availableStatus());
      },
      execute(): Promise<never> {
        executeCount += 1;
        throw new Error("the language service must never execute a process");
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
  "--lsp-port <port>  Starts the LSP server on the given port.",
  "--check-only  Only check the script, do not run it.",
  "--quit-after <int>  Quit after N iterations.",
  "--quit  Quit after the first iteration.",
].join("\n");

async function createHarness(options: { noProject?: boolean; helpText?: string } = {}): Promise<{
  service: GDScriptLanguageService;
  workspaceRoot: string;
  backendExecutes: () => number;
}> {
  const workspaceRoot = await withTempRoot();
  const runsRoot = await withTempRoot();
  const executableRoot = await withTempRoot();
  const executable = path.join(executableRoot, "godot-test.exe");
  await writeFile(executable, "#!/bin/sh\necho fixture\n");
  if (options.noProject !== true) {
    await mkdir(path.join(workspaceRoot, "src", "player"), { recursive: true });
    await writeFile(
      path.join(workspaceRoot, "project.godot"),
      '[application]\nconfig/name="Fixture"\n',
    );
    await writeFile(path.join(workspaceRoot, "src", "player", "player.gd"), "extends Node\n");
  }
  const scripted = createScriptedBackend();
  const config: UserGodotConfig = {
    activeInstallation: null,
    installations: { "test-install": { path: executable, editionHint: "standard" } },
    discoverOnPath: false,
  };
  const cache: import("../cache/engine-profile-cache.js").GodotEngineProfileCache = {
    load: () => Promise.resolve(null),
    store: () => Promise.resolve({ ok: false, reason: "unavailable", message: "unavailable" }),
    count: () => Promise.resolve(0),
  };
  const fakeProbe = createFakeGodotProbeRunner({
    helpText: options.helpText ?? DEFAULT_HELP_TEXT,
  });
  const service = createGDScriptLanguageService({
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
    parentEnvironment: {},
  });
  return { service, workspaceRoot, backendExecutes: scripted.executes };
}

describe("createGDScriptLanguageService", () => {
  it("reports support unavailable with an exact reason", async () => {
    const { service } = await createHarness();
    const support = await service.support();
    expect(support.state).toBe("unavailable");
    expect(support.reason).toContain("unavailable");
  });

  it("prepares an immutable session plan with the full preview", async () => {
    const { service } = await createHarness();
    const prepared = await service.prepare();
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    expect(prepared.preview.session).toMatchObject({
      sourceProject: "disposable mirror",
      godotMode: "headless recovery editor",
      lspNetwork: "loopback only",
      externalNetwork: "denied",
      sourceWrites: "denied",
      lspMutations: "disabled",
    });
    expect(prepared.preview.capabilities).toEqual({
      diagnostics: true,
      hover: true,
      completion: true,
      definition: true,
    });
    expect(prepared.preview.projectIntelligence.gdscriptFiles).toBe(1);
    expect(prepared.preview.manifestDigest).toMatch(/^[0-9a-f]{64}$/);
    expect(prepared.digest).toMatch(/^[0-9a-f]{64}$/);
    expect(JSON.stringify(prepared.preview)).not.toMatch(/[a-z]:[\\/]/i);
  });

  it("refuses as unsupported when the engine does not advertise LSP", async () => {
    const { service } = await createHarness({
      helpText: DEFAULT_HELP_TEXT.replace(
        "--lsp-port <port>  Starts the LSP server on the given port.\n",
        "",
      ),
    });
    const prepared = await service.prepare();
    expect(prepared.status).toBe("unsupported");
  });

  it("refuses without a Godot project", async () => {
    const { service } = await createHarness({ noProject: true });
    const prepared = await service.prepare();
    expect(prepared.status).toBe("failed");
  });

  it("start requires the approved digest and refuses under any other digest", async () => {
    const { service } = await createHarness();
    const prepared = await service.prepare();
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const wrong = await service.start(prepared.session, { approvedDigest: "f".repeat(64) });
    expect(wrong.status).toBe("conflict");
    expect(service.status().state).toBe("stale");
  });

  it("start returns a typed unavailable result after revalidation and never spawns", async () => {
    const { service, backendExecutes } = await createHarness();
    const prepared = await service.prepare();
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.start(prepared.session, { approvedDigest: prepared.digest });
    expect(result.status).toBe("unavailable");
    if (result.status === "unavailable") {
      expect(result.message).toContain("no engine was launched");
    }
    expect(backendExecutes()).toBe(0);
  });

  it("prepared sessions are single-use", async () => {
    const { service } = await createHarness();
    const prepared = await service.prepare();
    if (prepared.status !== "ready") {
      return;
    }
    const first = await service.start(prepared.session, { approvedDigest: prepared.digest });
    expect(first.status).toBe("unavailable");
    const second = await service.start(prepared.session, { approvedDigest: prepared.digest });
    expect(second.status).toBe("failed");
  });

  it("a changed project after preparation conflicts with the approval", async () => {
    const { service, workspaceRoot } = await createHarness();
    const prepared = await service.prepare();
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    await writeFile(
      path.join(workspaceRoot, "project.godot"),
      '[application]\nconfig/name="Changed"\n',
    );
    const result = await service.start(prepared.session, { approvedDigest: prepared.digest });
    expect(result.status).toBe("conflict");
  });

  it("propagates cancellation and leaves nothing behind", async () => {
    const { service, backendExecutes } = await createHarness();
    const controller = new AbortController();
    controller.abort();
    await expect(service.prepare(controller.signal)).rejects.toMatchObject({
      name: "AbortError",
    });
    expect(backendExecutes()).toBe(0);
  });

  it("status is truthful without an active session", async () => {
    const { service } = await createHarness();
    const status = service.status();
    expect(status.state).toBe("unavailable");
    expect(status.sessionId).toBeNull();
    expect(status.openDocumentCount).toBe(0);
    expect(status.networkIsolation).toBe("unavailable");
  });

  it("closeAll disposes prepared sessions", async () => {
    const { service } = await createHarness();
    const prepared = await service.prepare();
    expect(prepared.status).toBe("ready");
    await service.closeAll();
    if (prepared.status !== "ready") {
      return;
    }
    const result = await service.start(prepared.session, { approvedDigest: prepared.digest });
    expect(result.status).toBe("failed");
  });
});
