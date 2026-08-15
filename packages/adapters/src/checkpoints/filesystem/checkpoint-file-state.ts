import { createHash } from "node:crypto";
import { lstat, realpath } from "node:fs/promises";
import { join, sep } from "node:path";
import type { WorkspaceFileState } from "@siralos/core";
import { validateRelativeWorkspacePath } from "../../tools/workspace/mutations/mutation-paths.js";
import { readFileBounded } from "../../fs/file-read.js";

export const DEFAULT_MAX_STATE_BYTES = 1024 * 1024;

/**
 * Read the exact current workspace file state (exists + SHA-256) for a
 * checkpoint record's relative path.
 *
 * Containment: the record's relative path is lexically validated, then
 * the canonical parent directory must resolve inside the canonical
 * workspace root. A parent symlink/junction/reparse escape fails closed
 * (`{ exists: true, sha256: null }`) exactly like a linked target, so a
 * corrupted or malicious checkpoint record can never cause inspection
 * outside the intended workspace. The leaf is lstat-verified without
 * following and read through the bounded complete-read primitive.
 */
export async function readWorkspaceFileState(
  workspaceRoot: string,
  relativePath: string,
  maxStateBytes: number = DEFAULT_MAX_STATE_BYTES,
): Promise<WorkspaceFileState> {
  const validation = validateRelativeWorkspacePath(relativePath);
  if (validation !== null) {
    return { exists: true, sha256: null };
  }
  let canonicalRoot: string;
  try {
    canonicalRoot = await realpath(workspaceRoot);
  } catch {
    return { exists: false, sha256: null };
  }
  const separator = relativePath.lastIndexOf("/");
  const parent = separator === -1 ? "." : relativePath.slice(0, separator);
  const leaf = separator === -1 ? relativePath : relativePath.slice(separator + 1);
  let canonicalParent: string;
  try {
    canonicalParent = await realpath(
      join(canonicalRoot, ...(parent === "." ? [] : parent.split("/"))),
    );
  } catch {
    return { exists: false, sha256: null };
  }
  const rootPrefix = canonicalRoot.endsWith(sep) ? canonicalRoot : `${canonicalRoot}${sep}`;
  if (canonicalParent !== canonicalRoot && !canonicalParent.startsWith(rootPrefix)) {
    // A parent symlink/junction/reparse escape fails closed: the record
    // must never cause inspection outside the workspace.
    return { exists: true, sha256: null };
  }
  const absolute = join(canonicalParent, leaf);
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
  // The read is a bounded complete loop: a file grown or swapped after
  // the lstat is never fully materialized, never reported complete, and
  // a FIFO substitution can never block it.
  const bytes = await readFileBounded(absolute, maxStateBytes);
  if (bytes === null) {
    return { exists: true, sha256: null };
  }
  return { exists: true, sha256: createHash("sha256").update(bytes).digest("hex") };
}
