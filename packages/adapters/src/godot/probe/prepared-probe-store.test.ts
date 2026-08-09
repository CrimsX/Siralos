import { describe, expect, it } from "vitest";
import { createPreparedProbeStore, type PreparedProbePlan } from "./prepared-probe-store.js";
import type { GodotProbePreview, GodotProjectRiskManifest } from "@solaris/core";

function samplePlan(): PreparedProbePlan {
  const preview: GodotProbePreview = {
    projectName: "Demo",
    engineVersion: "4.7.1.stable.official",
    installationId: "path-1",
    engineEdition: "standard",
    support: "verified",
    compatibility: "compatible",
    risks: {
      toolScripts: 1,
      enabledEditorPlugins: 0,
      gdextensions: 0,
      autoloads: 0,
      dotnetProjects: 0,
    },
    mirror: { estimatedFileCount: 3, estimatedBytes: 99 },
    isolation: {
      sourceWorkspace: "not-used-as-project",
      disposableMirror: true,
      recoveryMode: true,
      headless: true,
      network: "denied",
      environment: "minimal",
      stdin: "closed",
    },
    manifestDigest: "m".repeat(64),
  };
  const manifest: GodotProjectRiskManifest = {
    projectFileSha256: "a".repeat(64),
    engineSelection: {
      installationId: "path-1",
      executableSha256: "b".repeat(64),
      version: "4.7.1.stable.official",
    },
    toolScripts: [],
    enabledEditorPlugins: [],
    gdextensionDescriptors: [],
    autoloads: [],
    dotnetProjects: [],
    authoredFileManifest: {
      fileCount: 3,
      totalBytes: 99,
      digest: "d".repeat(64),
      truncated: false,
    },
    scanWarnings: [],
    digest: "e".repeat(64),
  };
  return {
    preview,
    digest: "f".repeat(64),
    manifestDigest: "m".repeat(64),
    manifest,
    selection: {
      installation: {
        id: "path-1",
        sourceLabel: "explicit path",
        source: "cli-path",
        canonicalPath: "C:\\godot\\Godot.exe",
        sizeBytes: 1000,
        modifiedAtMs: 1000,
        sha256: "b".repeat(64),
        editionHint: "unknown",
        status: "valid",
      },
      profile: {
        installationId: "path-1",
        fingerprint: "b".repeat(64).slice(0, 12),
        version: {
          major: 4,
          minor: 7,
          patch: 1,
          status: "stable",
          statusNumber: null,
          build: "official",
          commit: null,
          raw: "4.7.1.stable.official",
        },
        edition: "standard",
        editionConfidence: "high",
        releaseChannel: "stable",
        capabilities: {
          editor: true,
          projectManager: false,
          recoveryMode: true,
          headless: true,
          projectPath: true,
          scene: false,
          script: false,
          checkOnly: false,
          import: false,
          quit: true,
          quitAfter: true,
          lsp: false,
          dap: false,
          debugServer: false,
          buildSolutions: false,
          extensionApiDump: false,
          extensionApiWithDocsDump: false,
          extensionApiValidation: false,
          docTool: false,
          movieWriting: false,
        },
        verifiedCapabilities: [],
        degradedCapabilities: [],
        executableSha256: "b".repeat(64),
        apiDumpSha256: null,
        support: "verified",
        diagnostics: [],
      },
    },
  };
}

describe("createPreparedProbeStore", () => {
  it("returns opaque single-use handles", () => {
    const store = createPreparedProbeStore();
    const result = store.put(samplePlan());
    expect(result.ok).toBe(true);
    if (!result.ok) {
      throw new Error("unreachable");
    }
    const consumed = store.consume(result.probe);
    expect(consumed).not.toBeNull();
    expect(store.consume(result.probe)).toBeNull();
    expect(store.size()).toBe(0);
  });

  it("counts the aggregate serialized byte state", () => {
    const store = createPreparedProbeStore();
    const first = store.put(samplePlan());
    const second = store.put(samplePlan());
    expect(first.ok).toBe(true);
    expect(second.ok).toBe(true);
    if (!first.ok || !second.ok) {
      throw new Error("unreachable");
    }
    expect(store.stateBytes()).toBe(
      (first as { stateBytes: number }).stateBytes + (second as { stateBytes: number }).stateBytes,
    );
    expect(store.size()).toBe(2);
  });

  it("enforces the count limit", () => {
    const store = createPreparedProbeStore({ maxProbes: 2 });
    expect(store.put(samplePlan()).ok).toBe(true);
    expect(store.put(samplePlan()).ok).toBe(true);
    const third = store.put(samplePlan());
    expect(third.ok).toBe(false);
    if (third.ok) {
      throw new Error("unreachable");
    }
    expect(third.reason).toBe("count-limit");
  });

  it("enforces the aggregate byte limit", () => {
    const store = createPreparedProbeStore({ maxStateBytes: 100 });
    const first = store.put(samplePlan());
    expect(first.ok).toBe(false);
    if (first.ok) {
      throw new Error("unreachable");
    }
    expect(first.reason).toBe("byte-limit");
  });

  it("expires prepared probes after the TTL", () => {
    let now = 1_000;
    const store = createPreparedProbeStore({ ttlMs: 500, now: () => now });
    const result = store.put(samplePlan());
    expect(result.ok).toBe(true);
    if (!result.ok) {
      throw new Error("unreachable");
    }
    expect(store.size()).toBe(1);
    now = 2_000;
    expect(store.consume(result.probe)).toBeNull();
    expect(store.size()).toBe(0);
  });

  it("disposes abandoned and denied plans explicitly", () => {
    const store = createPreparedProbeStore();
    const first = store.put(samplePlan());
    const second = store.put(samplePlan());
    expect(first.ok && second.ok).toBe(true);
    if (!first.ok || !second.ok) {
      throw new Error("unreachable");
    }
    expect(store.dispose(first.probe)).toBe(true);
    expect(store.dispose(first.probe)).toBe(false);
    expect(store.size()).toBe(1);
  });

  it("clears everything on disposeAll (session shutdown)", () => {
    const store = createPreparedProbeStore();
    store.put(samplePlan());
    store.put(samplePlan());
    store.disposeAll();
    expect(store.size()).toBe(0);
    expect(store.stateBytes()).toBe(0);
  });

  it("does not let a consumed plan be reused through a stale handle", () => {
    const store = createPreparedProbeStore();
    const result = store.put(samplePlan());
    expect(result.ok).toBe(true);
    if (!result.ok) {
      throw new Error("unreachable");
    }
    store.consume(result.probe);
    expect(store.consume(result.probe)).toBeNull();
    expect(store.dispose(result.probe)).toBe(false);
  });
});
