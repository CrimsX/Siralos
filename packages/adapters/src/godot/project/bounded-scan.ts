import { lstat, readdir } from "node:fs/promises";
import { join, relative } from "node:path";
import { GODOT_LIMITS, type GodotScanTruncationReason, type SafeDiagnostic } from "@solaris/core";
import { createTraversalBudget, type TraversalBudget } from "./traversal-limits.js";

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
  /** Bounds for deterministic tests; defaults come from GODOT_LIMITS. */
  readonly maxDirectories?: number;
  readonly maxEntries?: number;
  readonly maxSurfaced?: number;
  /** When true, only surface `.gd` files (used by the tool-script scan). */
  readonly onlyGdFiles?: boolean;
  /** When true, also surface `.cs` files and root `.csproj`/`.sln` files. */
  readonly includeDotnet?: boolean;
  /** When true, also surface `.gdextension` descriptors. */
  readonly includeGDExtensions?: boolean;
  /** Shared budget carrying the global deadline and counters for all phases. */
  readonly budget?: TraversalBudget;
}

export interface BoundedScanResult {
  /** Absolute paths of surfaced files. */
  readonly files: readonly string[];
  readonly scannedFileCount: number;
  readonly truncated: boolean;
  /** Exact reason for truncation; "none" when the scan completed. */
  readonly truncationReason: GodotScanTruncationReason;
  /** Bound-named warnings emitted when the walk stopped early. */
  readonly warnings: readonly SafeDiagnostic[];
}

/**
 * Bounded, symlink-safe project traversal. Excluded directories are never
 * entered, symbolic links are never followed, every readdir entry consumes
 * the entry budget (excluded and non-regular entries included), directory
 * and surfaced-file counts are bounded independently, and the shared
 * deadline is checked during traversal so cancellation and the static scan
 * timeout are honored across all inventory phases.
 */
export async function scanProjectFiles(options: BoundedScanOptions): Promise<BoundedScanResult> {
  const budget =
    options.budget ??
    createTraversalBudget({
      timeoutMs: options.timeoutMs ?? GODOT_LIMITS.staticProjectScanTimeoutMs,
      maxFiles: options.maxFiles ?? GODOT_LIMITS.maxProjectFilesScanned,
      maxDirectories: options.maxDirectories ?? GODOT_LIMITS.maxProjectDirectoriesVisited,
      maxEntries: options.maxEntries ?? GODOT_LIMITS.maxProjectEntriesExamined,
      maxSurfaced: options.maxSurfaced ?? GODOT_LIMITS.maxProjectFilesSurfaced,
      maxReadBytes: GODOT_LIMITS.maxProjectTotalReadBytes,
      maxPluginDirectories: GODOT_LIMITS.maxProjectPluginDirectories,
      maxDescriptorsParsed: GODOT_LIMITS.maxProjectDescriptorsParsed,
      maxInventoryItems: GODOT_LIMITS.maxProjectInventoryItems,
    });
  const warnings: SafeDiagnostic[] = [];
  const files: string[] = [];

  async function walk(directory: string): Promise<void> {
    if (budget.exhausted) {
      return;
    }
    budget.checkCancelled(options.signal);
    if (!budget.isWithinDeadline()) {
      return;
    }
    budget.directoriesVisited += 1;
    if (budget.directoriesVisited > budget.maxDirectories) {
      budget.stop("directory-limit");
      return;
    }
    let entries: readonly { readonly name: string; readonly isDirectory: () => boolean }[];
    try {
      entries = await readdir(directory, { withFileTypes: true });
    } catch {
      return;
    }
    const sorted = [...entries].sort((left, right) => left.name.localeCompare(right.name));
    // Cap the materialized listing slice to the remaining entry budget so a
    // hostile wide directory can never drive unbounded iteration.
    const remainingEntries = Math.max(0, budget.maxEntries - budget.entriesExamined);
    for (const entry of sorted.slice(0, remainingEntries)) {
      if (budget.exhausted) {
        return;
      }
      budget.checkCancelled(options.signal);
      if (!budget.isWithinDeadline()) {
        return;
      }
      budget.entriesExamined += 1;
      if (budget.entriesExamined > budget.maxEntries) {
        budget.stop("entry-limit");
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
      budget.filesScanned += 1;
      if (budget.filesScanned > budget.maxFiles) {
        budget.stop("file-limit");
        return;
      }
      const extension = entry.name.toLowerCase().split(".").pop() ?? "";
      const isGd = extension === "gd";
      const isCs = options.includeDotnet === true && extension === "cs";
      const isGDExtension = options.includeGDExtensions === true && extension === "gdextension";
      const isRootProjectFile =
        options.includeDotnet === true &&
        (extension === "csproj" || extension === "sln") &&
        relative(options.workspaceRoot, full).split(/[\\/]/).length === 1;
      if ((isGd && options.onlyGdFiles !== true) || isCs || isGDExtension || isRootProjectFile) {
        budget.filesSurfaced += 1;
        if (budget.filesSurfaced > budget.maxSurfaced) {
          budget.stop("surfaced-limit");
          return;
        }
        files.push(full);
      }
    }
    if (sorted.length > remainingEntries) {
      // More entries exist than the remaining entry budget allowed; the
      // listing was capped and the walk is truncated by the entry bound.
      budget.stop("entry-limit");
    }
  }

  await walk(options.workspaceRoot);
  const truncated = budget.reason !== "none";
  if (truncated && budget.reason !== "none") {
    warnings.push({
      severity: "warning",
      message: describeWalkTruncation(budget.reason),
    });
  }
  return {
    files,
    scannedFileCount: budget.filesScanned,
    truncated,
    truncationReason: budget.reason,
    warnings,
  };
}

function describeWalkTruncation(reason: Exclude<GodotScanTruncationReason, "none">): string {
  switch (reason) {
    case "file-limit":
      return "The project scan stopped at the maximum files-scanned bound (maxProjectFilesScanned); the traversal is partial.";
    case "directory-limit":
      return "The project scan stopped at the maximum directories-visited bound (maxProjectDirectoriesVisited); the traversal is partial.";
    case "entry-limit":
      return "The project scan stopped at the maximum entries-examined bound (maxProjectEntriesExamined); the traversal is partial.";
    case "surfaced-limit":
      return "The project scan stopped at the maximum files-surfaced bound (maxProjectFilesSurfaced); the traversal is partial.";
    case "timeout":
      return "The project scan stopped at the static scan deadline (staticProjectScanTimeoutMs); the traversal is partial.";
    case "plugin-limit":
    case "descriptor-limit":
    case "inventory-limit":
    case "bytes-limit":
    case "cancelled":
      return `The project scan stopped because a traversal bound was exhausted (${reason}); the traversal is partial.`;
  }
}
