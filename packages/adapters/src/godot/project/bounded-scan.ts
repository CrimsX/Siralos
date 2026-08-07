import { lstat, readdir } from "node:fs/promises";
import { join, relative } from "node:path";
import { GODOT_LIMITS } from "@solaris/core";

export const PROJECT_SCAN_EXCLUDED_DIRECTORIES: readonly string[] = [
  ".git",
  ".godot",
  "node_modules",
  "dist",
  "coverage",
  ".solaris",
];

export interface BoundedScanOptions {
  readonly workspaceRoot: string;
  readonly signal?: AbortSignal;
  readonly maxFiles?: number;
  readonly timeoutMs?: number;
  /** When true, only surface `.gd` files (used by the tool-script scan). */
  readonly onlyGdFiles?: boolean;
  /** When true, also surface `.cs` files and root `.csproj`/`.sln` files. */
  readonly includeDotnet?: boolean;
  /** When true, also surface `.gdextension` descriptors. */
  readonly includeGDExtensions?: boolean;
}

export interface BoundedScanResult {
  /** Absolute paths of surfaced files. */
  readonly files: readonly string[];
  readonly scannedFileCount: number;
  readonly truncated: boolean;
}

/**
 * Bounded, symlink-safe project traversal. Excluded directories are never
 * entered, symbolic links are never followed, the file count is bounded,
 * and the deadline is checked during traversal so cancellation and the
 * static scan timeout are honored.
 */
export async function scanProjectFiles(options: BoundedScanOptions): Promise<BoundedScanResult> {
  const maxFiles = options.maxFiles ?? GODOT_LIMITS.maxProjectFilesScanned;
  const timeoutMs = options.timeoutMs ?? GODOT_LIMITS.staticProjectScanTimeoutMs;
  const deadline = Date.now() + timeoutMs;
  const files: string[] = [];
  let scanned = 0;
  let truncated = false;

  async function walk(directory: string): Promise<void> {
    if (truncated) {
      return;
    }
    if (options.signal?.aborted) {
      throw createAbortError();
    }
    if (Date.now() > deadline) {
      truncated = true;
      return;
    }
    let entries: readonly { readonly name: string; readonly isDirectory: () => boolean }[];
    try {
      entries = await readdir(directory, { withFileTypes: true });
    } catch {
      return;
    }
    const sorted = [...entries].sort((left, right) => left.name.localeCompare(right.name));
    for (const entry of sorted) {
      if (truncated || scanned >= maxFiles) {
        truncated = true;
        return;
      }
      if (PROJECT_SCAN_EXCLUDED_DIRECTORIES.includes(entry.name)) {
        continue;
      }
      const full = join(directory, entry.name);
      let metadata;
      try {
        metadata = await lstat(full);
      } catch {
        continue;
      }
      if (metadata.isSymbolicLink()) {
        continue;
      }
      if (metadata.isDirectory()) {
        await walk(full);
        continue;
      }
      if (!metadata.isFile()) {
        continue;
      }
      scanned += 1;
      const extension = entry.name.toLowerCase().split(".").pop() ?? "";
      const isGd = extension === "gd";
      const isCs = options.includeDotnet === true && extension === "cs";
      const isGDExtension = options.includeGDExtensions === true && extension === "gdextension";
      const isRootProjectFile =
        options.includeDotnet === true &&
        (extension === "csproj" || extension === "sln") &&
        relative(options.workspaceRoot, full).split(/[\\/]/).length === 1;
      if (isGd && options.onlyGdFiles !== true) {
        files.push(full);
      } else if (isCs || isGDExtension || isRootProjectFile) {
        files.push(full);
      }
    }
  }

  await walk(options.workspaceRoot);
  return { files, scannedFileCount: scanned, truncated };
}

function createAbortError(): Error {
  return new DOMException("The project scan was aborted.", "AbortError");
}
