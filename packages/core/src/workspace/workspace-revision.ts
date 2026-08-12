import { canonicalizeJson, sha256Hex } from "../godot/digest.js";

/**
 * Workspace revision handles (Stage 3 milestone 3).
 *
 * An opaque model-facing revision handle identifies one exact cryptographic
 * file state:
 *
 *   rev_<32 hex chars>
 *
 * The handle is a convenience reference, never authority: possession does
 * not grant read/write access, approval, path access, or sandbox bypass.
 * The authoritative identity is the whole-file SHA-256, and every mutation
 * revalidates the current file against the SHA-256 the handle resolves to.
 * Handles are workspace-scoped: the same relative path in a different
 * workspace never resolves. The registry is session-scoped, in-memory, and
 * bounded — no durable storage is required.
 */

export type WorkspaceRevisionHandle = string;

export const WORKSPACE_REVISION_HANDLE_PREFIX = "rev_";
export const DEFAULT_REVISION_REGISTRY_LIMIT = 1024;

export interface WorkspaceRevisionIdentity {
  readonly workspaceFingerprint: string;
  readonly path: string;
  readonly sha256: string;
}

export interface ObservedWorkspaceRead {
  readonly path: string;
  readonly revision: WorkspaceRevisionHandle;
  readonly mode: "exact" | "structural" | "summary";
  readonly atMs: number;
}

/** Deterministic handle for one exact file state in one workspace. */
export function computeWorkspaceRevisionHandle(
  workspaceFingerprint: string,
  path: string,
  sha256: string,
): WorkspaceRevisionHandle {
  const digest = sha256Hex(canonicalizeJson({ workspace: workspaceFingerprint, path, sha256 }));
  // 128 bits of the cryptographic identity tuple: collision-safe for
  // practical runtime use, ergonomic for the model, and the registry is
  // authoritative for resolution (an unknown handle is always rejected).
  return `${WORKSPACE_REVISION_HANDLE_PREFIX}${digest.slice(0, 32)}`;
}

export interface WorkspaceRevisionRegistry {
  /** Issue (or return the existing) handle for one exact file state. */
  issue(path: string, sha256: string): WorkspaceRevisionHandle;
  /** Resolve a handle to its full identity; null when unknown/foreign. */
  resolve(handle: WorkspaceRevisionHandle): WorkspaceRevisionIdentity | null;
  /** The last issued valid handle for a path, or null when none. */
  currentRevision(path: string): WorkspaceRevisionHandle | null;
  /** A handle for an exact (path, sha256) state, or null when unknown. */
  revisionForState(path: string, sha256: string): WorkspaceRevisionHandle | null;
  /** Invalidate a path (Siralos mutation or external change detected). */
  invalidatePath(path: string): void;
  /** Record that a read observed a specific revision (multi-agent groundwork). */
  observeRead(
    path: string,
    revision: WorkspaceRevisionHandle,
    mode: ObservedWorkspaceRead["mode"],
  ): void;
  /** Bounded session-local record of reads (task/session metadata only). */
  observedReads(): readonly ObservedWorkspaceRead[];
  readonly size: number;
  clear(): void;
}

export interface WorkspaceRevisionRegistryOptions {
  readonly workspaceFingerprint: string;
  readonly maxEntries?: number;
}

export function createWorkspaceRevisionRegistry(
  options: WorkspaceRevisionRegistryOptions,
): WorkspaceRevisionRegistry {
  const maxEntries = options.maxEntries ?? DEFAULT_REVISION_REGISTRY_LIMIT;
  if (maxEntries < 1) {
    throw new Error("The revision registry requires a positive entry limit.");
  }
  // path -> current valid handle (FIFO eviction order preserved).
  const byPath = new Map<string, WorkspaceRevisionHandle>();
  // handle -> full identity.
  const identities = new Map<WorkspaceRevisionHandle, WorkspaceRevisionIdentity>();
  // insertion order for bounded eviction.
  const order: WorkspaceRevisionHandle[] = [];
  const observed: ObservedWorkspaceRead[] = [];
  let clock = 0;

  function evictIfNeeded(): void {
    while (identities.size > maxEntries) {
      const oldest = order.shift();
      if (oldest === undefined) {
        break;
      }
      const identity = identities.get(oldest);
      identities.delete(oldest);
      if (identity !== undefined && byPath.get(identity.path) === oldest) {
        byPath.delete(identity.path);
      }
    }
  }

  return {
    issue(path: string, sha256: string): WorkspaceRevisionHandle {
      const existing = byPath.get(path);
      if (existing !== undefined) {
        const identity = identities.get(existing);
        if (identity !== undefined && identity.sha256 === sha256) {
          return existing;
        }
      }
      const handle = computeWorkspaceRevisionHandle(options.workspaceFingerprint, path, sha256);
      identities.set(handle, { workspaceFingerprint: options.workspaceFingerprint, path, sha256 });
      order.push(handle);
      byPath.set(path, handle);
      evictIfNeeded();
      return handle;
    },

    resolve(handle: WorkspaceRevisionHandle): WorkspaceRevisionIdentity | null {
      if (!handle.startsWith(WORKSPACE_REVISION_HANDLE_PREFIX)) {
        return null;
      }
      return identities.get(handle) ?? null;
    },

    currentRevision(path: string): WorkspaceRevisionHandle | null {
      return byPath.get(path) ?? null;
    },

    revisionForState(path: string, sha256: string): WorkspaceRevisionHandle | null {
      for (const [handle, identity] of identities) {
        if (identity.path === path && identity.sha256 === sha256) {
          return handle;
        }
      }
      return null;
    },

    invalidatePath(path: string): void {
      const handle = byPath.get(path);
      if (handle !== undefined) {
        byPath.delete(path);
        // The identity stays resolvable as historical evidence; only the
        // current-revision binding is dropped.
      }
    },

    observeRead(
      path: string,
      revision: WorkspaceRevisionHandle,
      mode: ObservedWorkspaceRead["mode"],
    ): void {
      clock += 1;
      observed.push({ path, revision, mode, atMs: clock });
      if (observed.length > 64) {
        observed.shift();
      }
    },

    observedReads(): readonly ObservedWorkspaceRead[] {
      return [...observed];
    },

    get size(): number {
      return identities.size;
    },

    clear(): void {
      byPath.clear();
      identities.clear();
      order.length = 0;
      observed.length = 0;
    },
  };
}
