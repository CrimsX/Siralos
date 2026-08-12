import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import type {
  GodotKnowledgeBase,
  GodotKnowledgeProfileV1,
  SandboxBackendStatus,
} from "@siralos/core";
import type { UserGodotConfig } from "../../config/user-config.js";
import type { GodotEngineProfileCache } from "../cache/engine-profile-cache.js";
import {
  createGodotKnowledgeService,
  type GodotKnowledgeServiceDependencies,
} from "./godot-knowledge-service.js";
import {
  GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE,
  type GodotKnowledgeRunner,
} from "../process/godot-knowledge-runner.js";
import { createGodotKnowledgeCache } from "./knowledge-cache.js";
import { parseGodotApiDumpWithDocs } from "./api-dump-with-docs.js";
import { buildGodotApiIndex } from "./api-index.js";

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

function createBackend(executes: { count: () => number }) {
  return {
    id: "scripted-backend",
    inspect(): Promise<SandboxBackendStatus> {
      return Promise.resolve(availableStatus());
    },
    execute(): Promise<never> {
      executes.count();
      throw new Error("the knowledge service must never execute a process");
    },
    close(): Promise<void> {
      return Promise.resolve();
    },
  };
}

function unavailableRunner(): GodotKnowledgeRunner {
  return {
    isAvailable(): Promise<boolean> {
      return Promise.resolve(false);
    },
    generateDocumentation(): Promise<never> {
      throw new Error("must not be called when unavailable");
    },
  };
}

function engineProfileCache(): GodotEngineProfileCache {
  return {
    load: () => Promise.resolve(null),
    store: () => Promise.resolve({ ok: false, reason: "unavailable", message: "unavailable" }),
    count: () => Promise.resolve(0),
  };
}

function config(): UserGodotConfig {
  return {
    activeInstallation: null,
    installations: {},
    discoverOnPath: false,
  };
}

function loadFixtureBase(): GodotKnowledgeBase {
  const fixturePath = path.join(
    path.dirname(fileURLToPath(import.meta.url)),
    "fixtures",
    "extension-api-with-docs.fixture.json",
  );
  const parsed = parseGodotApiDumpWithDocs(readFileSync(fixturePath));
  if (!parsed.ok) {
    throw new Error(parsed.message);
  }
  const built = buildGodotApiIndex(parsed.document);
  if (!built.ok) {
    throw new Error(built.message);
  }
  const profile: GodotKnowledgeProfileV1 = {
    version: 1,
    engine: {
      installationId: "test-install",
      executableSha256: "a".repeat(64),
      godotVersion: "4.7.1.stable.official",
      edition: "standard",
    },
    api: {
      dumpSha256: parsed.document.sha256,
      generatedAt: "2025-01-01T00:00:00.000Z",
      classCount: parsed.document.classes.length,
      builtinClassCount: parsed.document.builtinClasses.length,
      utilityFunctionCount: parsed.document.utilityFunctions.length,
      globalEnumCount: parsed.document.globalEnums.length,
      globalConstantCount: parsed.document.globalConstants.length,
    },
    index: { schemaVersion: 1, symbolCount: built.index.symbols.length },
  };
  return { profile, index: built.index };
}

function createService(overrides: Partial<GodotKnowledgeServiceDependencies> = {}) {
  const executes = { count: () => 0 };
  const dependencies: GodotKnowledgeServiceDependencies = {
    workspaceRoot: "/workspace",
    config: config(),
    preference: { kind: "installation-id", installationId: "test-install" },
    overrideSource: "cli",
    backend: createBackend(executes),
    probeRunner: {
      isAvailable: () => Promise.resolve(false),
      probeVersion: () => Promise.reject(new Error("unused")),
      probeHelp: () => Promise.reject(new Error("unused")),
      dumpExtensionApi: () => Promise.reject(new Error("unused")),
    },
    cache: createGodotKnowledgeCache(),
    engineProfileCache: engineProfileCache(),
    hostPath: null,
    hostPathExt: null,
    platform: "linux",
    parentEnvironment: {},
    ...overrides,
  };
  return { service: createGodotKnowledgeService(dependencies), executes };
}

