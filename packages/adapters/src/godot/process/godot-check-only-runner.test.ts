import { describe, expect, it } from "vitest";
import type {
  GodotCommandCapabilities,
  GodotEngineProfile,
  GodotInstallation,
  GodotVersion,
} from "@siralos/core";
import { invalidInstallation } from "../discovery/path-discovery.js";
import {
  createGodotCheckOnlyRunner,
  GODOT_CHECK_ONLY_BASE_ARGUMENTS,
  GODOT_CHECK_ONLY_MIRROR_PATH_MARKER,
  GODOT_CHECK_ONLY_MIRROR_SCRIPT_MARKER,
  godotCheckOnlyArguments,
  godotCheckOnlyArgumentTemplate,
  type GodotCheckOnlyRunRequest,
} from "./godot-check-only-runner.js";

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
  lsp: false,
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

function request(overrides: Partial<GodotCheckOnlyRunRequest> = {}): GodotCheckOnlyRunRequest {
  return {
    installation: installation(),
    engineProfile: profile(),
    mirrorProjectPath: "/tmp/siralos-mirror-1",
    mirrorScriptPath: "/tmp/siralos-mirror-1/src/player/player.gd",
    ...overrides,
  };
}

describe("godotCheckOnlyArguments", () => {
  it("is exactly the fixed headless mirror check-only shape", () => {
    expect(GODOT_CHECK_ONLY_BASE_ARGUMENTS).toEqual([
      "--headless",
      "--path",
      GODOT_CHECK_ONLY_MIRROR_PATH_MARKER,
      "--script",
      GODOT_CHECK_ONLY_MIRROR_SCRIPT_MARKER,
      "--check-only",
    ]);
    expect(godotCheckOnlyArgumentTemplate()).toEqual(GODOT_CHECK_ONLY_BASE_ARGUMENTS);
  });

  it("never contains a scene, editor, import, LSP, or DAP option", () => {
    const argumentsText = JSON.stringify(GODOT_CHECK_ONLY_BASE_ARGUMENTS);
    for (const forbidden of [
      "--scene",
      "--editor",
      "--import",
      "--lsp",
      "--dap",
      "--recovery-mode",
    ]) {
      expect(argumentsText).not.toContain(forbidden);
    }
  });

  it("resolves the mirror project and script paths only from the prepared mirror", () => {
    const argumentsList = godotCheckOnlyArguments("/mirror/project", "/mirror/project/a.gd");
    expect(argumentsList).toEqual([
      "--headless",
      "--path",
      "/mirror/project",
      "--script",
      "/mirror/project/a.gd",
      "--check-only",
    ]);
  });
});

describe("createGodotCheckOnlyRunner", () => {
  it("is never available on this platform", async () => {
    const runner = createGodotCheckOnlyRunner({ backend: {} });
    expect(await runner.isAvailable()).toBe(false);
  });

  it("refuses with a typed unavailable outcome and never spawns", async () => {
    const runner = createGodotCheckOnlyRunner({ backend: {} });
    const outcome = await runner.runCheckOnly(request());
    expect(outcome.status).toBe("unavailable");
    if (outcome.status === "unavailable") {
      expect(outcome.message).toContain("unavailable");
      expect(outcome.message).toContain("never spawned");
    }
  });

  it("refuses as unsupported when --check-only is not advertised", async () => {
    const runner = createGodotCheckOnlyRunner({ backend: {} });
    const outcome = await runner.runCheckOnly(
      request({ engineProfile: profile({ ...DEFAULT_CAPABILITIES, checkOnly: false }) }),
    );
    expect(outcome.status).toBe("unsupported");
    if (outcome.status === "unsupported") {
      expect(outcome.message).toContain("--check-only");
      expect(outcome.message).toContain("never run normally");
    }
  });

  it("refuses as unsupported when --script or --headless is missing", async () => {
    const runner = createGodotCheckOnlyRunner({ backend: {} });
    const outcome = await runner.runCheckOnly(
      request({ engineProfile: profile({ ...DEFAULT_CAPABILITIES, script: false }) }),
    );
    expect(outcome.status).toBe("unsupported");
  });

  it("refuses as unsupported for a runtime-only edition", async () => {
    const runner = createGodotCheckOnlyRunner({ backend: {} });
    const outcome = await runner.runCheckOnly(
      request({ engineProfile: profile({ ...DEFAULT_CAPABILITIES, checkOnly: false }) }),
    );
    expect(outcome.status).toBe("unsupported");
  });

  it("refuses for an invalid installation", async () => {
    const runner = createGodotCheckOnlyRunner({ backend: {} });
    const outcome = await runner.runCheckOnly(
      request({ installation: invalidInstallation("test-install", "path", "PATH", "missing") }),
    );
    expect(outcome.status).toBe("unsupported");
  });

  it("propagates an aborted signal as an abort error", async () => {
    const runner = createGodotCheckOnlyRunner({ backend: {} });
    const controller = new AbortController();
    controller.abort();
    await expect(runner.runCheckOnly(request({ signal: controller.signal }))).rejects.toMatchObject(
      { name: "AbortError" },
    );
  });
});
