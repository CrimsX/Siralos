import { createHash } from "node:crypto";
import { lstat } from "node:fs/promises";
import { join } from "node:path";
import type { WorkspaceFileState } from "@siralos/core";
import { validateRelativeWorkspacePath } from "../../tools/workspace/mutations/mutation-paths.js";
import { readFileBounded } from "../../fs/file-read.js";

export const DEFAULT_MAX_STATE_BYTES = 1024 * 1024;

export async function readWorkspaceFileState(
  workspaceRoot: string,
  relativePath: string,
  maxStateBytes: number = DEFAULT_MAX_STATE_BYTES,
): Promise<WorkspaceFileState> {
  const validation = validateRelativeWorkspacePath(relativePath);
  if (validation !== null) {
    return { exists: true, sha256: null };
  }
  const absolute = join(workspaceRoot, ...relativePath.split("/"));
  let stats;
  try {
    stats = await lstat(absolute);
  } catch {
    return { exists: false, sha256: null };
  }
  if (stats.isSymbolicLink() || !stats.isFile()) {
    return { exists: true, sha256: null };
  }
  if (stats.size > maxStateBytes) {
    return { exists: true, sha256: null };
  }
  // The read is capped: a file grown or swapped after the lstat is never
  // fully materialized and a FIFO substitution can never block it.
  const bytes = await readFileBounded(absolute, maxStateBytes);
  if (bytes === null) {
    return { exists: true, sha256: null };
  }
  return { exists: true, sha256: createHash("sha256").update(bytes).digest("hex") };
}
