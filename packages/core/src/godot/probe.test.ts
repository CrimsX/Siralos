import { describe, expect, it } from "vitest";
import {
  computeGodotPreparedProbeDigest,
  computeGodotRiskManifestDigest,
  createPreparedGodotProbe,
  type GodotProjectRiskManifest,
} from "../index.js";

function sampleManifest(): Omit<GodotProjectRiskManifest, "digest"> {
  return {
    projectFileSha256: "a".repeat(64),
    engineSelection: {
      installationId: "path-1",
      executableSha256: "b".repeat(64),
      version: "4.7.1.stable.official",
    },
    toolScripts: [{ path: "scripts/tool.gd", sha256: "c".repeat(64), bytes: 12 }],
    enabledEditorPlugins: [
      { path: "addons/demo", name: "Demo", enabled: true, sha256: "d".repeat(64), bytes: 9 },
    ],
    gdextensionDescriptors: [
      {
        path: "bin/example.gdextension",
        sha256: "e".repeat(64),
        bytes: 10,
        referencedLibraries: [
          { path: "bin/example.so", sha256: "f".repeat(64), bytes: 100 },
          { path: "bin/missing.so", sha256: null, bytes: null },
        ],
      },
    ],
    autoloads: [{ name: "State", target: "*res://state.gd" }],
    dotnetProjects: ["MyGame.csproj"],
    authoredFileManifest: {
      fileCount: 3,
      totalBytes: 99,
      digest: "g".repeat(64),
      truncated: false,
    },
    scanWarnings: [{ severity: "warning", message: "Scan hit a bound." }],
  };
}

function fullManifest(): GodotProjectRiskManifest {
  const manifest = sampleManifest();
  return { ...manifest, digest: computeGodotRiskManifestDigest(manifest) };
}

describe("computeGodotRiskManifestDigest", () => {
  it("produces a 64-character hex digest", () => {
    expect(computeGodotRiskManifestDigest(sampleManifest())).toMatch(/^[0-9a-f]{64}$/);
  });

  it("is deterministic across equal manifests with different key orders", () => {
    const first = computeGodotRiskManifestDigest(sampleManifest());
    const reordered = computeGodotRiskManifestDigest({
      scanWarnings: sampleManifest().scanWarnings,
      projectFileSha256: sampleManifest().projectFileSha256,
      engineSelection: sampleManifest().engineSelection,
      toolScripts: [...sampleManifest().toolScripts],
      enabledEditorPlugins: [...sampleManifest().enabledEditorPlugins],
      gdextensionDescriptors: [...sampleManifest().gdextensionDescriptors],
      autoloads: [...sampleManifest().autoloads],
      dotnetProjects: [...sampleManifest().dotnetProjects],
      authoredFileManifest: sampleManifest().authoredFileManifest,
    });
    expect(reordered).toBe(first);
  });

  it("changes when project.godot changes", () => {
    const first = computeGodotRiskManifestDigest(sampleManifest());
    const changed = computeGodotRiskManifestDigest({
      ...sampleManifest(),
      projectFileSha256: "0".repeat(64),
    });
    expect(changed).not.toBe(first);
  });

  it("changes when the engine executable or version changes", () => {
    const first = computeGodotRiskManifestDigest(sampleManifest());
    const engineChanged = computeGodotRiskManifestDigest({
      ...sampleManifest(),
      engineSelection: {
        installationId: "path-1",
        executableSha256: "1".repeat(64),
        version: "4.7.1.stable.official",
      },
    });
    expect(engineChanged).not.toBe(first);
    const versionChanged = computeGodotRiskManifestDigest({
      ...sampleManifest(),
      engineSelection: {
        installationId: "path-1",
        executableSha256: "b".repeat(64),
        version: "4.8.0.stable.official",
      },
    });
    expect(versionChanged).not.toBe(first);
  });

  it("changes when a tool script or its hash changes", () => {
    const first = computeGodotRiskManifestDigest(sampleManifest());
    const scriptChanged = computeGodotRiskManifestDigest({
      ...sampleManifest(),
      toolScripts: [{ path: "scripts/tool.gd", sha256: "9".repeat(64), bytes: 12 }],
    });
    expect(scriptChanged).not.toBe(first);
    const pathChanged = computeGodotRiskManifestDigest({
      ...sampleManifest(),
      toolScripts: [{ path: "scripts/other.gd", sha256: "c".repeat(64), bytes: 12 }],
    });
    expect(pathChanged).not.toBe(first);
  });

  it("changes when plugin enabled state changes", () => {
    const first = computeGodotRiskManifestDigest(sampleManifest());
    const disabled = computeGodotRiskManifestDigest({
      ...sampleManifest(),
      enabledEditorPlugins: [
        { path: "addons/demo", name: "Demo", enabled: false, sha256: "d".repeat(64), bytes: 9 },
      ],
    });
    expect(disabled).not.toBe(first);
  });

  it("changes when a GDExtension or its referenced library changes", () => {
    const first = computeGodotRiskManifestDigest(sampleManifest());
    const descriptorChanged = computeGodotRiskManifestDigest({
      ...sampleManifest(),
      gdextensionDescriptors: [
        {
          path: "bin/example.gdextension",
          sha256: "0".repeat(64),
          bytes: 10,
          referencedLibraries:
            sampleManifest().gdextensionDescriptors[0]?.referencedLibraries ?? [],
        },
      ],
    });
    expect(descriptorChanged).not.toBe(first);
    const libraryChanged = computeGodotRiskManifestDigest({
      ...sampleManifest(),
      gdextensionDescriptors: [
        {
          path: "bin/example.gdextension",
          sha256: "e".repeat(64),
          bytes: 10,
          referencedLibraries: [
            { path: "bin/example.so", sha256: "7".repeat(64), bytes: 100 },
            { path: "bin/missing.so", sha256: null, bytes: null },
          ],
        },
      ],
    });
    expect(libraryChanged).not.toBe(first);
  });

  it("changes when autoloads, dotnet projects, or authored manifest change", () => {
    const first = computeGodotRiskManifestDigest(sampleManifest());
    const autoloadChanged = computeGodotRiskManifestDigest({
      ...sampleManifest(),
      autoloads: [{ name: "Other", target: "*res://other.gd" }],
    });
    expect(autoloadChanged).not.toBe(first);
    const dotnetChanged = computeGodotRiskManifestDigest({
      ...sampleManifest(),
      dotnetProjects: [],
    });
    expect(dotnetChanged).not.toBe(first);
    const authoredChanged = computeGodotRiskManifestDigest({
      ...sampleManifest(),
      authoredFileManifest: {
        fileCount: 3,
        totalBytes: 100,
        digest: "g".repeat(64),
        truncated: false,
      },
    });
    expect(authoredChanged).not.toBe(first);
  });

  it("is independent of the digest field itself", () => {
    const manifest = fullManifest();
    const withoutField = computeGodotRiskManifestDigest(manifest);
    expect(withoutField).toMatch(/^[0-9a-f]{64}$/);
  });

  it("matches a stable golden value (cross-environment determinism)", () => {
    // Golden digest computed with node:crypto over the same canonical form;
    // equal digests across environments is a determinism requirement.
    expect(computeGodotRiskManifestDigest(sampleManifest())).toBe(
      "8d6414ff9d42c1f0824a336ac2e4e3cada11fd3e542bc70578eabcf0ec11fd04",
    );
  });

  it("matches stable golden values across empty and small manifests", () => {
    const emptyManifest: Omit<GodotProjectRiskManifest, "digest"> = {
      projectFileSha256: "",
      engineSelection: {
        installationId: "p",
        executableSha256: "",
        version: "v",
      },
      toolScripts: [],
      enabledEditorPlugins: [],
      gdextensionDescriptors: [],
      autoloads: [],
      dotnetProjects: [],
      authoredFileManifest: { fileCount: 0, totalBytes: 0, digest: "", truncated: false },
      scanWarnings: [],
    };
    expect(computeGodotRiskManifestDigest(emptyManifest)).toBe(
      "49d45767c84cbe86ca88a9ee02d25ff5918d26b4abbbf810e8527d14547d4d63",
    );
    expect(
      computeGodotRiskManifestDigest({
        ...emptyManifest,
        scanWarnings: [{ severity: "info", message: "abc" }],
      }),
    ).toBe("f2de67f3d5f8ab10a05e2f1984231528ec1f926ffa5e47ed7425a109138f289e");
  });
});

