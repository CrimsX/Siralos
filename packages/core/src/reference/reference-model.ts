import { canonicalizeJson, sha256Hex } from "../godot/digest.js";

/**
 * External reference model (Stage 3 milestone 5).
 *
 * A reference is a first-class, read-only external source the application
 * may consult — a local directory outside the workspace namespace, or a
 * remote repository pinned to an immutable commit. References are the
 * foundation for `/evolve`-style research; this milestone ships the
 * contracts, the registry, and the projection integration. Core never
 * touches the network or the filesystem: resolution and materialization
 * happen through typed ports implemented by adapters.
 *
 * Reference identity is host-owned. The registry is the SINGLE owner of
 * reference identity in the application: CLI, provider adapters,
 * ContextProjector, and EvidenceProjector never resolve or refresh
 * references themselves.
 */

/** Opaque branded reference id: `ref_` + 24 hex chars derived from the alias. */
export type ReferenceId = string & { readonly __referenceId: unique symbol };

/** Opaque branded alias: `^[a-z][a-z0-9._-]{1,63}$`. */
export type ReferenceAlias = string & { readonly __referenceAlias: unique symbol };

export const REFERENCE_ID_PREFIX = "ref_";

/**
 * Deterministic reference id from the alias. The id is a convenience
 * reference, never authority: it identifies the declaration slot, while
 * the current revision carries the resolved identity (fingerprint /
 * commit). Ids are derived from the alias so the same alias always mints
 * the same id across sessions and the registry can be reconstructed.
 */
export function createReferenceId(alias: string): ReferenceId {
  const digest = sha256Hex(canonicalizeJson({ alias }));
  return `${REFERENCE_ID_PREFIX}${digest.slice(0, 24)}` as ReferenceId;
}

/**
 * Validate a reference alias. Rejects non-strings, wrong shape, and
 * anything longer than the pattern allows (the pattern caps at 64 chars,
 * matching `REFERENCE_LIMITS.maxAliasLength`).
 */
export function validateReferenceAlias(value: unknown): ReferenceAlias | null {
  if (typeof value !== "string") {
    return null;
  }
  if (!/^[a-z][a-z0-9._-]{1,63}$/.test(value)) {
    return null;
  }
  return value as ReferenceAlias;
}

/** Model-facing display name of an alias. */
export function formatReferenceAlias(alias: ReferenceAlias): string {
  return `@reference/${alias}`;
}

export type ReferenceKind = "local-directory" | "repository";

/**
 * Repository pin. Exactly one of commit/tag/branch is present in a
 * validated declaration; a branch (or an absent ref, which defaults to
 * branch `main`) is a MUTABLE ref and requires `allowMutableRefs` at
 * resolution — the resolved commit is always what gets recorded.
 */
export type RepositoryRef =
  | { readonly kind: "commit"; readonly commit: string }
  | { readonly kind: "tag"; readonly tag: string }
  | { readonly kind: "branch"; readonly branch: string };

export type ReferenceSource =
  | { readonly kind: "local-directory"; readonly path: string }
  | { readonly kind: "repository"; readonly repository: string; readonly ref: RepositoryRef };

/**
 * Trust class of a reference declaration. `explicit-user` references are
 * declared by the user directly; `trusted-project` / `untrusted-project`
 * come from project configuration (the default policy treats project
 * declarations as untrusted); `managed` is reserved for host-managed
 * references. Trust is metadata for policy decisions made elsewhere —
 * possession of a reference never grants capability.
 */
export type ReferenceTrustClass =
  "explicit-user" | "trusted-project" | "untrusted-project" | "managed";

/**
 * Resolved identity of one reference at one point in time. Local
 * directories are identified by their canonical absolute path and a
 * fingerprint; repositories by origin + resolved commit (plus the
 * requested pin, for auditability).
 */
