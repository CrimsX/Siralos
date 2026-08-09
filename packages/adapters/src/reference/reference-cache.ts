import type { ReferenceRevision, ReferenceSource } from "@solaris/core";

/**
 * Reference managed-cache metadata and store (Stage 3 milestone 5).
 *
 * The managed cache is Solaris-owned private storage OUTSIDE the workspace
 * (documented layout `~/.solaris/references/<fingerprint>/` with a
 * `metadata.json` and the materialized content beneath it). The cache is
 * never model-facing: no absolute cache path ever reaches the model, and
 * cache content is never presented as workspace material.
 *
 * The REAL production store is a fail-closed no-op: `store()` always
 * returns a typed `unavailable` outcome and performs ZERO filesystem
 * operations — nothing is ever created, read, or deleted. Repository
 * materialization requires sandboxed git execution, which is unavailable
 * at this stage, so no cache entry can ever legitimately exist; a no-op
 * store keeps the failure typed and honest instead of inventing an
 * on-disk cache that cannot be identity-bound (Node offers no
 * directory-relative create or delete-by-handle primitive).
 *
 * `createInMemoryCacheStore` is the working store for tests.
 */

export interface ReferenceCacheMetadata {
  readonly fingerprint: string;
  readonly source: ReferenceSource;
  readonly resolvedRevision: ReferenceRevision;
  readonly lastAccessMs: number;
  readonly sizeBytes: number;
}

export type CacheStoreOutcome =
  | { readonly status: "ok" }
  | { readonly status: "unavailable" | "refused" | "failed"; readonly reason: string };

export type CacheStatus =
  { readonly status: "enabled" } | { readonly status: "unavailable"; readonly reason: string };

export interface ReferenceCacheStore {
  load(fingerprint: string): Promise<ReferenceCacheMetadata | null>;
  store(metadata: ReferenceCacheMetadata): Promise<CacheStoreOutcome>;
  status(): CacheStatus;
}

export const REFERENCE_CACHE_UNAVAILABLE_MESSAGE =
  "the reference cache is unavailable at this stage: nothing is ever created, read, or deleted";

export interface CreateReferenceCacheStoreOptions {
  /**
   * Reserved: the documented cache layout is `~/.solaris/references/`.
   * The real store never touches the filesystem, so no root is ever used.
   */
  readonly root?: string;
}

export function createReferenceCacheStore(
  _options: CreateReferenceCacheStoreOptions = {},
): ReferenceCacheStore {
  return {
    load(): Promise<ReferenceCacheMetadata | null> {
      return Promise.resolve(null);
    },
    store(): Promise<CacheStoreOutcome> {
      return Promise.resolve({
        status: "unavailable",
        reason: REFERENCE_CACHE_UNAVAILABLE_MESSAGE,
      });
    },
    status(): CacheStatus {
      return { status: "unavailable", reason: REFERENCE_CACHE_UNAVAILABLE_MESSAGE };
    },
  };
}

/** Working in-memory cache store for tests (deterministic, no fs). */
export function createInMemoryCacheStore(): ReferenceCacheStore {
  const entries = new Map<string, ReferenceCacheMetadata>();
  return {
    load(fingerprint: string): Promise<ReferenceCacheMetadata | null> {
      return Promise.resolve(entries.get(fingerprint) ?? null);
    },
    store(metadata: ReferenceCacheMetadata): Promise<CacheStoreOutcome> {
      entries.set(metadata.fingerprint, metadata);
      return Promise.resolve({ status: "ok" });
    },
    status(): CacheStatus {
      return { status: "enabled" };
    },
  };
}
