/**
 * Change-set application contract (§22–§24).
 *
 * The applier owns the exact-application protocol: acquire the serialized
 * mutation lock, revalidate every pre-state precondition, create a
 * checkpoint for every affected existing file (and an absence state for
 * every create), verify all checkpoints durable, then apply prepared
 * files sequentially with post-state hash verification. A partial
 * infrastructure failure triggers internal recovery of the files Solaris
 * just changed from their just-created checkpoints (restoring the
 * approved pre-change state, preserving external changes), and the final
 * outcome is never success after partial application.
 *
 * On this stage every platform gate fails closed: the applier reports
 * typed `unavailable` outcomes before acquiring a lock, recording a
 * checkpoint, or touching a file, because Node offers no directory-relative
 * (openat/renameat) primitive and a same-user process can swap a parent or
 * target at any instruction boundary. The protocol below is tested
 * internal code exercised through injected in-memory file primitives.
 */

/** One file request handed to the applier (frozen prepared state). */
export interface ChangeSetApplyFileRequest {
  readonly path: string;
  readonly operation: "create" | "update" | "delete";
  /** Expected pre-state hash (null only for create). */
  readonly expectedSha256: string | null;
  /** Complete resulting content (create/update); null for delete. */
  readonly content: string | null;
  /** Pre-state hash recorded during preparation. */
  readonly beforeSha256: string | null;
  /** Post-state hash recorded during preparation. */
  readonly afterSha256: string | null;
  /** Preview line counts recorded on the checkpoint metadata. */
  readonly addedLines: number;
  readonly removedLines: number;
}

export interface ChangeSetApplyRequest {
  readonly changeSetId: string;
  readonly files: readonly ChangeSetApplyFileRequest[];
  readonly toolName: string;
  readonly signal?: AbortSignal;
}

export type ChangeSetApplyOutcome =
  | {
      readonly status: "applied";
      readonly checkpointIds: readonly string[];
    }
  | {
      readonly status: "conflict";
      readonly message: string;
      readonly path: string | null;
    }
  | {
      readonly status: "apply_failed_recovered";
      readonly message: string;
      readonly checkpointIds: readonly string[];
    }
  | {
      readonly status: "apply_failed_partial_recovery";
      readonly message: string;
      readonly checkpointIds: readonly string[];
    }
  | {
      readonly status: "apply_failed_uncertain";
      readonly message: string;
      readonly checkpointIds: readonly string[];
    }
  | {
      readonly status: "cancelled";
      readonly message: string;
      readonly checkpointIds: readonly string[];
      /** Files already applied before the cancellation (truthful partial state). */
      readonly appliedFiles: readonly string[];
    }
  | {
      readonly status: "unavailable" | "failed";
      readonly message: string;
      readonly checkpointIds: readonly string[];
    };

/**
 * Identity-bound file primitives the applier protocol uses. The
 * production composition injects a fail-closed implementation that never
 * touches the filesystem (nothing is ever created, written, or deleted
 * until a mechanically identity-bound commit primitive exists); tests
 * inject an in-memory implementation to exercise the protocol.
 */
export interface ChangeSetFilePrimitives {
  /** Current state of one workspace-relative file (read-only). */
  readFile(path: string): Promise<{ readonly exists: boolean; readonly sha256: string | null }>;
  /**
   * Complete current content of one workspace-relative file plus its
   * state; null content when the file does not exist. Used to record
   * checkpoint preimages before application.
   */
  readContent(path: string): Promise<{
    readonly exists: boolean;
    readonly sha256: string | null;
    readonly content: string | null;
  }>;
  /** Write the complete content of one workspace-relative file. */
  writeFile(path: string, content: string, signal?: AbortSignal): Promise<void>;
  /** Delete one workspace-relative file. */
  deleteFile(path: string, signal?: AbortSignal): Promise<void>;
}

export interface DevelopmentChangeSetApplier {
  /**
   * True only when the platform can mechanically bind every write to the
   * verified parent/target identity. False at this stage on every
   * platform; the applier then refuses before any lock, checkpoint, or
   * write.
   */
  isAvailable(): Promise<boolean>;

  apply(
    request: ChangeSetApplyRequest,
    primitives: ChangeSetFilePrimitives,
  ): Promise<ChangeSetApplyOutcome>;
}
