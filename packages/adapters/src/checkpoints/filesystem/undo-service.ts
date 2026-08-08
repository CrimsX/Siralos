import type { CheckpointStore, UndoOutcome, UndoService } from "@solaris/core";
import type { ApprovalReviewer } from "@solaris/core";
import type { MutationLock } from "../../tools/workspace/mutations/mutation-lock.js";

export interface UndoServiceDependencies {
  readonly workspaceRoot: string;
  readonly store: CheckpointStore;
  readonly lock: MutationLock;
  readonly reviewer: ApprovalReviewer;
}

export const UNDO_UNAVAILABLE_MESSAGE =
  "Undo is unavailable: restoring a checkpoint requires pathname-based displacement and replacement, and Node offers no directory-relative (openat/renameat) primitive, so a same-user process that swaps a parent or target at any instruction boundary could redirect the restore outside the workspace. Undo fails closed before any write; it will become available when a mechanically identity-bound commit primitive exists.";

/**
 * The undo service fails closed before any write.
 *
 * Checkpoint restoration performs the same pathname-based displacement and
 * replacement as the workspace mutation tools, so it is subject to the same
 * same-user parent/target substitution risk. Node's filesystem APIs resolve
 * mutable pathnames and verification-then-use is not atomic; the pinned
 * runtime exposes no dirfd-relative primitive and no native adapter is
 * shipped. Rather than performing a partial outside mutation and describing
 * cleanup as prevention, undo is refused before any filesystem activity,
 * approval, or checkpoint state change.
 */
export function createUndoService(_dependencies: UndoServiceDependencies): UndoService {
  return {
    async undo(): Promise<UndoOutcome> {
      return {
        type: "failed",
        checkpointId: null,
        path: null,
        message: UNDO_UNAVAILABLE_MESSAGE,
      };
    },
  };
}
