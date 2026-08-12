import { describe, expect, it } from "vitest";
import type {
  GodotCommandCapabilities,
  GodotEngineProfile,
  GodotInstallation,
  GodotVersion,
} from "@siralos/core";
import { invalidInstallation } from "../discovery/path-discovery.js";
import {
  createGodotKnowledgeRunner,
  GODOT_KNOWLEDGE_BASE_ARGUMENTS,
  godotKnowledgeArguments,
  type GodotKnowledgeRunRequest,
} from "./godot-knowledge-runner.js";

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

function request(overrides: Partial<GodotKnowledgeRunRequest> = {}): GodotKnowledgeRunRequest {
  return {
    installation: installation(),
    engineProfile: profile(),
    probeDirectory: "/tmp/siralos-knowledge-probe",
    ...overrides,
  };
}

describe("godotKnowledgeArguments", () => {
  it("is exactly the fixed with-docs probe tuple with no project arguments", () => {
    expect(GODOT_KNOWLEDGE_BASE_ARGUMENTS).toEqual(["--dump-extension-api-with-docs"]);
    expect(godotKnowledgeArguments()).toEqual(["--dump-extension-api-with-docs"]);
  });
});

describe("createGodotKnowledgeRunner", () => {
  it("is never available on this platform", async () => {
    const runner = createGodotKnowledgeRunner({ backend: {} });
    expect(await runner.isAvailable()).toBe(false);
  });

  it("refuses with a typed unavailable outcome and never spawns", async () => {
    const runner = createGodotKnowledgeRunner({ backend: {} });
    const outcome = await runner.generateDocumentation(request());
    expect(outcome.status).toBe("unavailable");
    if (outcome.status === "unavailable") {
      expect(outcome.message).toContain("unavailable");
      expect(outcome.message).toContain("never spawned");
    }
  });

  it("reports unsupported for a runtime-only edition", async () => {
    const runner = createGodotKnowledgeRunner({ backend: {} });
    const outcome = await runner.generateDocumentation(
      request({
        engineProfile: profile({ ...DEFAULT_CAPABILITIES, extensionApiWithDocsDump: false }),
      }),
    );
    expect(outcome.status).toBe("unsupported");
    if (outcome.status === "unsupported") {
      expect(outcome.message).toContain("--dump-extension-api-with-docs");
    }
  });

  it("never substitutes an ordinary --dump-extension-api result", async () => {
    const runner = createGodotKnowledgeRunner({ backend: {} });
    const outcome = await runner.generateDocumentation(
      request({
        engineProfile: profile({ ...DEFAULT_CAPABILITIES, extensionApiWithDocsDump: false }),
      }),
    );
    expect(outcome.status).toBe("unsupported");
    if (outcome.status === "unsupported") {
      expect(outcome.message).toContain("never substituted");
    }
  });

  it("refuses for an invalid installation", async () => {
    const runner = createGodotKnowledgeRunner({ backend: {} });
    const outcome = await runner.generateDocumentation(
      request({
        installation: invalidInstallation("test-install", "path", "PATH", "missing"),
      }),
    );
    expect(outcome.status).toBe("unsupported");
  });

  it("propagates an aborted signal as an abort error", async () => {
    const runner = createGodotKnowledgeRunner({ backend: {} });
    const controller = new AbortController();
    controller.abort();
    await expect(
      runner.generateDocumentation(request({ signal: controller.signal })),
    ).rejects.toMatchObject({ name: "AbortError" });
  });
});
