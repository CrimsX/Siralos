import { lstat, open, readdir, realpath } from "node:fs/promises";
import type { Dirent, Stats } from "node:fs";
import type { FileHandle } from "node:fs/promises";
import { join, relative } from "node:path";
import type { GodotScanTruncationReason } from "@siralos/core";
import { isWithinPathIdentity, samePathIdentity } from "../../fs-path-identity.js";

/**
 * Shared bounded-traversal and path-containment infrastructure for static
 * Godot project inspection.
 *
 * Two security invariants are enforced here and nowhere else:
 *
 * 1. Project-controlled path values (`res://...` references, plugin `script`
 *    values, GDExtension library targets) are validated LEXICALLY first —
 *    null bytes, absolute host paths, drive-qualified paths, UNC paths, and
 *    `..` escapes are rejected before any filesystem call — and only then
 *    canonically: every parent component is lstat'd without following, the
 *    parent is realpath'd, and identity-aware containment against the
 *    canonical workspace root is confirmed. An outside candidate is never
 *    lstat'd/stat'd/realpath'd merely to determine that it escapes.
 *
 * 2. Every per-file read verifies canonical containment before the open and
 *    re-verifies path identity after the read, so a parent or leaf swapped
 *    during inspection is detected and the data is discarded rather than
 *    trusted (deterministic design: mismatch after the read is treated as a
 *    changed/escape condition and the read is skipped).
 */

export interface GodotProjectFsOps {
  readonly lstat: (path: string) => Promise<Stats>;
  readonly realpath: (path: string) => Promise<string>;
  readonly readdir: (path: string, options: { readonly withFileTypes: true }) => Promise<Dirent[]>;
  readonly open: (path: string) => Promise<FileHandle>;
}

export const DEFAULT_FS_OPS: GodotProjectFsOps = {
  lstat: (path) => lstat(path),
  realpath: (path) => realpath(path),
  readdir: (path, options) => readdir(path, options),
  open: (path) => open(path, "r"),
};

/** Independent counters and deadline shared by walk, plugin, and read phases. */
export interface TraversalBudgetSpec {
  readonly timeoutMs: number;
  readonly maxFiles: number;
  readonly maxDirectories: number;
  readonly maxEntries: number;
  readonly maxSurfaced: number;
  readonly maxReadBytes: number;
  readonly maxPluginDirectories: number;
  readonly maxDescriptorsParsed: number;
  readonly maxInventoryItems: number;
  readonly maxDepth: number;
}

export class TraversalBudget {
  readonly deadline: number;
  readonly maxFiles: number;
  readonly maxDirectories: number;
  readonly maxEntries: number;
  readonly maxSurfaced: number;
  readonly maxReadBytes: number;
  readonly maxPluginDirectories: number;
  readonly maxDescriptorsParsed: number;
  readonly maxInventoryItems: number;
  readonly maxDepth: number;
  directoriesVisited = 0;
  entriesExamined = 0;
  filesScanned = 0;
  filesSurfaced = 0;
  bytesRead = 0;
  pluginDirectories = 0;
  descriptorsParsed = 0;
  inventoryItems = 0;
  reason: GodotScanTruncationReason = "none";

  constructor(spec: TraversalBudgetSpec) {
    this.deadline = Date.now() + spec.timeoutMs;
    this.maxFiles = spec.maxFiles;
    this.maxDirectories = spec.maxDirectories;
    this.maxEntries = spec.maxEntries;
    this.maxSurfaced = spec.maxSurfaced;
    this.maxReadBytes = spec.maxReadBytes;
    this.maxPluginDirectories = spec.maxPluginDirectories;
    this.maxDescriptorsParsed = spec.maxDescriptorsParsed;
    this.maxInventoryItems = spec.maxInventoryItems;
    this.maxDepth = spec.maxDepth;
  }

  get exhausted(): boolean {
    return this.reason !== "none";
  }

  /** Records the first exhaustion reason only, preserving priority order. */
  stop(reason: Exclude<GodotScanTruncationReason, "none">): void {
    if (this.reason === "none") {
      this.reason = reason;
    }
  }

  /** Throws when the signal is aborted; the budget is also recorded as cancelled. */
  checkCancelled(signal?: AbortSignal): void {
    if (signal?.aborted) {
      this.stop("cancelled");
      throw createAbortError();
    }
  }

  /** True while the shared deadline is in the future; stops with timeout when expired. */
  isWithinDeadline(): boolean {
    if (Date.now() <= this.deadline) {
      return true;
    }
    this.stop("timeout");
    return false;
  }

  /**
   * Adds raw read bytes; returns false when the total-read bound is now
   * exceeded (the bound is enforced between reads, never mid-read).
   */
  consumeBytes(bytes: number): boolean {
    this.bytesRead += bytes;
    if (this.bytesRead > this.maxReadBytes) {
      this.stop("bytes-limit");
      return false;
    }
    return true;
  }

  /** Returns false when the inventory output-item bound is exceeded. */
  addInventoryItem(): boolean {
    this.inventoryItems += 1;
    if (this.inventoryItems > this.maxInventoryItems) {
      this.stop("inventory-limit");
      return false;
    }
    return true;
  }
}

export function createTraversalBudget(spec: TraversalBudgetSpec): TraversalBudget {
  return new TraversalBudget(spec);
}

/**
 * Pure lexical validation of a project-provided RELATIVE path value.
 * Rejects null bytes, absolute host paths (leading `/`, drive-qualified
 * `C:\`, UNC `\\host\share`), and any `..` segment after separator
 * normalization, and bounds the UTF-8 byte length. No filesystem access.
 */
