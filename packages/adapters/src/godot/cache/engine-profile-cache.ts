import { randomUUID } from "node:crypto";
import { lstat, mkdir, open, readFile, realpath, rename, rm, readdir } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import type {
  GodotCommandCapabilities,
  GodotEdition,
  GodotEditionConfidence,
  GodotReleaseChannel,
  GodotVersion,
  SafeDiagnostic,
  SolarisGodotSupport,
} from "@solaris/core";

export const ENGINE_PROFILE_CACHE_SCHEMA_VERSION = 1;

/** Bounded cache entries; the oldest entries are evicted beyond this. */
export const ENGINE_PROFILE_CACHE_MAX_ENTRIES = 32;

/** Cached bounded engine-profile data. Never contains credentials, dumps, or project files. */
export interface CachedEngineProfile {
  readonly schemaVersion: number;
  readonly installationId: string;
  readonly executable: {
    readonly canonicalPath: string;
    readonly sizeBytes: number;
    readonly modifiedAtMs: number;
    readonly sha256: string;
  };
  readonly version: GodotVersion;
  readonly edition: GodotEdition;
  readonly editionConfidence: GodotEditionConfidence;
  readonly releaseChannel: GodotReleaseChannel;
  readonly capabilities: GodotCommandCapabilities;
  readonly verifiedCapabilities: readonly string[];
  readonly degradedCapabilities: readonly string[];
  readonly apiDumpSha256: string | null;
  readonly support: SolarisGodotSupport;
  readonly probedAtMs: number;
  readonly diagnostics: readonly SafeDiagnostic[];
}

export interface GodotEngineProfileCache {
  load(executableSha256: string): Promise<CachedEngineProfile | null>;
  store(profile: CachedEngineProfile): Promise<void>;
  count(): Promise<number>;
}

export interface EngineProfileCacheOptions {
  /** Defaults to `~/.solaris/godot/engine-profiles`. */
  readonly rootDirectory?: string;
  readonly maxEntries?: number;
}

/**
 * User-level engine-profile cache beneath `~/.solaris/godot/engine-profiles`.
 *
 * Cache identity is the executable SHA-256; a changed executable invalidates
 * the entry. Writes are atomic (temp file + rename), the root and entries
 * are verified against symlink substitution, entries are bounded with
 * oldest-first eviction, and project files can never populate the cache.
 * No provider tool can delete or modify profiles.
 */
export function createEngineProfileCache(
  options: EngineProfileCacheOptions = {},
): GodotEngineProfileCache {
  const root = options.rootDirectory ?? join(homedir(), ".solaris", "godot", "engine-profiles");
  const maxEntries = options.maxEntries ?? ENGINE_PROFILE_CACHE_MAX_ENTRIES;

  async function ensureVerifiedRoot(): Promise<string> {
    try {
      const rootMetadata = await lstat(root);
      if (rootMetadata.isSymbolicLink()) {
        throw new Error("The engine-profile cache root is a symbolic link; refusing to use it.");
      }
    } catch (error: unknown) {
      if (error instanceof Error && error.message.includes("symbolic link")) {
        throw error;
      }
      if (!isNotFoundError(error)) {
        throw new Error(`The engine-profile cache root is not accessible: ${describeError(error)}`);
      }
      await mkdir(root, { recursive: true });
    }
    let canonical: string;
    try {
      canonical = await realpath(root);
    } catch (error: unknown) {
      throw new Error(`The engine-profile cache root is not accessible: ${describeError(error)}`);
    }
    if (canonical !== root) {
      throw new Error("The engine-profile cache root is a symbolic link; refusing to use it.");
    }
    const rootMetadata = await lstat(canonical);
    if (!rootMetadata.isDirectory()) {
      throw new Error("The engine-profile cache root is not a directory.");
    }
    return canonical;
  }

  async function load(executableSha256: string): Promise<CachedEngineProfile | null> {
    if (!/^[0-9a-f]{64}$/.test(executableSha256)) {
      return null;
    }
    const verifiedRoot = await ensureVerifiedRoot();
    const path = join(verifiedRoot, `${executableSha256}.json`);
    let entryMetadata;
    try {
      entryMetadata = await lstat(path);
    } catch (error: unknown) {
      if (isNotFoundError(error)) {
        return null;
      }
      return null;
    }
    if (entryMetadata.isSymbolicLink() || !entryMetadata.isFile()) {
      return null;
    }
    let content: string;
    try {
      content = await readFile(path, "utf8");
    } catch {
      return null;
    }
    return parseCachedProfile(content);
  }

  async function store(profile: CachedEngineProfile): Promise<void> {
    const verifiedRoot = await ensureVerifiedRoot();
    await evictIfNeeded(verifiedRoot, maxEntries);
    const path = join(verifiedRoot, `${profile.executable.sha256}.json`);
    const tempPath = join(verifiedRoot, `.${randomUUID()}.tmp`);
    const handle = await open(tempPath, "wx", 0o600);
    try {
      await handle.writeFile(JSON.stringify(profile), "utf8");
      await handle.sync();
    } finally {
      await handle.close().catch(() => undefined);
    }
    await rename(tempPath, path);
  }

  async function count(): Promise<number> {
    const verifiedRoot = await ensureVerifiedRoot();
    const entries = await readdir(verifiedRoot);
    return entries.filter((entry) => /^[0-9a-f]{64}\.json$/.test(entry)).length;
  }

  return { load, store, count };
}