describe("createGodotKnowledgeService (production posture)", () => {
  it("reports support unavailable with an exact reason", async () => {
    const { service } = createService();
    const support = await service.support();
    expect(support.state).toBe("unavailable");
    expect(support.reason).toContain("unavailable");
    expect(support.platform).toBe("linux");
  });

  it("refresh fails closed as unavailable before any approval or spawn", async () => {
    const { service } = createService({ generationRunner: unavailableRunner() });
    const result = await service.refresh();
    expect(result).toEqual({
      status: "unavailable",
      message: GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE,
    });
  });

  it("status reports the truthful unavailable state with the cache disabled", () => {
    const { service } = createService();
    const status = service.status();
    expect(status.state).toBe("unavailable");
    expect(status.reason).toContain("unavailable");
    expect(status.profile).toBeNull();
    expect(status.cacheEnabled).toBe(false);
    expect(status.schemaVersion).toBe(1);
    expect(status.manualChannel).toBeNull();
  });

  it("search and lookup are unavailable without a loaded knowledge base", async () => {
    const { service } = createService();
    const search = await service.search({ query: "Node owner" });
    expect(search.status).toBe("unavailable");
    const lookup = await service.lookup("class:Node");
    expect(lookup.status).toBe("unavailable");
  });

  it("cancellation returns a typed cancelled result", async () => {
    const { service } = createService();
    const controller = new AbortController();
    controller.abort();
    const search = await service.search({ query: "Node" }, controller.signal);
    expect(search.status).toBe("cancelled");
    const refresh = await service.refresh(controller.signal);
    expect(refresh.status).toBe("cancelled");
  });
});

describe("createGodotKnowledgeService (loaded knowledge base)", () => {
  it("status reports ready with the exact engine profile and manual channel", () => {
    const { service } = createService({ knowledgeBase: loadFixtureBase() });
    const status = service.status();
    expect(status.state).toBe("ready");
    expect(status.profile?.engine.godotVersion).toBe("4.7.1.stable.official");
    expect(status.manualChannel).toBe("4.7");
    expect(status.cacheEnabled).toBe(false);
  });

  it("search returns exact-version bounded results with the engine version", async () => {
    const { service } = createService({ knowledgeBase: loadFixtureBase() });
    const result = await service.search({ query: "Node owner", kinds: ["property"] });
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    expect(result.engineVersion).toBe("4.7.1.stable.official");
    expect(result.results[0]).toMatchObject({
      symbol: "class:Node/property:owner",
      kind: "property",
      name: "owner",
      owner: "Node",
    });
    expect(result.results.length).toBeLessThanOrEqual(25);
  });

  it("lookup returns the bounded structured documentation with the engine version", async () => {
    const { service } = createService({ knowledgeBase: loadFixtureBase() });
    const result = await service.lookup("class:Node/method:add_child");
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    expect(result.engineVersion).toBe("4.7.1.stable.official");
    expect(result.result.signature).toContain("add_child(node: Node");
    expect(result.result.description).toContain("Adds a child node");
    expect(result.result.owner).toBe("Node");
    expect(result.result.inheritedFrom).toBeNull();
  });

  it("lookup returns a structured not-found result for unknown symbols", async () => {
    const { service } = createService({ knowledgeBase: loadFixtureBase() });
    const result = await service.lookup("class:Node/method:nope");
    expect(result.status).toBe("not_found");
  });

  it("rejects invalid queries and unknown kinds", async () => {
    const { service } = createService({ knowledgeBase: loadFixtureBase() });
    expect((await service.search({ query: "   " })).status).toBe("invalid_input");
    expect(
      (
        await service.search({
          query: "Node",
          kinds: ["hologram"],
        } as unknown as import("@siralos/core").GodotApiSearchQuery)
      ).status,
    ).toBe("invalid_input");
    expect((await service.lookup("")).status).toBe("invalid_input");
  });
});
