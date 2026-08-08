import { mkdir, mkdtemp, readdir, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  createEngineProfileCache,
  ENGINE_PROFILE_CACHE_MAX_FILE_BYTES,
  ENGINE_PROFILE_CACHE_SCAN_CAP,
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

async function writeEntry(cacheRoot: string, sha256: string, content: string): Promise<void> {
  await mkdir(cacheRoot, { recursive: true });
  await writeFile(join(cacheRoot, `${sha256}.json`), content);
}

/**
 * Serializes a base entry with selected fields overridden or deleted, for
 * malformed-entry fixtures. The typed entry is first widened so hostile
 * shapes can be injected.
 */
function malformed(
  overrides: Record<string, unknown>,
  deletedFields: readonly string[] = [],
): string {
  const record = { ...entry() } as Record<string, unknown>;
  for (const field of deletedFields) {
    delete record[field];
  }
  for (const [key, value] of Object.entries(overrides)) {
    record[key] = value;
  }
  return JSON.stringify(record);
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
    await writeEntry(cacheRoot, "a".repeat(64), "{ broken json");
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

  it("rejects oversized entries on load", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    await writeEntry(
      cacheRoot,
      "a".repeat(64),
      " ".repeat(ENGINE_PROFILE_CACHE_MAX_FILE_BYTES + 1),
    );
    expect(await cache.load("a".repeat(64))).toBeNull();
  });

  it("rejects entries with malformed arrays", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    await writeEntry(
      cacheRoot,
      "a".repeat(64),
      malformed({ verifiedCapabilities: "not-an-array" }),
    );
    expect(await cache.load("a".repeat(64))).toBeNull();
  });

  it("rejects entries with non-string array members", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    await writeEntry(cacheRoot, "a".repeat(64), malformed({ degradedCapabilities: [42] }));
    expect(await cache.load("a".repeat(64))).toBeNull();
  });

  it("rejects entries with missing fields", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    for (const field of ["probedAtMs", "edition", "support", "capabilities", "diagnostics"]) {
      await writeEntry(cacheRoot, "a".repeat(64), malformed({}, [field]));
      expect(await cache.load("a".repeat(64))).toBeNull();
    }
  });

  it("rejects entries with invalid enums and version shapes", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    await writeEntry(cacheRoot, "a".repeat(64), malformed({ edition: "banana" }));
    expect(await cache.load("a".repeat(64))).toBeNull();
    await writeEntry(cacheRoot, "a".repeat(64), malformed({ support: "totally-verified" }));
    expect(await cache.load("a".repeat(64))).toBeNull();
    await writeEntry(
      cacheRoot,
      "a".repeat(64),
      malformed({ version: { major: "four", minor: 7, patch: 1 } }),
    );
    expect(await cache.load("a".repeat(64))).toBeNull();
  });

  it("rejects entries with malformed capabilities and diagnostics", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    await writeEntry(
      cacheRoot,
      "a".repeat(64),
      malformed({ capabilities: { ...createEmptyGodotCommandCapabilities(), editor: "yes" } }),
    );
    expect(await cache.load("a".repeat(64))).toBeNull();
    await writeEntry(
      cacheRoot,
      "a".repeat(64),
      malformed({ diagnostics: [{ severity: "fatal", message: "boom" }] }),
    );
    expect(await cache.load("a".repeat(64))).toBeNull();
  });

  it("rejects an apiDumpSha256 that is not null or 64-hex", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    await writeEntry(cacheRoot, "a".repeat(64), malformed({ apiDumpSha256: "not-a-hash" }));
    expect(await cache.load("a".repeat(64))).toBeNull();
  });

  it("rejects a symlinked entry on load", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    await cache.store(entry());
    const other = await mkdtemp(join(cacheRoot, "other-"));
    await writeFile(join(other, "real.json"), JSON.stringify(entry()));
    const entryPath = join(cacheRoot, `${"a".repeat(64)}.json`);
    await rm(entryPath);
    try {
      await symlink(join(other, "real.json"), entryPath);
    } catch {
      return;
    }
    expect(await cache.load("a".repeat(64))).toBeNull();
  });

  it("accepts a valid 64-hex apiDumpSha256", async () => {
    const root = await withTemp();
    const cache = createEngineProfileCache({ rootDirectory: join(root, "cache") });
    await cache.store(entry({ apiDumpSha256: "b".repeat(64) }));
    expect(await cache.load("a".repeat(64))).not.toBeNull();
  });

  it("caps directory entries inspected by count", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    await mkdir(cacheRoot, { recursive: true });
    const count = ENGINE_PROFILE_CACHE_SCAN_CAP + 32;
    for (let index = 0; index < count; index += 1) {
      await writeEntry(cacheRoot, index.toString(16).padStart(64, "0"), "{}");
    }
    expect(await cache.count()).toBe(ENGINE_PROFILE_CACHE_SCAN_CAP);
  });

  it("evicts oldest entries when full and removes only real files", async () => {
    const root = await withTemp();
    const cacheRoot = join(root, "cache");
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot, maxEntries: 4 });
    for (let index = 0; index < 6; index += 1) {
      const sha = index.toString(16).padStart(64, "0");
      await cache.store(
        entry({ executable: { ...entry().executable, sha256: sha }, probedAtMs: index }),
      );
    }
    const entries = await readdir(cacheRoot);
    const profileFiles = entries.filter((entry) => /^[0-9a-f]{64}\.json$/.test(entry));
    expect(profileFiles.length).toBeLessThanOrEqual(4);
  });
});
