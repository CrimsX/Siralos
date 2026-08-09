import type {
  ReferenceAlias,
  ReferenceId,
  ReferenceRevision,
  ReferenceSource,
  ResolvedReferenceIdentity,
} from "./reference-model.js";
import type { MaterializationStatus } from "./reference-model.js";

/**
 * Reference ports (Stage 3 milestone 5).
 *
 * Core defines the ports; adapters implement them. The resolver maps a
 * declared source to a resolved identity (canonical path + fingerprint for
 * local directories, origin + commit for repositories). The materializer
 * manages the Solaris-owned private cache for repository references. The
 * access port provides list/read/search over a materialized reference.
 *
 * Core never performs network or process execution and never touches the
 * filesystem: these ports are the only boundary.
 */

export type ReferenceResolutionOutcome =
  | { readonly status: "resolved"; readonly identity: ResolvedReferenceIdentity }
  | { readonly status: "unavailable"; readonly reason: string }
  | { readonly status: "refused"; readonly reason: string }
  | { readonly status: "failed"; readonly reason: string };

export interface ReferenceResolverPort {
  /**
   * Resolve a declared source to an immutable identity. `allowMutableRefs`
   * is the registry's policy: when false, mutable refs (branch / absent
   * ref) must be refused by the resolver too — the registry also refuses
   * them before calling the resolver, as defense in depth.
   */
  resolveIdentity(
    source: ReferenceSource,
    options: { readonly allowMutableRefs: boolean },
  ): Promise<ReferenceResolutionOutcome>;
}

export type MaterializationOutcome =
  | { readonly status: "materialized"; root: string }
  | { readonly status: "unavailable"; readonly reason: string }
  | { readonly status: "refused"; readonly reason: string }
  | { readonly status: "failed"; readonly reason: string };

export interface ReferenceMaterializerPort {
  /**
   * Materialize a resolved reference into the Solaris-owned managed cache.
   * `root` is the private cache absolute path — INTERNAL only, never
   * model-facing.
   */
  materialize(
    referenceId: ReferenceId,
    identity: ResolvedReferenceIdentity,
  ): Promise<MaterializationOutcome>;
  status(referenceId: ReferenceId): MaterializationStatus;
}

export interface ReferenceReadRequest {
  readonly reference: ReferenceId;
  /** Reference-relative path (forward slashes, no leading slash). */
  readonly path: string;
  readonly mode: "exact" | "structural" | "summary";
  readonly startLine?: number;
  readonly endLine?: number;
}

export type ReferenceReadResult =
  | {
      readonly status: "ok";
      readonly referenceId: ReferenceId;
      readonly alias: ReferenceAlias;
      /** Revision of the reference at read time (registry-owned identity). */
      readonly revision: ReferenceRevision;
      readonly path: string;
      readonly sha256: string;
      /** Exact content; null when the mode is structural/summary. */
      readonly content: string | null;
      /** GDScript structure; only for `.gd` files in structural mode. */
      readonly structure: unknown;
      /** Bounded advisory summary; only for `.gd` files in summary mode. */
      readonly summary: unknown;
      readonly truncated: boolean;
    }
  | { readonly status: "unavailable"; readonly reason: string }
  | { readonly status: "invalid_path"; readonly reason: string }
  | { readonly status: "not_found"; readonly reason: string }
  | { readonly status: "unsupported"; readonly reason: string }
  | { readonly status: "failed"; readonly reason: string };

export interface ReferenceSearchRequest {
  readonly reference: ReferenceId;
  readonly query: string;
  /** Reference-relative path to scope the search; optional. */
  readonly path?: string;
  readonly maxResults?: number;
}

export type ReferenceSearchResult =
  | {
      readonly status: "ok";
      readonly referenceId: ReferenceId;
      readonly alias: ReferenceAlias;
      readonly revision: ReferenceRevision;
      readonly query: string;
      readonly matches: readonly {
        readonly path: string;
        readonly line: number;
        readonly column: number;
        readonly text: string;
      }[];
      readonly scannedFiles: number;
      readonly skippedFiles: number;
      readonly truncated: boolean;
      readonly truncationReason: string | null;
    }
  | { readonly status: "unavailable"; readonly reason: string }
  | { readonly status: "invalid_path"; readonly reason: string }
  | { readonly status: "not_found"; readonly reason: string }
  | { readonly status: "failed"; readonly reason: string };

export interface ReferenceListRequest {
  readonly reference: ReferenceId;
  /** Reference-relative directory; optional (defaults to the root). */
  readonly path?: string;
}

export type ReferenceListResult =
  | {
      readonly status: "ok";
      readonly referenceId: ReferenceId;
      readonly alias: ReferenceAlias;
      readonly revision: ReferenceRevision;
      readonly path: string;
      readonly entries: readonly {
        readonly name: string;
        readonly path: string;
        readonly type: "file" | "directory" | "symlink" | "other";
        readonly size?: number;
      }[];
      readonly truncated: boolean;
    }
  | { readonly status: "unavailable"; readonly reason: string }
  | { readonly status: "invalid_path"; readonly reason: string }
  | { readonly status: "not_found"; readonly reason: string }
  | { readonly status: "failed"; readonly reason: string };

/**
 * The access port over one materialized reference. Implementations are
 * fs-bound adapters and must fill `alias`/`revision` from the owning
 * registry (the registry is the SINGLE owner of reference identity; the
 * adapter never resolves or refreshes references itself — it receives the
 * identity context it needs at construction from the composition root).
 */
export interface ReferenceAccessPort {
  list(request: ReferenceListRequest): Promise<ReferenceListResult>;
  read(request: ReferenceReadRequest): Promise<ReferenceReadResult>;
  search(request: ReferenceSearchRequest): Promise<ReferenceSearchResult>;
}
