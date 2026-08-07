import { mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  createEngineProfileCache,
  ENGINE_PROFILE_CACHE_SCHEMA_VERSION,
  type CachedEngineProfile,
} from "./engine-profile-cache.js";
import { createEmptyGodotCommandCapabilities } from "@solaris/core";

const tempDirectories: string[] = [];

async function withTemp(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "solaris-godot-cache-"));
  tempDirectories.push(directory);
  return directory;
}

afterEach(async () => {
  for (const directory of tempDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
});

function entry(overrides: Partial<CachedEngineProfile> = {}): CachedEngineProfile {
  return {
    schemaVersion: ENGINE_PROFILE_CACHE_SCHEMA_VERSION,
    installationId: "primary",
    executable: {
      canonicalPath: "C:\\godot.exe",
      sizeBytes: 100,
      modifiedAtMs: 0,
      sha256: "a".repeat(64),
    },
    version: {
      raw: "4.7.1.stable.official",
      major: 4,
      minor: 7,
      patch: 1,
      status: "stable",
      statusNumber: null,
      build: "official",
      commit: null,
    },
    edition: "standard",
    editionConfidence: "high",
    releaseChannel: "stable",
    capabilities: createEmptyGodotCommandCapabilities(),
    verifiedCapabilities: ["version", "help"],
    degradedCapabilities: [],
    apiDumpSha256: null,
    support: "verified",
    probedAtMs: 1000,
    diagnostics: [],
    ...overrides,
  };
}

describe("createEngineProfileCache", () => {
  it("stores and loads a profile by executable sha256", async () => {
    const root = await withTemp();
    const cache = createEngineProfileCache({ rootDirectory: join(root, "cache") });
    await cache.store(entry());
    const loaded = await cache.load("a".repeat(64));
    expect(loaded?.version.raw).toBe("4.7.1.stable.official");
    expect(loaded?.schemaVersion).toBe(ENGINE_PROFILE_CACHE_SCHEMA_VERSION);
  });

  it("returns null for missing entries", async () => {
    const root = await withTemp();
    const cache = createEngineProfileCache({ rootDirectory: join(root, "cache") });
    expect(await cache.load("b".repeat(64))).toBeNull();
  });

  it("rejects corrupted entries on load", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    await cache.store(entry());
    await writeFile(join(cacheRoot, `${"a".repeat(64)}.json`), "{ broken json");
    expect(await cache.load("a".repeat(64))).toBeNull();
  });

  it("rejects entries with a mismatched schema version", async () => {
    const root = await withTemp();
    const cache = createEngineProfileCache({ rootDirectory: join(root, "cache") });
    await cache.store(entry({ schemaVersion: 999 }));
    expect(await cache.load("a".repeat(64))).toBeNull();
  });

  it("counts bounded entries", async () => {
    const root = await withTemp();
    const cache = createEngineProfileCache({ rootDirectory: join(root, "cache") });
    expect(await cache.count()).toBe(0);
    await cache.store(entry());
    expect(await cache.count()).toBe(1);
  });

  it("rejects a symlinked cache root", async () => {
    const root = await withTemp();
    const realRoot = join(root, "real");
    const linkRoot = join(root, "linked");
    const cache = createEngineProfileCache({ rootDirectory: linkRoot });
    try {
      await symlink(realRoot, linkRoot, "dir");
    } catch {
      return;
    }
    await expect(cache.store(entry())).rejects.toThrow(/symbolic link/);
  });

  it("writes entries atomically", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    await cache.store(entry());
    const content = await readFile(join(cacheRoot, `${"a".repeat(64)}.json`), "utf8");
    expect(JSON.parse(content)).toMatchObject({ installationId: "primary" });
  });
});