export type ProjectRelativePathVerdict =
  | { readonly ok: true; readonly value: string }
  | { readonly ok: false; readonly reason: "absolute" | "null-bytes" | "escape" | "too-long" };

export function validateProjectRelativePath(
  raw: string,
  maxBytes: number,
): ProjectRelativePathVerdict {
  if (raw.includes("\0")) {
    return { ok: false, reason: "null-bytes" };
  }
  const normalized = raw.replaceAll("\\", "/");
  if (normalized.startsWith("/")) {
    return { ok: false, reason: "absolute" };
  }
  if (/^[A-Za-z]:\//.test(normalized)) {
    return { ok: false, reason: "absolute" };
  }
  for (const segment of normalized.split("/")) {
    if (segment === "..") {
      return { ok: false, reason: "escape" };
    }
  }
  if (Buffer.byteLength(raw, "utf8") > maxBytes) {
    return { ok: false, reason: "too-long" };
  }
  return { ok: true, value: normalized };
}

/**
 * Canonical containment of an existing absolute candidate path inside the
 * canonical workspace root. The lexical identity check runs first and an
 * outside candidate returns "outside" without any filesystem call. For
 * in-root candidates every parent component is lstat'd without following
 * (symlinks/junctions/reparse points are rejected), the parent is
 * realpath'd, and identity-aware containment is confirmed. The returned
 * `canonicalPath` is the identity anchor for the after-read re-verification.
 */
export type ProjectPathContainmentVerdict =
  | { readonly ok: true; readonly canonicalPath: string }
  | { readonly ok: false; readonly reason: "outside" | "missing" | "symlink" };

export async function verifyProjectPathContainment(
  canonicalRoot: string,
  absolutePath: string,
  fsOps: GodotProjectFsOps,
): Promise<ProjectPathContainmentVerdict> {
  if (!isWithinPathIdentity(canonicalRoot, absolutePath)) {
    return { ok: false, reason: "outside" };
  }
  const relativePath = relative(canonicalRoot, absolutePath);
  if (
    relativePath.length === 0 ||
    relativePath === "." ||
    relativePath.startsWith("..") ||
    isAbsoluteRelative(relativePath)
  ) {
    return { ok: false, reason: "outside" };
  }
  const components = relativePath.split(/[\\/]+/).filter((component) => component.length > 0);
  if (components.length === 0) {
    return { ok: false, reason: "outside" };
  }
  const parentComponents = components.slice(0, -1);
  let current = canonicalRoot;
  for (const component of parentComponents) {
    current = join(current, component);
    try {
      const metadata = await fsOps.lstat(current);
      if (metadata.isSymbolicLink()) {
        return { ok: false, reason: "symlink" };
      }
    } catch {
      return { ok: false, reason: "missing" };
    }
  }
  let canonicalParent: string;
  try {
    canonicalParent = await fsOps.realpath(current);
  } catch {
    return { ok: false, reason: "missing" };
  }
  if (!isWithinPathIdentity(canonicalRoot, canonicalParent)) {
    return { ok: false, reason: "outside" };
  }
  return {
    ok: true,
    canonicalPath: join(canonicalParent, components[components.length - 1] ?? ""),
  };
}

function isAbsoluteRelative(value: string): boolean {
  if (value.startsWith("/") || value.startsWith("\\")) {
    return true;
  }
  return /^[A-Za-z]:[\\/]/.test(value);
}

/**
 * Verified bounded read of a project file: canonical containment before the
 * open, a single bounded raw read, and an identity re-verification after the
 * read. A path swapped during inspection returns "changed" and the data is
 * never returned to callers.
 */
export type BoundedProjectFileRead =
  | { readonly ok: true; readonly content: string; readonly bytesRead: number }
  | {
      readonly ok: false;
      readonly reason:
        "outside" | "missing" | "symlink" | "not-regular" | "oversized" | "changed" | "read-failed";
    };

export async function readBoundedProjectFile(options: {
  readonly canonicalRoot: string;
  readonly path: string;
  readonly maxBytes: number;
  readonly fsOps: GodotProjectFsOps;
}): Promise<BoundedProjectFileRead> {
  const verified = await verifyProjectPathContainment(
    options.canonicalRoot,
    options.path,
    options.fsOps,
  );
  if (!verified.ok) {
    return { ok: false, reason: verified.reason };
  }
  let metadata: Stats;
  try {
    metadata = await options.fsOps.lstat(options.path);
  } catch {
    return { ok: false, reason: "missing" };
  }
  if (metadata.isSymbolicLink()) {
    return { ok: false, reason: "symlink" };
  }
  if (!metadata.isFile()) {
    return { ok: false, reason: "not-regular" };
  }
  if (metadata.size > options.maxBytes) {
    return { ok: false, reason: "oversized" };
  }
  let handle: FileHandle;
  try {
    handle = await options.fsOps.open(options.path);
  } catch {
    return { ok: false, reason: "read-failed" };
  }
  let bytes: Buffer;
  try {
    const buffer = Buffer.alloc(Math.min(metadata.size, options.maxBytes) + 1);
    const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
    bytes = buffer.subarray(0, bytesRead);
  } catch {
    return { ok: false, reason: "read-failed" };
  } finally {
    await handle.close().catch(() => undefined);
  }
  if (bytes.length > options.maxBytes) {
    return { ok: false, reason: "oversized" };
  }
  const after = await options.fsOps.realpath(options.path).catch(() => null);
  if (after === null || !samePathIdentity(after, verified.canonicalPath)) {
    return { ok: false, reason: "changed" };
  }
  return { ok: true, content: bytes.toString("utf8"), bytesRead: bytes.length };
}

export function createAbortError(): Error {
  return new DOMException("The project scan was aborted.", "AbortError");
}
