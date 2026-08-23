import { lstat } from "node:fs/promises";
import { join, relative } from "node:path";
import { GODOT_LIMITS, type GodotScanTruncationReason, type SafeDiagnostic } from "@siralos/core";
import { createTraversalBudget, type TraversalBudget } from "./traversal-limits.js";
import { enumerateDirectoryBounded } from "../../fs/directory-enumeration.js";
import { foldPathComponent } from "../../fs-case.js";

export const PROJECT_SCAN_EXCLUDED_DIRECTORIES: readonly string[] = [
  ".git",
  ".godot",
  "node_modules",
  "dist",
  "coverage",
  ".siralos",
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
  readonly maxDepth?: number;
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
      maxDepth: options.maxDepth ?? GODOT_LIMITS.maxProjectScanDepth,
    });
  const warnings: SafeDiagnostic[] = [];
  const files: string[] = [];

  async function walk(directory: string, depth: number): Promise<void> {
    if (budget.exhausted) {
      return;
    }
    budget.checkCancelled(options.signal);
    if (!budget.isWithinDeadline()) {
      return;
    }
    if (depth > budget.maxDepth) {
      budget.stop("depth-limit");
      return;
    }
    budget.directoriesVisited += 1;
    if (budget.directoriesVisited > budget.maxDirectories) {
      budget.stop("directory-limit");
      return;
    }
    // Entries are enumerated incrementally and collected only up to the
    // remaining entry budget, so a hostile wide directory can never
    // materialize an unbounded listing. The collected set is sorted for
    // deterministic output order; entries beyond the budget stop the walk.
    const remainingEntries = Math.max(0, budget.maxEntries - budget.entriesExamined);
    const collected: Array<{ readonly name: string; readonly isDirectory: () => boolean }> = [];
    let truncatedListing: boolean;
    try {
      const outcome = await enumerateDirectoryBounded({
        directory,
        maxEntries: remainingEntries,
        signal: options.signal,
        deadline: budget.deadline,
        onEntry: (entry) => {
          collected.push({ name: entry.name, isDirectory: () => entry.isDirectory() });
        },
      });
      truncatedListing = outcome.truncated;
    } catch {
      return;
    }
    const sorted = collected.sort((left, right) => left.name.localeCompare(right.name));
    for (const entry of sorted) {
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
      if (
        PROJECT_SCAN_EXCLUDED_DIRECTORIES.some(
          (excluded) => foldPathComponent(excluded) === foldPathComponent(entry.name),
        )
      ) {
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
        await walk(full, depth + 1);
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
    if (truncatedListing) {
      // More entries exist than the remaining entry budget allowed; the
      // listing was capped and the walk is truncated by the entry bound.
      budget.stop("entry-limit");
    }
  }

  await walk(options.workspaceRoot, 0);
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
    case "depth-limit":
      return "The project scan stopped at the maximum directory-depth bound (maxProjectScanDepth); the traversal is partial.";
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
