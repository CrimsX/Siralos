import { createHash } from "node:crypto";
import { open, readFile, lstat, realpath } from "node:fs/promises";
import path from "node:path";
import type {
  ApprovalRequest,
  ApprovalReviewer,
  ChangePreview,
  CheckpointStore,
  FileCheckpoint,
  UndoOutcome,
  UndoService,
  WorkspaceFileState,
} from "@solaris/core";
import { planUndo } from "@solaris/core";
import type { MutationLock } from "../../tools/workspace/mutations/mutation-lock.js";
import {
  removeCreatedObjectIfSame,
  resolveCreateTarget,
  resolveMutationTarget,
  verifyExclusiveOpenIdentity,
  verifyParentChainIdentity,
  verifyParentChainIdentityOrThrow,
} from "../../tools/workspace/mutations/mutation-paths.js";
import {
  createMutationTempPath,
  removeMutationTemp,
} from "../../tools/workspace/mutations/mutation-temp.js";
import {
  removeQuarantinedCopy,
  replaceFileWithQuarantine,
  unlinkWithIdentityVerification,
} from "../../tools/workspace/mutations/safe-replacement.js";
import { buildUnifiedDiff } from "../../tools/workspace/mutations/diff.js";
import { hashMutationPlan } from "../../tools/workspace/mutations/mutation-hash.js";
import { decodeUtf8 } from "../../tools/workspace/text.js";
import { readWorkspaceFileState } from "./checkpoint-file-state.js";

export interface UndoServiceDependencies {
  readonly workspaceRoot: string;
  readonly store: CheckpointStore;
  readonly lock: MutationLock;
  readonly reviewer: ApprovalReviewer;
  /**
   * Test seam: deterministic barrier invoked immediately before the
   * irreversible commit open, after every final revalidation.
   */
  readonly beforeCommitOpen?: () => Promise<void>;
  /**
   * Test seam: deterministic barrier invoked after an escaped create is
   * detected and its handle closed, immediately before conditional cleanup.
   */
  readonly beforeObjectCleanup?: () => Promise<void>;
}

type UndoAction = "create" | "restore" | "delete";

