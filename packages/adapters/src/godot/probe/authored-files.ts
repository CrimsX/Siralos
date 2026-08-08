import { createHash } from "node:crypto";
import { lstat, open, readdir } from "node:fs/promises";
import { join, sep } from "node:path";
import { GODOT_LIMITS } from "@solaris/core";
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
  readonly timeoutMs?: number;
}

/**
 * Bounded, symlink-safe authored-file scan used for both the risk-manifest
 * authored-file digest and the workspace-integrity baseline. Excluded
 * generated/metadata directories are never entered, symbolic links and
 * special files are not authored content and are skipped, and the file
 * count, byte total, and deadline bound the walk. The digest is a SHA-256
 * over the canonical JSON of the entry list, so two identical workspaces
 * always produce identical digests.
 */
export async function scanAuthoredFiles(
  options: ScanAuthoredFilesOptions,
): Promise<AuthoredFileManifest> {
  const maxFiles = options.maxFiles ?? GODOT_LIMITS.maxBaselineManifestFiles;
  const maxBytes = options.maxBytes ?? GODOT_LIMITS.maxBaselineManifestBytes;
  const timeoutMs = options.timeoutMs ?? GODOT_LIMITS.riskRefreshTimeoutMs;
  const deadline = Date.now() + timeoutMs;
  const entries: AuthoredFileEntry[] = [];
  let totalBytes = 0;
  let truncated = false;

  const walk = async (directory: string, relativeDirectory: string): Promise<void> => {
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
    let dirents: { readonly name: string; readonly isDirectory: () => boolean }[];
    try {
      dirents = await readdir(directory, { withFileTypes: true });
    } catch {
      return;
    }
    dirents.sort((left, right) => left.name.localeCompare(right.name));
    for (const entry of dirents) {
      if (truncated || entries.length >= maxFiles) {
        truncated = true;
        return;
      }
      if (options.signal?.aborted) {
        throw createAbortError();
      }
      if (Date.now() > deadline) {
        truncated = true;
        return;
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
        continue;
      }
      if (metadata.isDirectory()) {
        await walk(entryPath, join(relativeDirectory, entry.name));
        continue;
      }
      if (!metadata.isFile()) {
        continue;
      }
      if (totalBytes + metadata.size > maxBytes) {
        truncated = true;
        return;
      }
      const hash = await hashAuthoredFile(entryPath, options.signal);
      if (hash === null) {
        continue;
      }
      entries.push({
        relativePath: normalizeRelative(join(relativeDirectory, entry.name)),
        bytes: metadata.size,
        sha256: hash,
      });
      totalBytes += metadata.size;
    }
  };

  await walk(options.workspaceRoot, "");
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
): Promise<string | null> {
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
    return hash.digest("hex");
  } catch (error: unknown) {
    if (isAbortError(error)) {
      throw error;
    }
    return null;
  } finally {
    await handle.close().catch(() => undefined);
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
