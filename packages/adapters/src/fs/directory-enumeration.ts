import { lstat, opendir, rmdir, unlink } from "node:fs/promises";
import type { Dirent } from "node:fs";
import { join } from "node:path";

/**
 * Incremental bounded directory enumeration.
 *
 * `fs.promises.readdir` materializes the complete entry array before any
 * bound can be consulted, so a hostile directory with millions of entries
 * exhausts memory before an entry cap ever runs. This helper reads entries
 * one at a time through a directory handle, applies the entry cap to the
 * examined count (excluded and non-regular entries count), checks
 * cancellation and deadline per entry, and always closes the handle.
 */
export type BoundedEnumerationOutcome = {
  readonly entriesExamined: number;
  readonly truncated: boolean;
  readonly missing: boolean;
};

export async function enumerateDirectoryBounded(options: {
  readonly directory: string;
  readonly maxEntries: number;
  readonly signal?: AbortSignal | undefined;
  /** Absolute epoch-millisecond deadline; enumeration stops when exceeded. */
  readonly deadline?: number | undefined;
  readonly onEntry: (entry: Dirent, index: number) => void;
}): Promise<BoundedEnumerationOutcome> {
  let handle;
  try {
    handle = await opendir(options.directory);
  } catch (error: unknown) {
    if (isNotFoundError(error)) {
      return { entriesExamined: 0, truncated: false, missing: true };
    }
    throw error;
  }
  try {
    let index = 0;
    for await (const entry of handle) {
      if (index >= options.maxEntries) {
        return { entriesExamined: index, truncated: true, missing: false };
      }
      if (options.signal?.aborted) {
        throw new DOMException("Directory enumeration was aborted.", "AbortError");
      }
      if (options.deadline !== undefined && Date.now() > options.deadline) {
        return { entriesExamined: index, truncated: true, missing: false };
      }
      options.onEntry(entry, index);
      index += 1;
    }
    return { entriesExamined: index, truncated: false, missing: false };
  } finally {
    await handle.close().catch(() => undefined);
  }
}

function isNotFoundError(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}

/**
 * Bounded no-follow recursive deletion of a Solaris-owned directory tree.
 *
 * Deletion is TWO-PHASE so a refused operation never partially deletes:
 *
 * 1. Plan: the tree is walked iteratively (never recursing through the call
 *    stack) without mutating anything, recording every entry with its type
 *    (symbolic links and junctions are recorded as leaves, never followed)
 *    and counting entries. If the entry budget is exceeded, the plan throws
 *    BEFORE any deletion, so zero entries are removed.
 * 2. Delete: the accepted plan is executed in post-order (children before
 *    parents). A failure during execution (for example a directory that
 *    became non-empty because the tree changed between phases) fails closed
 *    with an error and the remaining tree is preserved for inspection.
 *
 * The caller must have already verified that `root` itself is the exact
 * Solaris-created object (identity check) and is a real, non-link directory.
 */
export async function removeDirectoryTreeBounded(root: string, maxEntries: number): Promise<void> {
  const plan = await planDirectoryRemoval(root, maxEntries);
  // The plan records parents before children, so reverse order deletes
  // deepest entries first; a directory is removed only after its subtree.
  for (let index = plan.entries.length - 1; index >= 0; index -= 1) {
    const entry = plan.entries[index];
    if (entry === undefined) {
      continue;
    }
    if (entry.isDirectory) {
      await rmdir(entry.path).catch((error: unknown) => {
        throw new Error(
          `Directory removal failed at ${entry.path}: ${describeRemovalError(error)}; the remaining tree was preserved for manual inspection.`,
        );
      });
    } else {
      await unlink(entry.path).catch((error: unknown) => {
        throw new Error(
          `Directory removal failed at ${entry.path}: ${describeRemovalError(error)}; the remaining tree was preserved for manual inspection.`,
        );
      });
    }
  }
}

export interface DirectoryRemovalPlan {
  /** Parents recorded before children; delete in reverse for post-order. */
  readonly entries: readonly { readonly path: string; readonly isDirectory: boolean }[];
  readonly examined: number;
}

/**
 * Phase one of bounded removal: enumerates the full tree without mutating
 * anything and records every entry, failing closed when the entry budget is
 * exceeded so a refused removal has performed zero deletions. Symbolic
 * links and junctions are recorded as leaves and never followed; a path
 * that cannot be inspected fails the plan closed.
 */
export async function planDirectoryRemoval(
  root: string,
  maxEntries: number,
): Promise<DirectoryRemovalPlan> {
  const entries: { readonly path: string; readonly isDirectory: boolean }[] = [];
  const pending: string[] = [root];
  while (pending.length > 0) {
    const current = pending.pop();
    if (current === undefined) {
      break;
    }
    if (entries.length >= maxEntries) {
      throw new Error(
        `Directory removal exceeded the ${maxEntries}-entry budget; the plan was refused before any deletion, so zero entries were removed.`,
      );
    }
    let metadata;
    try {
      metadata = await lstat(current);
    } catch (error: unknown) {
      throw new Error(
        `Directory removal could not inspect ${current} (${describeRemovalError(error)}); the plan was refused before any deletion.`,
      );
    }
    if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
      // A link or non-directory leaf is recorded as a leaf; it is never
      // followed during planning and removed by unlink during deletion.
      entries.push({ path: current, isDirectory: false });
      continue;
    }
    entries.push({ path: current, isDirectory: true });
    let handle;
    try {
      handle = await opendir(current);
    } catch (error: unknown) {
      throw new Error(
        `Directory removal could not enumerate ${current} (${describeRemovalError(error)}); the plan was refused before any deletion.`,
      );
    }
    try {
      for await (const entry of handle) {
        pending.push(join(current, entry.name));
      }
    } catch (error: unknown) {
      throw new Error(
        `Directory removal could not enumerate ${current} (${describeRemovalError(error)}); the plan was refused before any deletion.`,
      );
    } finally {
      await handle.close().catch(() => undefined);
    }
  }
  return { entries, examined: entries.length };
}

function describeRemovalError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "an unknown filesystem error occurred";
}
