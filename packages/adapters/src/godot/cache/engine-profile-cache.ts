import { randomUUID } from "node:crypto";
import { lstat, mkdir, open, readdir, realpath, rename, rm } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import type {
  GodotCommandCapabilities,
  GodotEdition,
  GodotEditionConfidence,
  GodotReleaseChannel,
  GodotVersion,
  GodotVersionStatus,
  SafeDiagnostic,
  SolarisGodotSupport,
} from "@solaris/core";
import { createEmptyGodotCommandCapabilities } from "@solaris/core";

export const ENGINE_PROFILE_CACHE_SCHEMA_VERSION = 1;

/** Bounded cache entries; the oldest entries are evicted beyond this. */
export const ENGINE_PROFILE_CACHE_MAX_ENTRIES = 32;

/** Maximum cached entry file size (1 MiB); larger entries are rejected. */
export const ENGINE_PROFILE_CACHE_MAX_FILE_BYTES = 1024 * 1024;

/** Cap on directory entries inspected by eviction and count. */
export const ENGINE_PROFILE_CACHE_SCAN_CAP = 256;

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
 * the entry (the profiler re-verifies the full hash before serving). Writes
 * are atomic (exclusive temp file + rename), the root and every entry are
 * verified against symlink/junction substitution on all read paths (load,
 * store temp, eviction), entries are bounded in file size and count with
 * oldest-first eviction, cached structures are fully field-validated before
 * use, and project files can never populate the cache. No provider tool can
 * delete or modify profiles.
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
    const content = await readCachedEntryFile(path);
    if (content === null) {
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
    return entries
      .filter((entry) => /^[0-9a-f]{64}\.json$/.test(entry))
      .slice(0, ENGINE_PROFILE_CACHE_SCAN_CAP).length;
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
  const profiles = entries
    .filter((entry) => /^[0-9a-f]{64}\.json$/.test(entry))
    .slice(0, ENGINE_PROFILE_CACHE_SCAN_CAP);
  if (profiles.length < maxEntries) {
    return;
  }
  const dated: { readonly path: string; readonly probedAtMs: number }[] = [];
  for (const entry of profiles) {
    const path = join(root, entry);
    const content = await readCachedEntryFile(path);
    const parsed = content === null ? null : parseCachedProfile(content);
    dated.push({ path, probedAtMs: parsed?.probedAtMs ?? 0 });
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

/**
 * Reads one cache entry with no-follow verification and a byte cap: the
 * entry must be a regular non-link file whose size is within the bounded
 * cache limit. Symlinked/junctioned or oversized entries read as null.
 */
async function readCachedEntryFile(path: string): Promise<string | null> {
  let metadata;
  try {
    metadata = await lstat(path);
  } catch {
    return null;
  }
  if (metadata.isSymbolicLink() || !metadata.isFile()) {
    return null;
  }
  if (metadata.size > ENGINE_PROFILE_CACHE_MAX_FILE_BYTES) {
    return null;
  }
  try {
    const handle = await open(path, "r");
    try {
      const buffer = Buffer.alloc(ENGINE_PROFILE_CACHE_MAX_FILE_BYTES + 1);
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
      if (bytesRead > ENGINE_PROFILE_CACHE_MAX_FILE_BYTES) {
        return null;
      }
      return buffer.subarray(0, bytesRead).toString("utf8");
    } finally {
      await handle.close().catch(() => undefined);
    }
  } catch {
    return null;
  }
}

/**
 * Fully validates every cached field before use: version shape (including
 * the raw string), edition/editionConfidence/releaseChannel/support enums,
 * the capabilities object (every known key boolean), arrays with bounded
 * lengths and bounded strings, `apiDumpSha256` (64-hex or null), finite
 * numbers, and diagnostics of the bounded `{severity, message}` shape.
 * Nothing from the cache file is ever cast without validation; the result
 * is rebuilt as a fresh typed object.
 */
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
  if (record["schemaVersion"] !== ENGINE_PROFILE_CACHE_SCHEMA_VERSION) {
    return null;
  }
  const installationId = asBoundedString(record["installationId"], 512);
  if (installationId === null) {
    return null;
  }
  const executableRecord = asRecord(record["executable"]);
  if (executableRecord === null) {
    return null;
  }
  const canonicalPath = asBoundedString(executableRecord["canonicalPath"], 4096);
  const sizeBytes = asFiniteNumber(executableRecord["sizeBytes"], 0);
  const modifiedAtMs = asFiniteNumber(executableRecord["modifiedAtMs"], -Infinity);
  const sha256 = asSha256(executableRecord["sha256"]);
  if (canonicalPath === null || sizeBytes === null || modifiedAtMs === null || sha256 === null) {
    return null;
  }
  const version = parseVersionField(record["version"]);
  if (version === null) {
    return null;
  }
  const edition = asOneOf(record["edition"], [
    "standard",
    "dotnet",
    "editor-unknown",
    "runtime-only",
    "unknown",
  ]);
  const editionConfidence = asOneOf(record["editionConfidence"], ["high", "medium", "low"]);
  const releaseChannel = asOneOf(record["releaseChannel"], [
    "stable",
    "release-candidate",
    "beta",
    "alpha",
    "development",
    "custom",
    "unknown",
  ]);
  const support = asOneOf(record["support"], [
    "verified",
    "compatible-untested",
    "prerelease-untested",
    "custom-build-untested",
    "unsupported-major",
    "runtime-only",
    "invalid",
  ]);
  if (
    edition === null ||
    editionConfidence === null ||
    releaseChannel === null ||
    support === null
  ) {
    return null;
  }
  const capabilities = parseCapabilitiesField(record["capabilities"]);
  if (capabilities === null) {
    return null;
  }
  const verifiedCapabilities = asBoundedStringArray(record["verifiedCapabilities"], 32, 512);
  const degradedCapabilities = asBoundedStringArray(record["degradedCapabilities"], 32, 512);
  if (verifiedCapabilities === null || degradedCapabilities === null) {
    return null;
  }
  const apiDumpSha256Value = record["apiDumpSha256"];
  const apiDumpSha256 = apiDumpSha256Value === null ? null : asSha256(apiDumpSha256Value);
  if (apiDumpSha256Value !== null && apiDumpSha256 === null) {
    return null;
  }
  const probedAtMs = asFiniteNumber(record["probedAtMs"], 0);
  if (probedAtMs === null) {
    return null;
  }
  const diagnostics = parseDiagnosticsField(record["diagnostics"]);
  if (diagnostics === null) {
    return null;
  }
  return {
    schemaVersion: ENGINE_PROFILE_CACHE_SCHEMA_VERSION,
    installationId,
    executable: { canonicalPath, sizeBytes, modifiedAtMs, sha256 },
    version,
    edition,
    editionConfidence,
    releaseChannel,
    capabilities,
    verifiedCapabilities,
    degradedCapabilities,
    apiDumpSha256,
    support,
    probedAtMs,
    diagnostics,
  };
}

const VERSION_STATUSES: readonly GodotVersionStatus[] = [
  "stable",
  "rc",
  "beta",
  "alpha",
  "dev",
  "custom",
  "unknown",
];

function parseVersionField(value: unknown): GodotVersion | null {
  const record = asRecord(value);
  if (record === null) {
    return null;
  }
  const raw = asBoundedString(record["raw"], 512);
  const major = asInteger(record["major"]);
  const minor = asInteger(record["minor"]);
  const status = asOneOf(record["status"], VERSION_STATUSES);
  if (raw === null || major === null || minor === null || status === null) {
    return null;
  }
  const patch = record["patch"] === null ? null : asInteger(record["patch"]);
  if (record["patch"] !== null && patch === null) {
    return null;
  }
  const statusNumber = record["statusNumber"] === null ? null : asInteger(record["statusNumber"]);
  if (record["statusNumber"] !== null && statusNumber === null) {
    return null;
  }
  const build = record["build"] === null ? null : asBoundedString(record["build"], 512);
  if (record["build"] !== null && build === null) {
    return null;
  }
  const commit = record["commit"] === null ? null : asBoundedString(record["commit"], 512);
  if (record["commit"] !== null && commit === null) {
    return null;
  }
  return {
    raw,
    major,
    minor,
    patch,
    status,
    statusNumber,
    build,
    commit,
  };
}

function parseCapabilitiesField(value: unknown): GodotCommandCapabilities | null {
  const record = asRecord(value);
  if (record === null) {
    return null;
  }
  const result: Record<string, boolean> = {};
  for (const key of Object.keys(createEmptyGodotCommandCapabilities())) {
    const field = record[key];
    if (typeof field !== "boolean") {
      return null;
    }
    result[key] = field;
  }
  // Every known capability key was validated as a boolean above; the cast
  // only narrows the fully-validated record to the typed interface.
  return result as unknown as GodotCommandCapabilities;
}

function parseDiagnosticsField(value: unknown): SafeDiagnostic[] | null {
  if (!Array.isArray(value) || value.length > 100) {
    return null;
  }
  const diagnostics: SafeDiagnostic[] = [];
  for (const entry of value) {
    const record = asRecord(entry);
    if (record === null) {
      return null;
    }
    const severity = asOneOf(record["severity"], ["info", "warning", "error"]);
    const message = asBoundedString(record["message"], 2048);
    if (severity === null || message === null) {
      return null;
    }
    diagnostics.push({ severity, message });
  }
  return diagnostics;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
}

function asBoundedString(value: unknown, maxLength: number): string | null {
  if (typeof value !== "string" || value.length > maxLength) {
    return null;
  }
  return value;
}

function asBoundedStringArray(
  value: unknown,
  maxLength: number,
  maxStringLength: number,
): string[] | null {
  if (!Array.isArray(value) || value.length > maxLength) {
    return null;
  }
  const result: string[] = [];
  for (const entry of value) {
    const text = asBoundedString(entry, maxStringLength);
    if (text === null) {
      return null;
    }
    result.push(text);
  }
  return result;
}

function asFiniteNumber(value: unknown, minimum: number): number | null {
  if (typeof value !== "number" || !Number.isFinite(value) || value < minimum) {
    return null;
  }
  return value;
}

function asInteger(value: unknown): number | null {
  if (typeof value !== "number" || !Number.isSafeInteger(value)) {
    return null;
  }
  return value;
}

function asSha256(value: unknown): string | null {
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/.test(value)) {
    return null;
  }
  return value;
}

function asOneOf<T extends string>(value: unknown, allowed: readonly T[]): T | null {
  if (typeof value !== "string" || !allowed.includes(value as T)) {
    return null;
  }
  return value as T;
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
