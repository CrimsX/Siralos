import { describe, expect, it } from "vitest";
import {
  createGodotKnowledgeCache,
  KNOWLEDGE_CACHE_UNAVAILABLE_MESSAGE,
} from "./knowledge-cache.js";
import type { GodotKnowledgeBase } from "@solaris/core";

function sampleBase(): GodotKnowledgeBase {
  return {
    profile: {
      version: 1,
      engine: {
        installationId: "test-install",
        executableSha256: "a".repeat(64),
        godotVersion: "4.7.1.stable.official",
        edition: "standard",
      },
      api: {
        dumpSha256: "b".repeat(64),
        generatedAt: "2025-01-01T00:00:00.000Z",
        classCount: 1,
        builtinClassCount: 0,
        utilityFunctionCount: 0,
        globalEnumCount: 0,
        globalConstantCount: 0,
      },
      index: { schemaVersion: 1, symbolCount: 0 },
    },
    index: {
      schemaVersion: 1,
      engineVersion: "4.7.1.stable.official",
      dumpSha256: "b".repeat(64),
      symbols: [],
      dumpBytes: 0,
    },
  };
}

describe("createGodotKnowledgeCache", () => {
  it("is never initialized, read, or written: load is always a miss", async () => {
    const cache = createGodotKnowledgeCache({ rootDirectory: "/does/not/exist" });
    expect(await cache.load("a".repeat(64))).toBeNull();
    expect(await cache.load("any-fingerprint")).toBeNull();
    expect(await cache.count()).toBe(0);
  });

  it("store returns a typed unavailable outcome and stores nothing", async () => {
    const cache = createGodotKnowledgeCache();
    const outcome = await cache.store(sampleBase());
    expect(outcome).toEqual({
      ok: false,
      reason: "unavailable",
      message: KNOWLEDGE_CACHE_UNAVAILABLE_MESSAGE,
    });
    expect(await cache.count()).toBe(0);
  });

  it("never serves cached data: the provider cannot retrieve a profile", async () => {
    const cache = createGodotKnowledgeCache();
    const first = await cache.load("a".repeat(64));
    const second = await cache.load("a".repeat(64));
    expect(first).toBeNull();
    expect(second).toBeNull();
  });
});
