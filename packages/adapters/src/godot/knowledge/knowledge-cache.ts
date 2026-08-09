import type { GodotKnowledgeBase } from "@solaris/core";

export const KNOWLEDGE_CACHE_SCHEMA_VERSION = 1;

export interface KnowledgeCacheStoreOutcome {
  readonly ok: false;
  readonly reason: "unavailable";
  readonly message: string;
}

export interface GodotKnowledgeCache {
  /**
   * Always resolves to null (a cache miss) without any filesystem access.
   * The knowledge cache is explicitly unavailable while engine probing and
   * generation are unavailable: it is never initialized, created, read, or
   * written, so no cache path can ever be misclassified as a link and no
   * cached data can ever be served.
   */
  load(executableSha256: string): Promise<GodotKnowledgeBase | null>;

  /**
   * Never stores anything. Returns a typed unavailable result; nothing is
   * created, renamed, or overwritten, so no parent-substitution or
   * atomicity property is claimed or attempted.
   */
  store(base: GodotKnowledgeBase): Promise<KnowledgeCacheStoreOutcome>;

  /** Always resolves to 0 without any filesystem access. */
  count(): Promise<0>;
}

export interface KnowledgeCacheOptions {
  /**
   * Accepted for signature compatibility and future use; the unavailable
   * implementation never reads or creates this directory.
   */
  readonly rootDirectory?: string;
}

export const KNOWLEDGE_CACHE_UNAVAILABLE_MESSAGE =
  "The Godot knowledge cache is unavailable: exact-engine API generation is unavailable at this stage, and Node offers no directory-relative or handle-relative primitive with which a cache write could be bound to a verified parent object. The cache is never initialized, created, read, or written.";

/**
 * Explicitly unavailable knowledge cache.
 *
 * API generation fails closed (the runner never spawns the executable), so
 * no knowledge profile can ever be produced. A cache that is never
 * exercised cannot be attacked: this component performs ZERO filesystem
 * operations — no directory verification, no creation, no read, no write,
 * no rename, no cleanup — and therefore no Windows canonicalization failure
 * can be misreported as a link, and no parent or entry substitution can
 * redirect a write. `load()` is always a miss, `store()` returns a typed
 * unavailable outcome, and `count()` is 0. When a mechanically identity-
 * bound generation primitive exists, the cache design (fingerprint and dump
 * binding, schema validation, symlink rejection, bounded size) must be
 * rebuilt from scratch against that primitive; nothing from this component
 * is retained as a claimed capability.
 */
export function createGodotKnowledgeCache(
  _options: KnowledgeCacheOptions = {},
): GodotKnowledgeCache {
  return {
    load(): Promise<GodotKnowledgeBase | null> {
      return Promise.resolve(null);
    },
    store(): Promise<KnowledgeCacheStoreOutcome> {
      return Promise.resolve({
        ok: false,
        reason: "unavailable",
        message: KNOWLEDGE_CACHE_UNAVAILABLE_MESSAGE,
      });
    },
    count(): Promise<0> {
      return Promise.resolve(0);
    },
  };
}
