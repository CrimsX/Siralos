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

export interface EngineProfileStoreOutcome {
  readonly ok: false;
  readonly reason: "unavailable";
  readonly message: string;
}

export interface GodotEngineProfileCache {
  /**
   * Always resolves to null (a cache miss) without any filesystem access.
   * The cache is explicitly unavailable while engine probing is unavailable:
   * it is never initialized, created, read, or written, so no cache path can
   * ever be misclassified as a link and no cached data can ever be served.
   */
  load(executableSha256: string): Promise<null>;

  /**
   * Never stores anything. Returns a typed unavailable result; nothing is
   * created, renamed, or overwritten, so no parent-substitution or
   * atomicity property is claimed or attempted.
   */
  store(profile: CachedEngineProfile): Promise<EngineProfileStoreOutcome>;

  /** Always resolves to 0 without any filesystem access. */
  count(): Promise<0>;
}

export interface EngineProfileCacheOptions {
  /**
   * Accepted for signature compatibility and future use; the unavailable
   * implementation never reads or creates this directory.
   */
  readonly rootDirectory?: string;
  readonly maxEntries?: number;
}

export const ENGINE_PROFILE_CACHE_UNAVAILABLE_MESSAGE =
  "The engine-profile cache is unavailable: engine probing is unavailable at this stage, and Node offers no directory-relative or handle-relative primitive with which a cache write could be bound to a verified parent object. The cache is never initialized, created, read, or written.";

/**
 * Explicitly unavailable engine-profile cache.
 *
 * Engine probing is intentionally unavailable (the probe runner reports
 * `unavailable` and never spawns the executable), so no engine profile can
 * ever be produced. A cache that is never exercised cannot be attacked: this
 * component performs ZERO filesystem operations — no directory verification,
 * no creation, no read, no write, no rename, no cleanup — and therefore no
 * Windows canonicalization failure can be misreported as a link, and no
 * parent or entry substitution can redirect a write. `load()` is always a
 * miss, `store()` returns a typed unavailable outcome, and `count()` is 0.
 * When a mechanically identity-bound probing primitive exists, the cache
 * design (full-hash revalidation before a hit is served) must be rebuilt
 * from scratch against that primitive; nothing from this component is
 * retained as a claimed capability.
 */
export function createEngineProfileCache(
  _options: EngineProfileCacheOptions = {},
): GodotEngineProfileCache {
  return {
    load(): Promise<null> {
      return Promise.resolve(null);
    },
    store(): Promise<EngineProfileStoreOutcome> {
      return Promise.resolve({
        ok: false,
        reason: "unavailable",
        message: ENGINE_PROFILE_CACHE_UNAVAILABLE_MESSAGE,
      });
    },
    count(): Promise<0> {
      return Promise.resolve(0);
    },
  };
}