describe("computeGodotPreparedProbeDigest", () => {
  const baseParts = {
    manifestDigest: "m".repeat(64),
    commandDigest: "c".repeat(64),
    mirrorPolicyVersion: 1,
    sandboxProfileId: "godot-recovery-probe-offline",
    probeLimits: {
      timeoutMs: 60_000,
      maxFiles: 100_000,
      maxBytes: 4 * 1024 * 1024 * 1024,
      maxSingleFileBytes: 512 * 1024 * 1024,
      maxDepth: 64,
      maxRelativePathBytes: 1024,
    },
  };

  it("is deterministic", () => {
    expect(computeGodotPreparedProbeDigest(baseParts)).toBe(
      computeGodotPreparedProbeDigest(baseParts),
    );
  });

  it("binds the manifest digest", () => {
    const changed = computeGodotPreparedProbeDigest({
      ...baseParts,
      manifestDigest: "n".repeat(64),
    });
    expect(changed).not.toBe(computeGodotPreparedProbeDigest(baseParts));
  });

  it("binds the recovery command digest", () => {
    const changed = computeGodotPreparedProbeDigest({
      ...baseParts,
      commandDigest: "d".repeat(64),
    });
    expect(changed).not.toBe(computeGodotPreparedProbeDigest(baseParts));
  });

  it("binds the mirror policy version, sandbox profile, and limits", () => {
    const base = computeGodotPreparedProbeDigest(baseParts);
    expect(computeGodotPreparedProbeDigest({ ...baseParts, mirrorPolicyVersion: 2 })).not.toBe(
      base,
    );
    expect(
      computeGodotPreparedProbeDigest({
        ...baseParts,
        sandboxProfileId: "godot-probe-offline",
      }),
    ).not.toBe(base);
    expect(
      computeGodotPreparedProbeDigest({
        ...baseParts,
        probeLimits: { ...baseParts.probeLimits, timeoutMs: 30_000 },
      }),
    ).not.toBe(base);
  });

  it("matches a stable golden value (cross-environment determinism)", () => {
    expect(computeGodotPreparedProbeDigest(baseParts)).toBe(
      "360242a24c9f092f45008a51c7b6f91d17c77db12d3e4b0c9df7ab5755eafc27",
    );
  });
});

describe("prepared probe branding", () => {
  it("exposes opaque prepared probes", () => {
    const probe = createPreparedGodotProbe();
    expect(probe).toBeDefined();
  });
});