export type ResolvedReferenceIdentity =
  | {
      readonly kind: "local-directory";
      readonly canonicalPath: string;
      readonly fingerprint: string;
    }
  | {
      readonly kind: "repository";
      readonly origin: string;
      readonly commit: string;
      readonly requestedRef: RepositoryRef;
    };

/** One immutable revision of a reference's resolved identity. */
export interface ReferenceRevision {
  readonly identity: ResolvedReferenceIdentity;
  readonly resolvedAtMs: number;
}

/**
 * Reference status. Declined/unresolvable references stay listed so the
 * `reference` configuration is visible and auditable even when unusable:
 *
 *   ready             — resolved; `revision()` returns the current revision
 *   resolution-failed — the resolver reported failure
 *   unavailable       — the resolver reported the source unavailable
 *   declined          — the host refused the declaration (duplicate alias,
 *                       workspace-namespace containment, mutable ref without
 *                       an explicit pin, resolver refusal)
 */
export type ReferenceStatus = "ready" | "resolution-failed" | "unavailable" | "declined";

/**
 * Materialization status (managed cache state). Materialization is
 * INTERNAL — the managed-cache root is never model-facing. Local
 * directories need no materialization; repositories are materialized by
 * the adapter into a Siralos-owned cache.
 */
export type MaterializationStatus =
  "not-required" | "not-materialized" | "materialized" | "unavailable" | "failed";

export interface Reference {
  readonly id: ReferenceId;
  readonly alias: ReferenceAlias;
  readonly kind: ReferenceKind;
  readonly source: ReferenceSource;
  readonly trust: ReferenceTrustClass;
  readonly description: string | null;
  readonly status: ReferenceStatus;
  /** Precise reason for non-ready status; null when ready. */
  readonly failureReason: string | null;
}

export const REFERENCE_LIMITS = {
  /** Maximum declared references per application session. */
  maxReferences: 16,
  /** Maximum alias length (the alias pattern also enforces 2..64). */
  maxAliasLength: 64,
  /** Maximum UTF-8 bytes of a reference description. */
  maxDescriptionBytes: 512,
  /** Maximum length of the normalized repository origin. */
  maxRepositoryLength: 2048,
  /** Maximum length of a declared local-directory path. */
  maxLocalDirectoryPathLength: 4096,
  /** Maximum commit pin length (the commit pattern also caps at 64). */
  maxCommitLength: 64,
  /** Maximum tag pin length. */
  maxTagLength: 128,
  /** Maximum branch pin length. */
  maxBranchLength: 128,
  /** Maximum manifest entries a materialized reference may expose. */
  maxManifestEntries: 10_000,
  /** Maximum total manifest bytes. */
  maxManifestBytes: 8 * 1024 * 1024,
  /**
   * Per-file SHA-256 cap for reference content hashing. Fail closed: a
   * file above the cap makes the manifest non-fingerprintable with a
   * precise reason — it is never silently marked "unhashed".
   */
  maxFileSha256Bytes: 1024 * 1024,
  /** Task revision bindings retained per registry (FIFO eviction). */
  maxRevisionBindings: 64,
} as const;

export type ReferenceLimits = { readonly [K in keyof typeof REFERENCE_LIMITS]: number };

/**
 * Immutable snapshot of the revisions a task started with. Captured by
 * `bindTask` at task start; the binding outlives later refreshes so task
 * evidence and context stay bound to the revisions the task actually saw.
 */
export interface ReferenceTaskBinding {
  readonly taskId: string;
  /** ReferenceId -> revision, for every reference that was ready at bind time. */
  readonly revisions: ReadonlyMap<ReferenceId, ReferenceRevision>;
  readonly boundAtMs: number;
}

/** Id of an alias (same derivation as `createReferenceId`). */
export function referenceIdOf(alias: ReferenceAlias): ReferenceId {
  return createReferenceId(alias);
}

/** Model-facing display name of a reference: `@reference/<alias>`. */
export function referenceDisplayName(reference: Reference): string {
  return formatReferenceAlias(reference.alias);
}
