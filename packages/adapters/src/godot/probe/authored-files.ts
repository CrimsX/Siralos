import { createHash } from "node:crypto";
import { lstat, open } from "node:fs/promises";
import type { BigIntStats, Dirent } from "node:fs";
import { join, sep } from "node:path";
import { GODOT_LIMITS } from "@siralos/core";
import { enumerateDirectoryBounded } from "../../fs/directory-enumeration.js";
import { PROJECT_SCAN_EXCLUDED_DIRECTORIES } from "../project/bounded-scan.js";

export interface AuthoredFileEntry {
  readonly relativePath: string;
  readonly bytes: number;
  readonly sha256: string;
}

export interface AuthoredFileManifest {
  readonly entries: readonly AuthoredFileEntry[];
  readonly fileCount: number;
  readonly totalBytes: number;
  readonly digest: string;
  readonly truncated: boolean;
}

export interface ScanAuthoredFilesOptions {
  readonly workspaceRoot: string;
  readonly signal?: AbortSignal;
  readonly maxFiles?: number;
  readonly maxBytes?: number;
  readonly maxEntries?: number;
  readonly maxDepth?: number;
  readonly timeoutMs?: number;
}

/**
 * Bounded, symlink-safe authored-file scan used for both the risk-manifest
 * authored-file digest and the workspace-integrity baseline. Excluded
 * generated/metadata directories are never entered, symbolic links and
 * special files are not authored content and are skipped, directories are
 * enumerated incrementally through a directory handle (a hostile directory
 * with millions of entries is never materialized), the file count, entry
 * count, depth, byte total, and deadline bound the walk, and the digest is
 * a SHA-256 over the canonical JSON of the sorted entry list, so two
 * identical workspaces always produce identical digests. Every hashed file
 * is lstat-verified without following (a symlink/junction leaf is never
 * dereferenced) and the open handle's final stat must still match the
 * pre-open stat (size, timestamps, device, inode); a file replaced or
 * rewritten during the read is discarded rather than trusted.
 */
export async function scanAuthoredFiles(
  options: ScanAuthoredFilesOptions,
): Promise<AuthoredFileManifest> {
  const maxFiles = options.maxFiles ?? GODOT_LIMITS.maxBaselineManifestFiles;
  const maxBytes = options.maxBytes ?? GODOT_LIMITS.maxBaselineManifestBytes;
  const maxEntries = options.maxEntries ?? GODOT_LIMITS.maxProjectEntriesExamined;
  const maxDepth = options.maxDepth ?? GODOT_LIMITS.maxMirrorDepth;
  const timeoutMs = options.timeoutMs ?? GODOT_LIMITS.riskRefreshTimeoutMs;
  const deadline = Date.now() + timeoutMs;
  const entries: AuthoredFileEntry[] = [];
  let totalBytes = 0;
  let truncated = false;

  const walk = async (
    directory: string,
    relativeDirectory: string,
    depth: number,
    entryBudget: number,
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
    if (Date.now() > deadline) {
      truncated = true;
      return;
    }
    // Incremental enumeration through a directory handle: a hostile
    // directory with millions of entries is never materialized. The
    // synchronous collector stays within the remaining entry budget.
    const collected: { readonly entry: Dirent }[] = [];
    const outcome = await enumerateDirectoryBounded({
      directory,
      maxEntries: entryBudget,
      ...(options.signal === undefined ? {} : { signal: options.signal }),
      deadline,
      onEntry: (entry) => {
        collected.push({ entry });
      },
    });
    if (outcome.truncated) {
      truncated = true;
    }
    for (const { entry } of collected) {
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
      if (entries.length >= maxFiles) {
        truncated = true;
        return;
      }
      if (
        PROJECT_SCAN_EXCLUDED_DIRECTORIES.includes(entry.name) ||
        entry.name.startsWith(".siralos-mutation-") ||
        entry.name.startsWith(".siralos-quarantine-")
      ) {
        continue;
      }
      const entryPath = join(directory, entry.name);
      const metadata = await safeLstat(entryPath);
      if (metadata === null) {
        continue;
      }
      if (metadata.isSymbolicLink()) {
        continue;
      }
      if (metadata.isDirectory()) {
        await walk(entryPath, join(relativeDirectory, entry.name), depth + 1, entryBudget);
        continue;
      }
      if (!metadata.isFile()) {
        continue;
      }
      const fileBytes = Number(metadata.size);
      if (totalBytes + fileBytes > maxBytes) {
        truncated = true;
        return;
      }
      const hash = await hashAuthoredFile(entryPath, options.signal);
      if (hash === null) {
        continue;
      }
      entries.push({
        relativePath: normalizeRelative(join(relativeDirectory, entry.name)),
        bytes: fileBytes,
        sha256: hash.sha256,
      });
      totalBytes += fileBytes;
    }
  };

  await walk(options.workspaceRoot, "", 0, maxEntries);
  entries.sort((left, right) => left.relativePath.localeCompare(right.relativePath));
  const digest = computeAuthoredFileDigest(entries);
  return {
    entries,
    fileCount: entries.length,
    totalBytes,
    digest,
    truncated,
  };
}

