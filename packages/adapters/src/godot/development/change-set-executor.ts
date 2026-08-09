import type {
  ChangeSetApplyOutcome,
  ChangeSetApplyRequest,
  ChangeSetFilePrimitives,
  CheckpointStore,
  DevelopmentChangeSetApplier,
} from "@solaris/core";

export const CHANGE_SET_EXECUTION_UNAVAILABLE_MESSAGE =
  "The exact change set cannot be applied on this platform: Node offers no directory-relative (openat/renameat) primitive, so a same-user process that swaps a parent or target at any instruction boundary can redirect pathname-based staging and replacement outside the workspace. The change set fails closed before any write, lock, approval, or checkpoint; it will become available when a mechanically identity-bound commit primitive exists.";

/**
 * Change-set executor (§22–§24).
 *
 * The apply protocol: acquire the serialized mutation lock, revalidate
 * every source precondition (expected pre-state hashes), create a
 * checkpoint for every affected existing file (with the exact pre-change
 * bytes) and an absence state for every create, verify all checkpoints
 * durable, then apply prepared files sequentially with post-state hash
 * verification — the lock is never released between files. A partial
 * infrastructure failure triggers internal recovery of the files Solaris
 * just changed from their just-created checkpoint preimages (hash-gated:
 * only files whose current state still matches the partially applied
 * result are restored, external changes are preserved, conflicts are
 * reported), and success is never reported after partial application.
 *
 * At this stage every platform gate fails closed: `isAvailable()` is
 * false and `apply` refuses with a typed `unavailable` outcome before
 * acquiring the lock, recording a checkpoint, or touching a file. The
 * protocol below is tested internal code exercised through injected
 * in-memory file primitives (the production composition injects a
 * fail-closed primitives implementation that performs zero filesystem
 * operations).
 */

export interface ChangeSetExecutorDependencies {
  readonly store: CheckpointStore;
  readonly lock: {
    acquire(signal?: AbortSignal): Promise<() => void>;
  };
  /** Tool name recorded on every checkpoint (the change-set tool). */
  readonly toolName: string;
  /** True only when the platform can mechanically bind every write. */
  readonly canApplyIdentityBound: boolean;
}

export function createDevelopmentChangeSetApplier(
  dependencies: ChangeSetExecutorDependencies,
): DevelopmentChangeSetApplier {
  return {
    isAvailable(): Promise<boolean> {
      return Promise.resolve(dependencies.canApplyIdentityBound);
    },
    apply(
      request: ChangeSetApplyRequest,
      primitives: ChangeSetFilePrimitives,
    ): Promise<ChangeSetApplyOutcome> {
      if (!dependencies.canApplyIdentityBound) {
        return Promise.resolve({
          status: "unavailable",
          message: CHANGE_SET_EXECUTION_UNAVAILABLE_MESSAGE,
          checkpointIds: [],
        });
      }
      return applyChangeSetProtocol(request, primitives, dependencies);
    },
  };
}

/**
 * The checkpoint-then-apply protocol with partial-failure recovery.
 * Pure orchestration over the injected primitives and the checkpoint
 * store; the caller has already passed the identity-bound gate.
 */
export async function applyChangeSetProtocol(
  request: ChangeSetApplyRequest,
  primitives: ChangeSetFilePrimitives,
  dependencies: ChangeSetExecutorDependencies,
): Promise<ChangeSetApplyOutcome> {
  if (request.signal?.aborted) {
    return {
      status: "cancelled",
      message: "The change-set application was cancelled.",
      checkpointIds: [],
      appliedFiles: [],
    };
  }
  const release = await dependencies.lock.acquire(request.signal);
  const checkpointIds: string[] = [];
  const checkpointByPath = new Map<string, string>();
  const applied: string[] = [];
  try {
    // Revalidate every precondition before any checkpoint.
    for (const file of request.files) {
      const current = await primitives.readFile(file.path);
      if (file.operation === "create") {
        if (current.exists) {
          return {
            status: "conflict",
            message: `"${file.path}" appeared since preparation; the change set was not applied.`,
            path: file.path,
          };
        }
        continue;
      }
      if (!current.exists || current.sha256 !== file.expectedSha256) {
        return {
          status: "conflict",
          message: `"${file.path}" changed since preparation; the change set was not applied.`,
          path: file.path,
        };
      }
    }
    // Checkpoint every affected file (with its exact pre-change bytes,
    // and an absence state for every create) before anything is applied.
    for (const file of request.files) {
      const preimage = file.operation === "create" ? null : await primitives.readContent(file.path);
      const before = file.operation === "create" ? null : (preimage?.content ?? null);
      const checkpoint = await dependencies.store.prepare(
        {
          relativePath: file.path,
          operation: file.operation,
          toolName: dependencies.toolName,
          before: {
            exists: file.operation === "create" ? false : true,
            sha256: file.beforeSha256,
            byteLength: before === null ? null : utf8ByteLength(before),
            bytes: before === null ? null : new TextEncoder().encode(before),
          },
          after: {
            exists: file.operation === "delete" ? false : true,
            sha256: file.afterSha256,
            byteLength:
              file.operation === "delete" || file.content === null
                ? null
                : utf8ByteLength(file.content),
          },
          preview: { addedLines: file.addedLines, removedLines: file.removedLines },
        },
        request.signal,
      );
      checkpointIds.push(checkpoint.id);
      checkpointByPath.set(file.path, checkpoint.id);
    }
    // Apply files sequentially; the lock is never released in between.
    for (const file of request.files) {
      if (request.signal?.aborted) {
        return {
          status: "cancelled",
          message: "The change-set application was cancelled.",
          checkpointIds,
          appliedFiles: [...applied],
        };
      }
      if (file.operation === "delete") {
        await primitives.deleteFile(file.path, request.signal);
      } else {
        await primitives.writeFile(file.path, file.content ?? "", request.signal);
      }
      applied.push(file.path);
      const current = await primitives.readFile(file.path);
      if (file.operation === "delete" ? current.exists : current.sha256 !== file.afterSha256) {
        throw new Error(`"${file.path}" does not match the prepared post-state after application.`);
      }
    }
    return { status: "applied", checkpointIds };
  } catch (error: unknown) {
    if (request.signal?.aborted) {
      return {
        status: "cancelled",
        message: "The change-set application was cancelled.",
        checkpointIds,
        appliedFiles: [...applied],
      };
    }
    if (applied.length === 0) {
      const message = error instanceof Error ? error.message : "The change-set application failed.";
      return { status: "failed", message, checkpointIds };
    }
    const message = error instanceof Error ? error.message : "The change-set application failed.";
    return recoverPartialApplication(
      request,
      primitives,
      dependencies,
      checkpointByPath,
      applied,
      message,
    );
  } finally {
    release();
  }
}

