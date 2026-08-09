import { describe, expect, it } from "vitest";
import {
  computeGDScriptPreparedSessionDigest,
  createDefaultPolicy,
  createPreparedGDScriptSession,
  GODOT_LIMITS,
  type GDScriptPreparedSessionDigestParts,
  type GDScriptLSPSessionPreview,
} from "../index.js";

function preview(): GDScriptLSPSessionPreview {
  return {
    projectName: "Fixture",
    engineVersion: "4.7.1.stable.official",
    installationId: "test-install",
    engineEdition: "standard",
    support: "compatible-untested",
    compatibility: "compatible",
    projectIntelligence: {
      gdscriptFiles: 84,
      toolScripts: 6,
      editorPlugins: 2,
      gdextensions: 0,
    },
    session: {
      sourceProject: "disposable mirror",
      godotMode: "headless recovery editor",
      lspNetwork: "loopback only",
      externalNetwork: "denied",
      sourceWrites: "denied",
      providerSecrets: "removed",
      lspMutations: "disabled",
    },
    capabilities: { diagnostics: true, hover: true, completion: true, definition: true },
    manifestDigest: "a".repeat(64),
  };
}

function parts(): GDScriptPreparedSessionDigestParts {
  return {
    manifestDigest: "a".repeat(64),
    executableSha256: "b".repeat(64),
    engineVersion: "4.7.1.stable.official",
    mirrorPolicyVersion: 1,
    capabilities: { diagnostics: true, hover: true, completion: true, definition: true },
    sandboxProfileId: "godot-lsp-local",
    lspPolicyVersion: GODOT_LIMITS.lspPolicyVersion,
    sessionLimits: {
      startupTimeoutMs: GODOT_LIMITS.lspStartupTimeoutMs,
      idleTimeoutMs: GODOT_LIMITS.lspIdleTimeoutMs,
      maxLifetimeMs: GODOT_LIMITS.lspMaxSessionLifetimeMs,
      requestTimeoutMs: GODOT_LIMITS.lspRequestTimeoutMs,
      shutdownTimeoutMs: GODOT_LIMITS.lspShutdownTimeoutMs,
    },
  };
}

describe("GDScript LSP session model", () => {
  it("binds the prepared-session digest to every security-relevant field", () => {
    const digest = computeGDScriptPreparedSessionDigest(parts());
    expect(digest).toMatch(/^[0-9a-f]{64}$/);
    expect(computeGDScriptPreparedSessionDigest(parts())).toBe(digest);
    const variants: GDScriptPreparedSessionDigestParts[] = [
      { ...parts(), manifestDigest: "c".repeat(64) },
      { ...parts(), executableSha256: "d".repeat(64) },
      { ...parts(), engineVersion: "4.6.4.stable.official" },
      { ...parts(), mirrorPolicyVersion: 2 },
      {
        ...parts(),
        capabilities: { diagnostics: false, hover: true, completion: true, definition: true },
      },
      { ...parts(), sandboxProfileId: "validation-offline" },
      { ...parts(), lspPolicyVersion: GODOT_LIMITS.lspPolicyVersion + 1 },
      { ...parts(), sessionLimits: { ...parts().sessionLimits, startupTimeoutMs: 5_000 } },
    ];
    for (const variant of variants) {
      expect(computeGDScriptPreparedSessionDigest(variant)).not.toBe(digest);
    }
  });

  it("keeps the preview immutable and free of absolute paths", () => {
    const value = preview();
    expect(value.session.sourceProject).toBe("disposable mirror");
    expect(value.session.lspMutations).toBe("disabled");
    expect(JSON.stringify(value)).not.toMatch(/[a-z]:[\\/]/i);
  });

  it("creates opaque single-use session handles", () => {
    const first = createPreparedGDScriptSession();
    const second = createPreparedGDScriptSession();
    expect(first).not.toBe(second);
  });

  it("defines bounded LSP limits", () => {
    expect(GODOT_LIMITS.lspMessageBodyBytes).toBe(16 * 1024 * 1024);
    expect(GODOT_LIMITS.lspHeaderBytes).toBe(32 * 1024);
    expect(GODOT_LIMITS.lspMaxPendingRequests).toBe(128);
    expect(GODOT_LIMITS.lspMaxOpenDocuments).toBe(256);
    expect(GODOT_LIMITS.lspMaxDiagnosticsPerDocument).toBe(2_000);
    expect(GODOT_LIMITS.lspMaxCompletionItems).toBe(500);
    expect(GODOT_LIMITS.lspMaxHoverBytes).toBe(512 * 1024);
    expect(GODOT_LIMITS.lspMaxDefinitionLocations).toBe(100);
    expect(GODOT_LIMITS.lspStartupTimeoutMs).toBe(30_000);
    expect(GODOT_LIMITS.lspRequestTimeoutMs).toBe(15_000);
    expect(GODOT_LIMITS.lspIdleTimeoutMs).toBe(10 * 60 * 1000);
    expect(GODOT_LIMITS.lspMaxSessionLifetimeMs).toBe(30 * 60 * 1000);
    expect(GODOT_LIMITS.lspShutdownTimeoutMs).toBe(5_000);
  });
});

describe("godot.lsp capability policy", () => {
  it("is ask in the user-facing Godot profiles and never publicly allow", () => {
    for (const profileId of ["inspect", "develop-offline"] as const) {
      const policy = createDefaultPolicy(profileId);
      expect(policy.rules["godot.lsp"]).toBe("ask");
    }
    expect(createDefaultPolicy("validation-offline").rules["godot.lsp"]).not.toBe("allow");
  });

  it("is deny in the internal execution profiles", () => {
    for (const profileId of [
      "godot-probe-offline",
      "godot-recovery-probe-offline",
      "godot-diagnostics-offline",
      "godot-lsp-local",
    ] as const) {
      const policy = createDefaultPolicy(profileId);
      expect(policy.rules["godot.lsp"]).toBe("deny");
    }
  });
});
