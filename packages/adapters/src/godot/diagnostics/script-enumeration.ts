import { lstat } from "node:fs/promises";
import { join, sep } from "node:path";
import { GODOT_LIMITS } from "@solaris/core";
import type { GodotScriptCheckTarget } from "@solaris/core";
import { PROJECT_SCAN_EXCLUDED_DIRECTORIES } from "../project/bounded-scan.js";
import { hashAbsoluteFile, createAbortError } from "../probe/risk-manifest.js";
import {
  DEFAULT_FS_OPS,
  validateProjectRelativePath,
  verifyProjectPathContainment,
} from "../project/traversal-limits.js";

/**
 * Static, bounded, read-only enumeration and validation of workspace
 * GDScript files for check-only diagnostics. The source workspace is only
 * read here; the checked engine only ever sees the disposable mirror.
 */

export interface EnumerateGDScriptFilesOptions {
  readonly workspaceRoot: string;
  readonly signal?: AbortSignal;
  readonly maxFiles?: number;
  readonly maxTotalBytes?: number;
  readonly maxEntries?: number;
  readonly maxDepth?: number;
}

export interface GodotScriptEnumeration {
  /** Sorted workspace-relative targets with `/` separators. */
  readonly targets: readonly { readonly path: string; readonly bytes: number }[];
  readonly truncated: boolean;
}

/** Statically enumerates eligible `.gd` files deterministically. */
export async function enumerateGDScriptFiles(
  options: EnumerateGDScriptFilesOptions,
): Promise<GodotScriptEnumeration> {
  const maxFiles = options.maxFiles ?? GODOT_LIMITS.maxGDScriptFilesPerProject;
  const maxTotalBytes = options.maxTotalBytes ?? GODOT_LIMITS.maxGDScriptTotalBytes;
  const maxEntries = options.maxEntries ?? GODOT_LIMITS.maxProjectEntriesExamined;
  const maxDepth = options.maxDepth ?? GODOT_LIMITS.maxMirrorDepth;
  const collected: { readonly path: string; readonly bytes: number }[] = [];
  let truncated = false;
  let totalBytes = 0;

  const walk = async (
    directory: string,
    relativeDirectory: string,
    depth: number,
  ): Promise<void> => {
    if (truncated) {
      return;
    }
    if (options.signal?.aborted) {
      throw createAbortError();
    }
    if (depth > maxDepth) {
      truncated = true;
      return;
    }
    let entries;
    try {
      const { readdir } = await import("node:fs/promises");
      entries = await readdir(directory, { withFileTypes: true });
    } catch {
      return;
    }
    if (entries.length > maxEntries) {
      truncated = true;
      return;
    }
    for (const entry of entries) {
      if (truncated) {
        return;
      }
      if (options.signal?.aborted) {
        throw createAbortError();
      }
      if (
        PROJECT_SCAN_EXCLUDED_DIRECTORIES.includes(entry.name) ||
        entry.name.startsWith(".solaris-mutation-") ||
        entry.name.startsWith(".solaris-quarantine-")
      ) {
        continue;
      }
      const entryPath = join(directory, entry.name);
      let metadata;
      try {
        metadata = await lstat(entryPath);
      } catch {
        continue;
      }
      if (metadata.isSymbolicLink()) {
        // Symlinked scripts and symlinked directories are never followed.
        continue;
      }
      if (metadata.isDirectory()) {
        await walk(entryPath, join(relativeDirectory, entry.name), depth + 1);
        continue;
      }
      if (!metadata.isFile() || !entry.name.toLowerCase().endsWith(".gd")) {
        continue;
      }
      if (metadata.size > GODOT_LIMITS.maxGDScriptFileBytes) {
        truncated = true;
        return;
      }
      if (collected.length >= maxFiles) {
        truncated = true;
        return;
      }
      if (totalBytes + metadata.size > maxTotalBytes) {
        truncated = true;
        return;
      }
      collected.push({
        path: join(relativeDirectory, entry.name).split(sep).join("/"),
        bytes: metadata.size,
      });
      totalBytes += metadata.size;
    }
  };

  await walk(options.workspaceRoot, "", 0);
  collected.sort((left, right) => (left.path < right.path ? -1 : left.path > right.path ? 1 : 0));
  return { targets: collected, truncated };
}