export function createUndoService(dependencies: UndoServiceDependencies): UndoService {
  let approvalCounter = 0;

  async function selectCheckpoint(checkpointId?: string): Promise<FileCheckpoint | null> {
    if (checkpointId !== undefined) {
      const checkpoint = await dependencies.store.get(checkpointId);
      if (checkpoint === null) {
        return null;
      }
      if (checkpoint.state !== "applied") {
        return null;
      }
      return checkpoint;
    }
    const applied = await dependencies.store.list({ states: ["applied"] });
    if (applied.length === 0) {
      return null;
    }
    const sorted = [...applied].sort((a, b) => (a.createdAt < b.createdAt ? 1 : -1));
    return sorted[0] ?? null;
  }

  async function undo(checkpointId?: string, signal?: AbortSignal): Promise<UndoOutcome> {
    const checkpoint = await selectCheckpoint(checkpointId);
    if (checkpoint === null) {
      return {
        type: "failed",
        checkpointId: null,
        path: null,
        message:
          checkpointId === undefined
            ? "There is no eligible checkpoint to undo."
            : `Checkpoint ${checkpointId} is not eligible for undo.`,
      };
    }
    const path = checkpoint.relativePath;
    if (signal?.aborted) {
      return { type: "cancelled", checkpointId: checkpoint.id, path };
    }
    const current = await readWorkspaceFileState(dependencies.workspaceRoot, path);
    const planned = planUndo(checkpoint, current);
    if (planned.decision === "conflict") {
      return {
        type: "conflict",
        checkpointId: checkpoint.id,
        path,
        message: planned.reason,
      };
    }
    const action = planned.action;
    const preview = await buildReversePreview(path, action, checkpoint);
    if (preview === null) {
      return {
        type: "failed",
        checkpointId: checkpoint.id,
        path,
        message: "The reverse preview could not be produced; undo was not applied.",
      };
    }
    const requestId = `undo-${(approvalCounter += 1)}`;
    const digest = hashMutationPlan({
      relativePath: path,
      operation: action === "create" ? "create" : action === "delete" ? "delete" : "update",
      beforeSha256: checkpoint.after.exists ? (checkpoint.after.sha256 ?? null) : null,
      afterSha256: action === "delete" ? null : (checkpoint.before.sha256 ?? null),
    });
    const approvalRequest: ApprovalRequest = {
      id: requestId,
      capability: "workspace.write",
      toolName: "solaris.undo",
      summary: `Checkpoint ${checkpoint.id} (${checkpoint.operation} by ${checkpoint.toolName})`,
      paths: [path],
      preview,
      digest,
    };
    let decision;
    try {
      decision = await dependencies.reviewer.review(approvalRequest, signal);
    } catch {
      decision = { type: "deny", reason: "The approval reviewer failed; undo was denied." };
    }
    if (decision.type !== "approve_once") {
      if (decision.type === "cancelled") {
        return { type: "cancelled", checkpointId: checkpoint.id, path };
      }
      return { type: "denied", checkpointId: checkpoint.id, path };
    }
    let release: () => void;
    try {
      release = await dependencies.lock.acquire(signal);
    } catch {
      return { type: "cancelled", checkpointId: checkpoint.id, path };
    }
    try {
      const conflict = await revalidateAfterState(checkpoint);
      if (conflict !== null) {
        return { type: "conflict", checkpointId: checkpoint.id, path, message: conflict };
      }
      if (signal?.aborted) {
        return { type: "cancelled", checkpointId: checkpoint.id, path };
      }
      let preimage: Uint8Array | null = null;
      if (action !== "delete") {
        preimage = await dependencies.store.loadPreimage(checkpoint.id);
        if (preimage === null) {
          return {
            type: "failed",
            checkpointId: checkpoint.id,
            path,
            message: "The checkpoint preimage is missing; undo was not applied.",
          };
        }
      }
      if (signal?.aborted) {
        return { type: "cancelled", checkpointId: checkpoint.id, path };
      }
      const restoreOutcome = await restore(action, checkpoint, preimage, signal);
      if (restoreOutcome.kind !== "ok") {
        if (restoreOutcome.kind === "cancelled") {
          return { type: "cancelled", checkpointId: checkpoint.id, path };
        }
        return {
          type: "conflict",
          checkpointId: checkpoint.id,
          path,
          message: `Undo could not be applied: ${restoreOutcome.message}`,
        };
      }
      try {
        await dependencies.store.markUndone(checkpoint.id);
      } catch (error: unknown) {
        const quarantine = restoreOutcome.quarantinePath;
        const recoverable =
          quarantine === null
            ? ""
            : ` The original copy is preserved at ${quarantine}; do not delete it.`;
        return {
          type: "failed",
          checkpointId: checkpoint.id,
          path,
          message: `Undo was applied but the checkpoint could not be marked undone: ${describeError(error)}. Recovery state is uncertain.${recoverable}`,
        };
      }
      if (restoreOutcome.quarantinePath !== null) {
        try {
          await removeQuarantinedCopy(restoreOutcome.quarantinePath);
        } catch (error: unknown) {
          // The lifecycle state is durably finalized; a failed quarantine
          // removal leaves only a recoverable copy behind, never a loss.
          return {
            type: "undone",
            checkpointId: checkpoint.id,
            path,
            message: `Undo applied; the quarantine copy at ${restoreOutcome.quarantinePath} could not be removed: ${describeError(error)}`,
          };
        }
      }
      return { type: "undone", checkpointId: checkpoint.id, path };
    } finally {
      release();
    }
  }

  async function revalidateAfterState(checkpoint: FileCheckpoint): Promise<string | null> {
    const current = await readWorkspaceFileState(
      dependencies.workspaceRoot,
      checkpoint.relativePath,
    );
    const after: WorkspaceFileState = {
      exists: checkpoint.after.exists,
      sha256: checkpoint.after.sha256,
    };
    if (current.exists !== after.exists || (after.exists && current.sha256 !== after.sha256)) {
      return "The file changed after the proposal was approved; undo would overwrite newer work.";
    }
    return null;
  }

  type RestoreOutcome =
    | { readonly kind: "ok"; readonly quarantinePath: string | null }
    | { readonly kind: "cancelled" }
    | { readonly kind: "failed"; readonly message: string };

  async function restore(
    action: UndoAction,
    checkpoint: FileCheckpoint,
    preimage: Uint8Array | null,
    signal?: AbortSignal,
  ): Promise<RestoreOutcome> {
    const absolute = joinAbsolute(dependencies.workspaceRoot, checkpoint.relativePath);
    if (action === "delete") {
      const resolved = await resolveMutationTarget(
        dependencies.workspaceRoot,
        checkpoint.relativePath,
      );
      if (resolved.status !== "resolved") {
        return { kind: "failed", message: resolved.message };
      }
      if (signal?.aborted) {
        return { kind: "cancelled" };
      }
      if (dependencies.beforeCommitOpen !== undefined) {
        await dependencies.beforeCommitOpen();
      }
      const commitOutcome = await unlinkWithIdentityVerification({
        targetPath: absolute,
        expectedTargetSha256: checkpoint.after.sha256,
        // The parent chain is re-verified immediately before the
        // displacement rename, and the displaced object's identity is
        // re-proven after it, so a parent swapped in the final window can
        // never delete anything outside the workspace.
        verifyParentIdentity: () =>
          verifyParentChainIdentityOrThrow(dependencies.workspaceRoot, absolute),
      });
      if (commitOutcome.kind !== "success") {
        if (commitOutcome.kind === "uncertain") {
          return {
            kind: "failed",
            message: `${commitOutcome.message} The quarantine copy at ${commitOutcome.quarantinePath} must not be deleted; recover the original from it.`,
          };
        }
        return { kind: "failed", message: commitOutcome.message };
      }
      let stillExists = true;
      try {
        await lstat(absolute);
      } catch {
        stillExists = false;
      }
      if (stillExists) {
        return { kind: "failed", message: "The file still exists after undo deletion." };
      }
      return { kind: "ok", quarantinePath: null };
    }
    if (preimage === null) {
      return { kind: "failed", message: "The preimage is missing." };
    }
    if (action === "create") {
      const resolved = await resolveCreateTarget(
        dependencies.workspaceRoot,
        checkpoint.relativePath,
      );
      if (resolved.status !== "resolved") {
        return { kind: "failed", message: resolved.message };
      }
      if (signal?.aborted) {
        return { kind: "cancelled" };
      }
      let canonicalRoot: string;
      try {
        canonicalRoot = await realpath(dependencies.workspaceRoot);
      } catch (error: unknown) {
        return { kind: "failed", message: describeError(error) };
      }
      // Exclusive restoration is not no-follow on intermediate components:
      // a parent swapped for a symlink or junction after the final
      // revalidation would restore the preimage outside the workspace. The
      // parent chain identity is re-verified immediately before the open;
      // Node offers no openat-style dirfd primitive, so a swap in the
      // remaining window is detected AFTER the open but BEFORE any byte is
      // written, through the opened handle, and the exact created object is
      // removed only when identity-provable.
      const parentChain = await verifyParentChainIdentity(dependencies.workspaceRoot, absolute);
      if (!parentChain.ok) {
        return { kind: "failed", message: parentChain.message };
      }
      if (signal?.aborted) {
        return { kind: "cancelled" };
      }
      if (dependencies.beforeCommitOpen !== undefined) {
        await dependencies.beforeCommitOpen();
      }
      let handle;
      try {
        handle = await open(absolute, "wx");
      } catch (error: unknown) {
        return { kind: "failed", message: describeError(error) };
      }
      const openedIdentity = await verifyExclusiveOpenIdentity(handle, absolute, canonicalRoot);
      if (!openedIdentity.ok) {
        await handle.close().catch(() => {});
        if (dependencies.beforeObjectCleanup !== undefined) {
          await dependencies.beforeObjectCleanup();
        }
        const cleanup = await removeCreatedObjectIfSame(
          absolute,
          openedIdentity.dev,
          openedIdentity.ino,
        );
        if (cleanup === "preserved") {
          return {
            kind: "failed",
            message: `${openedIdentity.message}; nothing was written. The created object could not be identity-proven and was left in place; it was not deleted.`,
          };
        }
        if (cleanup === "absent") {
          return {
            kind: "failed",
            message: `${openedIdentity.message}; nothing was written and no stray object remained.`,
          };
        }
        return {
          kind: "failed",
          message: `${openedIdentity.message}; nothing was written and the created object was removed.`,
        };
      }
      try {
        // Write only through the verified handle: the path is never
        // reopened, so a substitution made after the open cannot redirect
        // the restore.
        await handle.writeFile(preimage);
        await handle.sync();
      } finally {
        await handle.close();
      }
    } else {
      const resolved = await resolveMutationTarget(
        dependencies.workspaceRoot,
        checkpoint.relativePath,
      );
      if (resolved.status !== "resolved") {
        return { kind: "failed", message: resolved.message };
      }
      const tempPath = createMutationTempPath(path.dirname(absolute));
      let tempIdentity: { readonly dev: number; readonly ino: number } | undefined;
      let quarantinePath: string | null = null;
      try {
        // The staging open follows intermediate links; the parent chain is
        // re-verified immediately before it so a parent swapped since the
        // revalidation can never redirect the staged write outside the
        // workspace.
        await verifyParentChainIdentityOrThrow(dependencies.workspaceRoot, absolute);
        const handle = await open(tempPath, "wx");
        try {
          const stagedStats = await handle.stat();
          tempIdentity = { dev: stagedStats.dev, ino: stagedStats.ino };
          await handle.writeFile(preimage);
          await handle.sync();
        } finally {
          await handle.close();
        }
        if (signal?.aborted) {
          return { kind: "cancelled" };
        }
        if (dependencies.beforeCommitOpen !== undefined) {
          await dependencies.beforeCommitOpen();
        }
        const commitOutcome = await replaceFileWithQuarantine({
          tempPath,
          targetPath: absolute,
          expectedTargetSha256: checkpoint.after.sha256,
          // The staged preimage bytes are verified by hash again after the
          // commit, so a staged file tampered between staging and the
          // exclusive link is detected before the restore is reported.
          expectedStagedSha256: createHash("sha256").update(preimage).digest("hex"),
          // The parent chain is re-verified immediately before the
          // displacement rename, and the displaced object's identity is
          // re-proven after it, so a parent swapped in the final window can
          // never restore anything outside the workspace.
          verifyParentIdentity: () =>
            verifyParentChainIdentityOrThrow(dependencies.workspaceRoot, absolute),
        });
        if (commitOutcome.kind !== "success") {
          if (commitOutcome.kind === "uncertain") {
            return {
              kind: "failed",
              message: `${commitOutcome.message} The quarantine copy at ${commitOutcome.quarantinePath} must not be deleted; recover the original from it.`,
            };
          }
          return { kind: "failed", message: commitOutcome.message };
        }
        quarantinePath = commitOutcome.quarantinePath;
        const verified = await verifyRestored(absolute, checkpoint);
        if (verified !== null) {
          return {
            kind: "failed",
            message: `${verified} The original copy is preserved at ${quarantinePath}; do not delete it.`,
          };
        }
      } finally {
        await removeMutationTemp(tempPath, tempIdentity).catch(() => {});
      }
      return { kind: "ok", quarantinePath };
    }
    const verified = await verifyRestored(absolute, checkpoint);
    if (verified !== null) {
      return { kind: "failed", message: verified };
    }
    return { kind: "ok", quarantinePath: null };
  }

  async function verifyRestored(
    absolute: string,
    checkpoint: FileCheckpoint,
  ): Promise<string | null> {
    let bytes: Buffer;
    try {
      bytes = await readFile(absolute);
    } catch (error: unknown) {
      return `Post-restore verification could not read the file: ${describeError(error)}`;
    }
    if (checkpoint.before.sha256 === null) {
      return "Post-restore verification failed: the expected hash is missing.";
    }
    const hash = createHash("sha256").update(bytes).digest("hex");
    if (hash !== checkpoint.before.sha256) {
      return "Post-restore verification failed: the restored bytes do not match the checkpoint.";
    }
    return null;
  }

  async function buildReversePreview(
    path: string,
    action: UndoAction,
    checkpoint: FileCheckpoint,
  ): Promise<ChangePreview | null> {
    const current = await readWorkspaceFileState(dependencies.workspaceRoot, path);
    let currentText = "";
    if (current.exists) {
      const bytes = await readFile(joinAbsolute(dependencies.workspaceRoot, path));
      const decoded = decodeUtf8(bytes);
      if (decoded === null) {
        return null;
      }
      currentText = decoded;
    }
    let beforeText = "";
    if (action !== "delete") {
      const preimage = await dependencies.store.loadPreimage(checkpoint.id);
      if (preimage === null) {
        return null;
      }
      const decoded = decodeUtf8(Buffer.from(preimage));
      if (decoded === null) {
        return null;
      }
      beforeText = decoded;
    }
    const diff = buildUnifiedDiff(
      path,
      action === "create" ? "" : currentText,
      action === "delete" ? "" : beforeText,
    );
    if (diff.status === "too_large") {
      return null;
    }
    const operation: "create" | "update" | "delete" =
      action === "create" ? "create" : action === "delete" ? "delete" : "update";
    const filePreview = {
      path,
      operation,
      beforeSha256:
        action === "delete" ? current.sha256 : (checkpoint.before.sha256 ?? current.sha256),
      afterSha256: action === "create" ? (checkpoint.before.sha256 ?? null) : null,
      addedLines: diff.diff.addedLines,
      removedLines: diff.diff.removedLines,
      unifiedDiff: diff.diff.unifiedDiff,
    };
    return {
      files: [filePreview],
      totalAddedLines: diff.diff.addedLines,
      totalRemovedLines: diff.diff.removedLines,
      truncated: false,
    };
  }

  return { undo };
}

function joinAbsolute(workspaceRoot: string, relativePath: string): string {
  return path.join(workspaceRoot, ...relativePath.split("/"));
}

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "An unknown undo failure occurred.";
}