async function evictIfNeeded(root: string, maxEntries: number): Promise<void> {
  let entries: string[];
  try {
    entries = await readdir(root);
  } catch {
    return;
  }
  const profiles = entries.filter((entry) => /^[0-9a-f]{64}\.json$/.test(entry));
  if (profiles.length < maxEntries) {
    return;
  }
  const dated: { readonly path: string; readonly probedAtMs: number }[] = [];
  for (const entry of profiles) {
    const path = join(root, entry);
    try {
      const content = await readFile(path, "utf8");
      const parsed = parseCachedProfile(content);
      dated.push({ path, probedAtMs: parsed?.probedAtMs ?? 0 });
    } catch {
      dated.push({ path, probedAtMs: 0 });
    }
  }
  dated.sort((left, right) => left.probedAtMs - right.probedAtMs);
  const surplus = dated.length - (maxEntries - 1);
  for (let index = 0; index < surplus && index < dated.length; index += 1) {
    const entry = dated[index];
    if (entry === undefined) {
      continue;
    }
    try {
      const entryMetadata = await lstat(entry.path);
      if (!entryMetadata.isSymbolicLink() && entryMetadata.isFile()) {
        await rm(entry.path, { force: true });
      }
    } catch {
      // best-effort eviction; never follows links
    }
  }
}

function parseCachedProfile(content: string): CachedEngineProfile | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(content);
  } catch {
    return null;
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return null;
  }
  const record = parsed as Record<string, unknown>;
  if (
    record["schemaVersion"] !== ENGINE_PROFILE_CACHE_SCHEMA_VERSION ||
    typeof record["installationId"] !== "string" ||
    typeof record["version"] !== "object" ||
    record["version"] === null ||
    typeof record["executable"] !== "object" ||
    record["executable"] === null
  ) {
    return null;
  }
  const executable = record["executable"] as Record<string, unknown>;
  if (
    typeof executable["sha256"] !== "string" ||
    !/^[0-9a-f]{64}$/.test(executable["sha256"] as string)
  ) {
    return null;
  }
  const version = record["version"] as Record<string, unknown>;
  const versionStatuses: readonly string[] = [
    "stable",
    "rc",
    "beta",
    "alpha",
    "dev",
    "custom",
    "unknown",
  ];
  if (
    typeof version["major"] !== "number" ||
    typeof version["minor"] !== "number" ||
    (version["patch"] !== null && typeof version["patch"] !== "number") ||
    typeof version["status"] !== "string" ||
    !versionStatuses.includes(version["status"] as string)
  ) {
    return null;
  }
  return record as unknown as CachedEngineProfile;
}

function isNotFoundError(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "an unknown filesystem error occurred";
}
