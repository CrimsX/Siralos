import { opendir } from "node:fs/promises";
import type { Dirent } from "node:fs";

/**
 * Incremental bounded directory enumeration.
 *
 * `fs.promises.readdir` materializes the complete entry array before any
 * bound can be consulted, so a hostile directory with millions of entries
 * exhausts memory before an entry cap ever runs. This helper reads entries
 * one at a time through a directory handle, applies the entry cap to the
 * examined count (excluded and non-regular entries count), checks
 * cancellation and deadline per entry, and always closes the handle.
 *
 * This module is read-only: it enumerates and never deletes, renames, or
 * writes anything. There is deliberately no removal counterpart — Node
 * offers no directory-handle-relative deletion primitive, so identity-bound
 * recursive deletion cannot be implemented safely and is never offered.
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