export function computeAuthoredFileDigest(entries: readonly AuthoredFileEntry[]): string {
  const canonical = JSON.stringify(
    entries.map((entry) => ({
      bytes: entry.bytes,
      path: entry.relativePath,
      sha256: entry.sha256,
    })),
  );
  return createHash("sha256").update(canonical, "utf8").digest("hex");
}

async function hashAuthoredFile(
  filePath: string,
  signal: AbortSignal | undefined,
): Promise<{ readonly sha256: string } | null> {
  const before = await safeLstat(filePath);
  if (before === null || before.isSymbolicLink() || !before.isFile()) {
    return null;
  }
  let handle;
  try {
    handle = await open(filePath, "r");
  } catch {
    return null;
  }
  try {
    const hash = createHash("sha256");
    const buffer = Buffer.alloc(1024 * 1024);
    for (;;) {
      if (signal?.aborted) {
        throw createAbortError();
      }
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, null);
      if (bytesRead === 0) {
        break;
      }
      hash.update(buffer.subarray(0, bytesRead));
    }
    // The object read must still be the object accepted before the open; a
    // same-path substitution or in-place rewrite during the read discards
    // the result rather than trusting it.
    const after = await handle.stat({ bigint: true });
    if (!sameFileIdentity(before, after)) {
      return null;
    }
    return { sha256: hash.digest("hex") };
  } catch (error: unknown) {
    if (isAbortError(error)) {
      throw error;
    }
    return null;
  } finally {
    await handle.close().catch(() => undefined);
  }
}

/**
 * Identity fields that must not change while a file is being read. BigInt
 * stats expose nanosecond timestamps on every platform; on platforms where
 * `ino` is unusable it reads as zero on both sides and the timestamp and
 * size comparisons still apply.
 */
function sameFileIdentity(before: BigIntStats, after: BigIntStats): boolean {
  return (
    before.size === after.size &&
    before.mtimeNs === after.mtimeNs &&
    before.ctimeNs === after.ctimeNs &&
    before.dev === after.dev &&
    before.ino === after.ino
  );
}

async function safeLstat(path: string): Promise<BigIntStats | null> {
  try {
    return await lstat(path, { bigint: true });
  } catch {
    return null;
  }
}

function normalizeRelative(relativePath: string): string {
  return relativePath.split(sep).join("/");
}

function isAbortError(error: unknown): boolean {
  return (
    error instanceof Error &&
    (error.name === "AbortError" || ("code" in error && error.code === "ABORT_ERR"))
  );
}

function createAbortError(): Error {
  return new DOMException("The authored-file scan was aborted.", "AbortError");
}
