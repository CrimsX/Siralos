import { mkdtemp, readdir, rm, stat, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  createEngineProfileCache,
  ENGINE_PROFILE_CACHE_SCHEMA_VERSION,
  type CachedEngineProfile,
} from "./engine-profile-cache.js";
import { createEmptyGodotCommandCapabilities } from "@siralos/core";

const tempDirectories: string[] = [];

async function withTemp(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "siralos-godot-cache-"));
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

/**
 * The engine-profile cache is an explicitly unavailable component while
 * engine probing is unavailable: it performs ZERO filesystem operations. No
 * cache path is initialized, created, read, written, renamed, or removed, so
 * no Windows canonicalization failure can be misreported as a link and no
 * parent-substitution or entry-substitution attack surface exists.
 */
describe("createEngineProfileCache (unavailable contract)", () => {
  it("load always returns null (a cache miss) without creating the cache directory", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    expect(await cache.load("a".repeat(64))).toBeNull();
    // A missing cache must be a plain miss: no filesystem mutation happened.
    await expect(stat(cacheRoot)).rejects.toThrow();
  });

  it("load never reads an existing cache entry: a valid on-disk entry is still a miss", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    await writeFile(join(root, `${"a".repeat(64)}.json`), JSON.stringify(entry()));
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    expect(await cache.load("a".repeat(64))).toBeNull();
    const entries = await readdir(root);
    expect(entries).toEqual([`${"a".repeat(64)}.json`]);
  });

  it("store returns a typed unavailable result and never creates directories", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    const outcome = await cache.store(entry());
    expect(outcome.ok).toBe(false);
    if (!outcome.ok) {
      expect(outcome.reason).toBe("unavailable");
      expect(outcome.message).toContain("unavailable");
    }
    await expect(stat(cacheRoot)).rejects.toThrow();
  });

  it("store never writes, renames, or overwrites anything", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    await writeFile(join(root, "victim.txt"), "keep me");
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    await cache.store(entry());
    const content = await import("node:fs/promises").then((fs) =>
      fs.readFile(join(root, "victim.txt"), "utf8"),
    );
    expect(content).toBe("keep me");
    expect(await readdir(root)).toEqual(["victim.txt"]);
  });

  it("count always resolves to zero without enumerating anything", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    expect(await cache.count()).toBe(0);
    await expect(stat(cacheRoot)).rejects.toThrow();
  });

  it("never classifies any path as a link: a missing root, a managed ancestor, or a permission-denied canonicalization can never be misreported", async () => {
    // Regression for the managed-Windows misclassification: the previous
    // implementation treated a failed realpath (EPERM/EACCES/UNKNOWN on a
    // managed ancestor such as the user profile root) as proof of a link and
    // refused the cache with "resolves through a link". The unavailable
    // cache performs no canonicalization at all, so no valid path can ever
    // be falsely classified as a link — and the ordinary cases below must
    // resolve as plain misses, never as link rejections.
    const root = await withTemp();
    // 1. A normal missing cache path.
    const missing = createEngineProfileCache({ rootDirectory: join(root, "nested", "cache") });
    expect(await missing.load("b".repeat(64))).toBeNull();
    // 2. An existing directory path (no link involvement).
    await writeFile(join(root, "file.txt"), "x");
    const existing = createEngineProfileCache({ rootDirectory: join(root, "file.txt") });
    expect(await existing.load("c".repeat(64))).toBeNull();
    // 3. A symlinked root is never followed because it is never touched.
    const realRoot = join(root, "real");
    const linkRoot = join(root, "linked");
    try {
      await symlink(realRoot, linkRoot, "dir");
    } catch {
      // symlink unsupported; the remaining assertions still apply
    }
    const linked = createEngineProfileCache({ rootDirectory: linkRoot });
    expect(await linked.load("d".repeat(64))).toBeNull();
    expect(await linked.count()).toBe(0);
  });

  it("cache operations never touch the filesystem at all (root, ancestors, and siblings are untouched)", async () => {
    const root = await withTemp();
    const marker = join(root, "marker.txt");
    await writeFile(marker, "untouched");
    const cache = createEngineProfileCache({ rootDirectory: join(root, "cache") });
    await cache.load("e".repeat(64));
    await cache.store(entry());
    await cache.count();
    // No new directory, no temp file, no entry file, no cleanup artifact.
    expect(await readdir(root)).toEqual(["marker.txt"]);
    const content = await import("node:fs/promises").then((fs) => fs.readFile(marker, "utf8"));
    expect(content).toBe("untouched");
  });
});
