import { describe, expect, it } from "vitest";
import type {
  GodotCommandCapabilities,
  GodotEngineProfile,
  GodotInstallation,
  GodotVersion,
} from "@solaris/core";
import { invalidInstallation } from "../discovery/path-discovery.js";
import {
  createGodotLSPServerRunner,
  GODOT_LSP_BASE_ARGUMENTS,
  GODOT_LSP_MIRROR_PATH_MARKER,
  GODOT_LSP_PORT_MARKER,
  godotLSPArguments,
  godotLSPArgumentTemplate,
  type GodotLSPServerStartRequest,
} from "./godot-lsp-runner.js";

const DEFAULT_CAPABILITIES: GodotCommandCapabilities = {
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
  dap: false,
  debugServer: false,
  buildSolutions: false,
  extensionApiDump: true,
  extensionApiWithDocsDump: true,
  extensionApiValidation: false,
  docTool: true,
  movieWriting: false,
};

function version(): GodotVersion {
  return {
    raw: "4.7.1.stable.official",
    major: 4,
    minor: 7,
    patch: 1,
    status: "stable",
    statusNumber: null,
    build: "official",
    commit: null,
  };
}

function installation(): GodotInstallation {
  return {
    id: "test-install",
    sourceLabel: "user-config",
    source: "user-config",
    canonicalPath: "/opt/godot/godot",
    sizeBytes: 1024,
    modifiedAtMs: 0,
    sha256: "a".repeat(64),
    editionHint: "standard",
    status: "valid",
  };
}

function profile(
  capabilities: GodotCommandCapabilities = DEFAULT_CAPABILITIES,
): GodotEngineProfile {
  return {
    installationId: "test-install",
    fingerprint: "a".repeat(16),
    version: version(),
    edition: "standard",
    editionConfidence: "high",
    releaseChannel: "stable",
    capabilities,
    verifiedCapabilities: [],
    degradedCapabilities: [],
    executableSha256: "a".repeat(64),
    apiDumpSha256: null,
    support: "compatible-untested",
    diagnostics: [],
  };
}

function request(overrides: Partial<GodotLSPServerStartRequest> = {}): GodotLSPServerStartRequest {
  return {
    installation: installation(),
    engineProfile: profile(),
    mirrorProjectPath: "/tmp/solaris-mirror-1",
    allocatedPort: 65000,
    ...overrides,
  };
}

describe("godotLSPArguments", () => {
  it("is exactly the fixed headless recovery editor tuple with markers", () => {
    expect(GODOT_LSP_BASE_ARGUMENTS).toEqual([
      "--headless",
      "--editor",
      "--recovery-mode",
      "--path",
      GODOT_LSP_MIRROR_PATH_MARKER,
      "--lsp-port",
      GODOT_LSP_PORT_MARKER,
    ]);
    expect(godotLSPArgumentTemplate()).toEqual(GODOT_LSP_BASE_ARGUMENTS);
  });

  it("never contains scene, script, import, DAP, debug, or export options", () => {
    const text = JSON.stringify(GODOT_LSP_BASE_ARGUMENTS);
    for (const forbidden of [
      "--scene",
      "--script",
      "--import",
      "--dap-port",
      "--debug-server",
      "--build-solutions",
      "--quit",
      "--doctool",
    ]) {
      expect(text).not.toContain(forbidden);
    }
  });

  it("resolves the mirror path and allocated port only from Solaris-owned values", () => {
    expect(godotLSPArguments("/mirror/project", 6005)).toEqual([
      "--headless",
      "--editor",
      "--recovery-mode",
      "--path",
      "/mirror/project",
      "--lsp-port",
      "6005",
    ]);
  });
});

describe("createGodotLSPServerRunner", () => {
  it("is never available on this platform", async () => {
    const runner = createGodotLSPServerRunner({ backend: {} });
    expect(await runner.isAvailable()).toBe(false);
  });

  it("refuses with a typed unavailable outcome and never spawns", async () => {
    const runner = createGodotLSPServerRunner({ backend: {} });
    const outcome = await runner.startServer(request());
    expect(outcome.status).toBe("unavailable");
    if (outcome.status === "unavailable") {
      expect(outcome.message).toContain("unavailable");
      expect(outcome.message).toContain("never spawned");
    }
  });

  it("refuses as unsupported when --lsp-port is not advertised", async () => {
    const runner = createGodotLSPServerRunner({ backend: {} });
    const outcome = await runner.startServer(
      request({ engineProfile: profile({ ...DEFAULT_CAPABILITIES, lsp: false }) }),
    );
    expect(outcome.status).toBe("unsupported");
  });

  it("refuses as unsupported without the recovery pairing", async () => {
    const runner = createGodotLSPServerRunner({ backend: {} });
    const outcome = await runner.startServer(
      request({ engineProfile: profile({ ...DEFAULT_CAPABILITIES, recoveryMode: false }) }),
    );
    expect(outcome.status).toBe("unsupported");
  });

  it("refuses for an invalid installation", async () => {
    const runner = createGodotLSPServerRunner({ backend: {} });
    const outcome = await runner.startServer(
      request({ installation: invalidInstallation("test-install", "path", "PATH", "missing") }),
    );
    expect(outcome.status).toBe("unsupported");
  });

  it("propagates an aborted signal as an abort error", async () => {
    const runner = createGodotLSPServerRunner({ backend: {} });
    const controller = new AbortController();
    controller.abort();
    await expect(runner.startServer(request({ signal: controller.signal }))).rejects.toMatchObject({
      name: "AbortError",
    });
  });
});
