/**
 * Disposable Godot project mirror port. Core owns the contract; the adapter
 * owns every filesystem operation. The provider cannot choose the mirror
 * location or its contents, and the project cannot influence the copy
 * policy: the request and the limits are Siralos-generated and fixed.
 *
 * The mirror may execute only when the adapter can mechanically guarantee
 * that no file or directory can be created outside the verified private
 * root (creation bound to verified parent identity, intermediate links
 * refused), that the mirror contains exactly the approved bytes, and that
 * cleanup deletes exactly the created objects and cannot delete a later
 * substituted object. Where Node lacks the necessary directory-relative
 * (openat/mkdirat-style) and delete-by-handle primitives, `isAvailable()`
 * returns false and every operation reports a typed `unavailable` outcome
 * without touching the filesystem.
 */

export interface ProjectMirrorRequest {
  /** The source workspace root; never passed to the engine. */
  readonly workspaceRoot: string;
  /**
   * Siralos-generated verified parent directory in which the mirror is
   * created. Must not resolve inside the workspace or any forbidden root.
   */
  readonly parentDirectory: string;
  /** Roots the mirror must never resolve inside (e.g. checkpoint storage). */
  readonly forbiddenRoots?: readonly string[];
  readonly signal?: AbortSignal;
}

export interface ProjectMirrorFileEntry {
  /** Workspace-relative path with `/` separators. */
  readonly relativePath: string;
  readonly bytes: number;
  readonly sha256: string;
}

export interface PreparedProjectMirror {
  /** Absolute mirror project path (Siralos-generated; never provider-visible). */
  readonly projectPath: string;
  readonly sourceRoot: string;
  readonly parentDirectory: string;
  readonly entries: readonly ProjectMirrorFileEntry[];
  readonly copiedBytes: number;
}

export type ProjectMirrorPreparationResult =
  | {
      readonly status: "ready";
      readonly mirror: PreparedProjectMirror;
    }
  | {
      readonly status: "unavailable";
      readonly message: string;
    }
  | {
      readonly status: "conflict";
      readonly message: string;
    }
  | {
      readonly status: "mirror_unsupported";
      readonly message: string;
    }
  | {
      readonly status: "too_large";
      /** Which limit prevented the copy: files, bytes, single-file, path-length, depth. */
      readonly limit: string;
      readonly message: string;
    }
  | {
      readonly status: "failed";
      readonly message: string;
    };

export type ProjectMirrorVerification =
  | {
      readonly ok: true;
    }
  | {
      readonly ok: false;
      readonly reason:
        | "unavailable"
        | "unexpected-files"
        | "symlink"
        | "hash-mismatch"
        | "source-conflict"
        | "missing"
        | "inaccessible";
      readonly message: string;
    };

export interface ProjectMirror {
  /**
   * False when the platform/runtime cannot provide identity-bound mirror
   * creation and cleanup; every operation then reports `unavailable` and
   * performs zero filesystem operations.
   */
  isAvailable(): Promise<boolean>;

  prepare(request: ProjectMirrorRequest): Promise<ProjectMirrorPreparationResult>;

  verify(mirror: PreparedProjectMirror, signal?: AbortSignal): Promise<ProjectMirrorVerification>;

  destroy(mirror: PreparedProjectMirror): Promise<
    | {
        readonly ok: true;
      }
    | {
        readonly ok: false;
        readonly reason: "unavailable" | "failed";
        readonly message: string;
      }
  >;
}
