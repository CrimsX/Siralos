import { describe, expect, it } from "vitest";
import {
  classifyGodotManualChannel,
  computeGodotKnowledgeProfileDigest,
  KNOWLEDGE_SCHEMA_VERSION,
  validateGodotKnowledgeCache,
  type GodotKnowledgeProfileV1,
  type GodotVersion,
} from "../index.js";

function sampleProfile(): GodotKnowledgeProfileV1 {
  return {
    version: 1,
    engine: {
      installationId: "path-1",
      executableSha256: "a".repeat(64),
      godotVersion: "4.7.1.stable.official",
      edition: "standard",
    },
    api: {
      dumpSha256: "b".repeat(64),
      generatedAt: "2025-01-01T00:00:00.000Z",
      classCount: 1000,
      builtinClassCount: 200,
      utilityFunctionCount: 300,
      globalEnumCount: 40,
      globalConstantCount: 5,
    },
    index: {
      schemaVersion: KNOWLEDGE_SCHEMA_VERSION,
      symbolCount: 45_000,
    },
  };
}

describe("GodotKnowledgeProfileV1", () => {
  it("has an immutable version-1 shape bound to the executable fingerprint", () => {
    const profile = sampleProfile();
    expect(profile.version).toBe(1);
    expect(profile.engine.executableSha256).toBe("a".repeat(64));
    expect(profile.api.dumpSha256).toBe("b".repeat(64));
    expect(profile.index.schemaVersion).toBe(KNOWLEDGE_SCHEMA_VERSION);
  });

  it("computes a deterministic profile digest that binds every field", () => {
    const profile = sampleProfile();
    const digest = computeGodotKnowledgeProfileDigest(profile);
    expect(digest).toMatch(/^[0-9a-f]{64}$/);
    expect(computeGodotKnowledgeProfileDigest(profile)).toBe(digest);
    const changed = { ...profile, engine: { ...profile.engine, executableSha256: "c".repeat(64) } };
    expect(computeGodotKnowledgeProfileDigest(changed)).not.toBe(digest);
    const changedCount = {
      ...profile,
      api: { ...profile.api, classCount: profile.api.classCount + 1 },
    };
    expect(computeGodotKnowledgeProfileDigest(changedCount)).not.toBe(digest);
  });
});

describe("validateGodotKnowledgeCache", () => {
  it("accepts a profile whose executable, dump, and schema all match", () => {
    const profile = sampleProfile();
    const validation = validateGodotKnowledgeCache(profile, {
      executableSha256: profile.engine.executableSha256,
      dumpSha256: profile.api.dumpSha256,
      schemaVersion: profile.index.schemaVersion,
    });
    expect(validation).toEqual({ valid: true });
  });

  it("invalidates the cache when the executable fingerprint changes", () => {
    const profile = sampleProfile();
    const validation = validateGodotKnowledgeCache(profile, {
      executableSha256: "z".repeat(64),
      dumpSha256: profile.api.dumpSha256,
      schemaVersion: profile.index.schemaVersion,
    });
    expect(validation).toEqual({ valid: false, reason: "executable-changed" });
  });

  it("invalidates the cache when the API dump hash changes", () => {
    const profile = sampleProfile();
    const validation = validateGodotKnowledgeCache(profile, {
      executableSha256: profile.engine.executableSha256,
      dumpSha256: "z".repeat(64),
      schemaVersion: profile.index.schemaVersion,
    });
    expect(validation).toEqual({ valid: false, reason: "dump-changed" });
  });

  it("invalidates the cache when the schema version changes", () => {
    const profile = sampleProfile();
    const validation = validateGodotKnowledgeCache(profile, {
      executableSha256: profile.engine.executableSha256,
      dumpSha256: profile.api.dumpSha256,
      schemaVersion: profile.index.schemaVersion + 1,
    });
    expect(validation).toEqual({ valid: false, reason: "schema-changed" });
  });

  it("never silently survives a fingerprint change: the first mismatch wins", () => {
    const profile = sampleProfile();
    const validation = validateGodotKnowledgeCache(profile, {
      executableSha256: "z".repeat(64),
      dumpSha256: "y".repeat(64),
      schemaVersion: 99,
    });
    expect(validation).toEqual({ valid: false, reason: "executable-changed" });
  });
});

describe("classifyGodotManualChannel", () => {
  function version(status: GodotVersion["status"]): GodotVersion {
    return {
      raw: `4.7.1.${status === "stable" ? "stable" : status}.official`,
      major: 4,
      minor: 7,
      patch: 1,
      status,
      statusNumber: status === "stable" ? null : 1,
      build: "official",
      commit: null,
    };
  }

  it("maps an exact stable engine to its major.minor channel", () => {
    expect(classifyGodotManualChannel(version("stable"))).toBe("4.7");
  });

  it("maps supported older stable minors to their own channel", () => {
    const older = version("stable");
    expect(classifyGodotManualChannel({ ...older, minor: 6 })).toBe("4.6");
  });

  it("never silently uses latest docs for prerelease or custom engines", () => {
    for (const status of ["rc", "beta", "alpha", "dev", "custom", "unknown"] as const) {
      expect(classifyGodotManualChannel(version(status))).toBe("unverified");
    }
  });
});
