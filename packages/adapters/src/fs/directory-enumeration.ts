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
 * `fs.promises.rm(dir, { recursive: true })` is unbounded: a hostile local
 * writer can fill the tree with an arbitrary number of entries and the
 * removal cannot be interrupted. This helper walks the tree iteratively
 * (never recursing through the call stack), refuses to follow symbolic
 * links or junctions (the leaf must be a real directory before its children
 * are enumerated; a link planted mid-walk is removed as a leaf, never
 * followed), counts every entry examined, and fails closed with an error
 * when the entry budget is exceeded so the caller reports instead of
 * deleting without bound. The caller must have already verified that `root`
 * itself is a real, non-link directory.
 */
export async function removeDirectoryTreeBounded(root: string, maxEntries: number): Promise<void> {
  type Work = { readonly path: string; readonly phase: "enter" | "exit" };
  const pending: Work[] = [{ path: root, phase: "enter" }];
  let examined = 0;
  while (pending.length > 0) {
    const current = pending.pop();
    if (current === undefined) {
      break;
    }
    examined += 1;
    if (examined > maxEntries) {
      throw new Error(
        `Directory removal exceeded the ${maxEntries}-entry budget; the tree was preserved for manual inspection.`,
      );
    }
    if (current.phase === "exit") {
      await rmdir(current.path).catch(() => undefined);
      continue;
    }
    let metadata;
    try {
      metadata = await lstat(current.path);
    } catch {
      continue;
    }
    if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
      // A link or non-directory leaf is removed as a leaf, never followed.
      await unlink(current.path).catch(() => undefined);
      continue;
    }
    let handle;
    try {
      handle = await opendir(current.path);
    } catch {
      continue;
    }
    // Post-order: the "exit" item is pushed before the children so the
    // children (pushed later) pop first and the directory is removed only
    // after its subtree.
    pending.push({ path: current.path, phase: "exit" });
    try {
      for await (const entry of handle) {
        pending.push({ path: join(current.path, entry.name), phase: "enter" });
      }
    } finally {
      await handle.close().catch(() => undefined);
    }
  }
}