export type GodotCheckScriptValidation =
  | {
      readonly ok: true;
      readonly canonicalPath: string;
      readonly sha256: string;
      readonly bytes: number;
    }
  | {
      readonly ok: false;
      readonly reason:
        | "invalid-path"
        | "absolute"
        | "traversal"
        | "not-gd"
        | "missing"
        | "not-regular"
        | "symlink"
        | "too-large"
        | "unreadable";
      readonly message: string;
    };

/**
 * Validates and hashes one workspace-relative `.gd` path: lexical
 * validation, containment verification, non-following lstat (symlinks
 * rejected), regular file only, size bound, and an identity-checked SHA-256
 * over the exact bytes. The returned target is the exact digest binding for
 * the prepared check and its approval.
 */
export async function validateCheckScript(options: {
  readonly workspaceRoot: string;
  readonly relativePath: string;
  readonly signal?: AbortSignal;
}): Promise<GodotCheckScriptValidation> {
  const { workspaceRoot, relativePath, signal } = options;
  const lexical = validateProjectRelativePath(relativePath, GODOT_LIMITS.maxResReferencePathBytes);
  if (!lexical.ok) {
    if (lexical.reason === "absolute") {
      return {
        ok: false,
        reason: "absolute",
        message: "The script path must be workspace-relative, not absolute.",
      };
    }
    if (lexical.reason === "escape") {
      return {
        ok: false,
        reason: "traversal",
        message: "The script path must not escape the workspace.",
      };
    }
    return {
      ok: false,
      reason: "invalid-path",
      message: "The script path is invalid.",
    };
  }
  if (!relativePath.toLowerCase().endsWith(".gd")) {
    return {
      ok: false,
      reason: "not-gd",
      message: "Only workspace-relative .gd script paths can be checked.",
    };
  }
  const verified = await verifyProjectPathContainment(
    workspaceRoot,
    join(workspaceRoot, relativePath),
    DEFAULT_FS_OPS,
  );
  if (!verified.ok) {
    if (verified.reason === "symlink") {
      return {
        ok: false,
        reason: "symlink",
        message: "Symbolic links are rejected; the script must be a regular file.",
      };
    }
    if (verified.reason === "outside") {
      return {
        ok: false,
        reason: "traversal",
        message: "The script path must not escape the workspace.",
      };
    }
    return {
      ok: false,
      reason: "missing",
      message: `The script ${relativePath} does not exist in the workspace.`,
    };
  }
  let metadata;
  try {
    metadata = await lstat(verified.canonicalPath);
  } catch {
    return {
      ok: false,
      reason: "missing",
      message: `The script ${relativePath} does not exist in the workspace.`,
    };
  }
  if (metadata.isSymbolicLink()) {
    return {
      ok: false,
      reason: "symlink",
      message: "Symbolic links are rejected; the script must be a regular file.",
    };
  }
  if (!metadata.isFile()) {
    return {
      ok: false,
      reason: "not-regular",
      message: "The script path is not a regular file.",
    };
  }
  if (metadata.size > GODOT_LIMITS.maxGDScriptFileBytes) {
    return {
      ok: false,
      reason: "too-large",
      message: `The script exceeds the ${GODOT_LIMITS.maxGDScriptFileBytes}-byte GDScript file bound.`,
    };
  }
  const hashed = await hashAbsoluteFile(verified.canonicalPath, signal);
  if (hashed === null) {
    return {
      ok: false,
      reason: "unreadable",
      message: `The script ${relativePath} could not be verified; it may have changed during inspection.`,
    };
  }
  return {
    ok: true,
    canonicalPath: verified.canonicalPath,
    sha256: hashed.sha256,
    bytes: hashed.bytes,
  };
}

/** Hash an already-validated target (project-wide checks). */
export async function hashScriptTarget(options: {
  readonly workspaceRoot: string;
  readonly relativePath: string;
  readonly signal?: AbortSignal;
}): Promise<GodotScriptCheckTarget | null> {
  const validated = await validateCheckScript(options);
  if (!validated.ok) {
    return null;
  }
  return {
    path: options.relativePath.split(sep).join("/"),
    sha256: validated.sha256,
    bytes: validated.bytes,
  };
}