/**
 * Internal recovery of a partially applied change set (§24): restores
 * only the files Solaris just changed, each gated on its current hash
 * still matching the partially applied result, using the just-created
 * checkpoint preimages. External changes are preserved and reported; the
 * final state is never success after partial application.
 */
async function recoverPartialApplication(
  request: ChangeSetApplyRequest,
  primitives: ChangeSetFilePrimitives,
  dependencies: ChangeSetExecutorDependencies,
  checkpointByPath: Map<string, string>,
  applied: readonly string[],
  failureMessage: string,
): Promise<ChangeSetApplyOutcome> {
  const restored: string[] = [];
  const conflicted: string[] = [];
  for (const filePath of applied) {
    const file = request.files.find((entry) => entry.path === filePath);
    if (file === undefined) {
      conflicted.push(filePath);
      continue;
    }
    try {
      const current = await primitives.readFile(filePath);
      // "Partially applied" means the delete already removed the file (it
      // must then be restored from the preimage) or an update/create wrote
      // its post-state. Anything else is an external change: preserved.
      const partiallyApplied =
        file.operation === "delete" ? !current.exists : current.sha256 === file.afterSha256;
      if (!partiallyApplied) {
        conflicted.push(filePath);
        continue;
      }
      if (file.operation === "create") {
        await primitives.deleteFile(filePath, request.signal);
        restored.push(filePath);
        continue;
      }
      const checkpointId = checkpointByPath.get(filePath);
      if (checkpointId === undefined) {
        conflicted.push(filePath);
        continue;
      }
      const preimage = await dependencies.store.loadPreimage(checkpointId);
      if (preimage === null) {
        conflicted.push(filePath);
        continue;
      }
      await primitives.writeFile(filePath, new TextDecoder().decode(preimage), request.signal);
      restored.push(filePath);
    } catch {
      conflicted.push(filePath);
    }
  }
  const checkpointIds = [...checkpointByPath.values()];
  if (restored.length === applied.length && conflicted.length === 0) {
    return {
      status: "apply_failed_recovered",
      message: `The change-set application failed partway (${failureMessage}); every file Solaris had changed was restored from its checkpoint.`,
      checkpointIds,
    };
  }
  if (restored.length > 0) {
    return {
      status: "apply_failed_partial_recovery",
      message: `The change-set application failed partway (${failureMessage}); ${restored.length} file(s) were restored, but ${conflicted.length} file(s) could not be restored because their current state no longer matches the partially applied result.`,
      checkpointIds,
    };
  }
  return {
    status: "apply_failed_uncertain",
    message: `The change-set application failed partway (${failureMessage}); no file could be restored because the current state no longer matches the partially applied result. The checkpoints remain for manual recovery.`,
    checkpointIds,
  };
}

function utf8ByteLength(text: string): number {
  return new TextEncoder().encode(text).length;
}

/**
 * Production file primitives: zero filesystem operations. Every call
 * fails closed because Node offers no directory-relative (openat/renameat)
 * primitive and a same-user process can swap a parent or target at any
 * instruction boundary. The production applier refuses before invoking
 * these; tests inject in-memory primitives to exercise the protocol.
 */
export function createFailClosedChangeSetFilePrimitives(): ChangeSetFilePrimitives {
  const refuse = (): Promise<never> => {
    return Promise.reject(new Error(CHANGE_SET_EXECUTION_UNAVAILABLE_MESSAGE));
  };
  return {
    readFile: refuse,
    readContent: refuse,
    writeFile: refuse,
    deleteFile: refuse,
  };
}
