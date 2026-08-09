import { canonicalizeJson, sha256Hex } from "./digest.js";
import type { GodotVersion } from "./version.js";
import type { GodotApiIndex } from "./api.js";

/**
 * Version-matched Godot API knowledge profile (schema version 1).
 *
 * The profile is immutable after creation and bound to three identities:
 * the exact executable fingerprint (SHA-256 of the verified executable),
 * the exact API dump (SHA-256 of the generated `extension_api.json`), and
 * the Solaris knowledge schema version. A profile must never silently
 * survive an executable fingerprint change: any mismatch invalidates the
 * profile and requires regeneration.
 */
export interface GodotKnowledgeProfileV1 {
  readonly version: 1;
  readonly engine: {
    readonly installationId: string;
    readonly executableSha256: string;
    readonly godotVersion: string;
    readonly edition: string;
  };
  readonly api: {
    readonly dumpSha256: string;
    readonly generatedAt: string;
    readonly classCount: number;
    readonly builtinClassCount: number;
    readonly utilityFunctionCount: number;
    readonly globalEnumCount: number;
    readonly globalConstantCount: number;
  };
  readonly index: {
    readonly schemaVersion: number;
    readonly symbolCount: number;
  };
}

export const KNOWLEDGE_SCHEMA_VERSION = 1;

/** Deterministic SHA-256 over every field of the profile. */
export function computeGodotKnowledgeProfileDigest(profile: GodotKnowledgeProfileV1): string {
  return sha256Hex(canonicalizeJson(profile));
}

/**
 * A complete loaded knowledge base: the immutable profile plus the bounded
 * in-memory symbol index derived from the exact engine-generated dump. The
 * provider can never see the raw dump; it receives only bounded search and
 * lookup results.
 */
export interface GodotKnowledgeBase {
  readonly profile: GodotKnowledgeProfileV1;
  readonly index: GodotApiIndex;
}

/**
 * Cache-invalidation decision for a stored knowledge profile against the
 * currently selected engine and its fresh API dump. A profile is valid only
 * when the executable fingerprint, the API dump hash, and the schema
 * version all match; anything else fails closed and requires regeneration.
 */
export type GodotKnowledgeCacheValidation =
  | {
      readonly valid: true;
    }
  | {
      readonly valid: false;
      readonly reason: "executable-changed" | "dump-changed" | "schema-changed";
    };

export function validateGodotKnowledgeCache(
  profile: GodotKnowledgeProfileV1,
  expected: {
    readonly executableSha256: string;
    readonly dumpSha256: string;
    readonly schemaVersion: number;
  },
): GodotKnowledgeCacheValidation {
  if (profile.engine.executableSha256 !== expected.executableSha256) {
    return { valid: false, reason: "executable-changed" };
  }
  if (profile.api.dumpSha256 !== expected.dumpSha256) {
    return { valid: false, reason: "dump-changed" };
  }
  if (profile.index.schemaVersion !== expected.schemaVersion) {
    return { valid: false, reason: "schema-changed" };
  }
  return { valid: true };
}

/**
 * Official-manual documentation channel matching an exact engine version.
 * Solaris does not synchronize manual documentation yet; the channel is
 * safe metadata only and is never silently replaced by `latest` docs for a
 * stable project. Stable versions map to their exact `<major>.<minor>`
 * channel; prerelease and custom builds are `unverified`.
 */
export function classifyGodotManualChannel(version: GodotVersion): string {
  if (version.status === "stable") {
    return `${version.major}.${version.minor}`;
  }
  return "unverified";
}

/**
 * Truthful platform-level support state for exact-engine API knowledge
 * generation. Generation is available only when the engine launch can be
 * mechanically bound to the verified executable identity (exec-by-handle)
 * and an enforcing sandbox backend exists. Anything less is reported as
 * unavailable and the runner refuses before creating a probe directory or
 * launching Godot.
 */
export interface GodotKnowledgeSupport {
  readonly state: "available" | "unavailable";
  /** Exact reason when unavailable; null when available. */
  readonly reason: string | null;
  readonly platform: string;
}

export type GodotKnowledgeState = "ready" | "unavailable" | "unsupported";

/**
 * Bounded in-memory knowledge state for CLI diagnostics. Nothing here is
 * persistent trust data; the cache is an explicitly unavailable no-op at
 * this stage, so `ready` is only reachable through an injected knowledge
 * base in tests.
 */
export interface GodotKnowledgeStatus {
  readonly state: GodotKnowledgeState;
  readonly reason: string | null;
  readonly platform: string;
  readonly profile: GodotKnowledgeProfileV1 | null;
  readonly cacheEnabled: false;
  readonly schemaVersion: number;
  readonly manualChannel: string | null;
}

export type GodotKnowledgeRefreshResult =
  | {
      readonly status: "ready";
      readonly profile: GodotKnowledgeProfileV1;
      readonly previousProfile: GodotKnowledgeProfileV1 | null;
    }
  | {
      readonly status: "unavailable" | "unsupported" | "failed" | "cancelled";
      readonly message: string;
    };

export type GodotKnowledgeQueryResult =
  | {
      readonly status: "ready";
      readonly engineVersion: string;
      readonly results: readonly import("./api.js").GodotApiSearchResult[];
      readonly truncated: boolean;
    }
  | {
      readonly status: "unavailable" | "unsupported" | "invalid_input" | "failed";
      readonly message: string;
    };

export type GodotKnowledgeLookupResult =
  | {
      readonly status: "ready";
      readonly engineVersion: string;
      readonly result: import("./api.js").GodotApiLookupResult;
    }
  | {
      readonly status: "not_found" | "unavailable" | "unsupported" | "invalid_input" | "failed";
      readonly message: string;
    };

/**
 * Narrow Godot knowledge port owned by core. The adapter implements it; the
 * provider and the CLI consume it. The provider cannot choose the
 * executable, its arguments, the probe directory, the sandbox
 * configuration, the cache location, or any limit; the provider cannot
 * request raw index files and cannot change the engine profile.
 */
export interface GodotKnowledge {
  /** Truthful platform-level support state (never claims availability). */
  support(): Promise<GodotKnowledgeSupport>;

  /**
   * Regenerate the exact-engine API knowledge profile. Project-independent,
   * offline, deterministic, fixed Solaris command; requires an enforcing
   * sandbox and a selected trusted engine. When generation is unavailable
   * on this platform, returns a typed `unavailable` result before creating
   * a probe directory and never launches Godot.
   */
  refresh(signal?: AbortSignal): Promise<GodotKnowledgeRefreshResult>;

  /** Bounded literal/token API search over the loaded knowledge base. */
  search(
    query: import("./api.js").GodotApiSearchQuery,
    signal?: AbortSignal,
  ): Promise<GodotKnowledgeQueryResult>;

  /** Exact-symbol API lookup over the loaded knowledge base. */
  lookup(symbol: string, signal?: AbortSignal): Promise<GodotKnowledgeLookupResult>;

  /** Bounded in-memory knowledge state for CLI diagnostics. */
  status(): GodotKnowledgeStatus;
}
