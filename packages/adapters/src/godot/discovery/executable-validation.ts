import { createHash } from "node:crypto";
import { lstat, realpath, stat } from "node:fs/promises";
import { isAbsolute, resolve, sep } from "node:path";
import { GODOT_LIMITS } from "@solaris/core";

export interface ExecutableIdentity {
  /** Canonical absolute path (symlinks resolved). */
  readonly canonicalPath: string;
  readonly sizeBytes: number;
  readonly modifiedAtMs: number;
  readonly sha256: string;
}

export type ExecutableValidationResult =
  | { readonly ok: true; readonly identity: ExecutableIdentity }
  | { readonly ok: false; readonly error: string };

export interface ValidateExecutableOptions {
  readonly path: string;
  readonly workspaceRoot: string;
  readonly signal?: AbortSignal;
  /** Maximum accepted executable size; defaults to the Godot milestone limit. */
  readonly maxBytes?: number;
}

/**
 * Validates and fingerprints an executable candidate.
 *
 * Steps: resolve to an absolute path, canonicalize (recording the canonical
 * target even when the configured path resolves through a symlink), require
 * a regular file, reject special files, bound the size, and compute a
 * SHA-256. Executables inside the project workspace are rejected by
 * default; the project is untrusted and cannot host the engine.
 */
export async function validateExecutable(
  options: ValidateExecutableOptions,
): Promise<ExecutableValidationResult> {
  const maxBytes = options.maxBytes ?? GODOT_LIMITS.maxExecutableBytes;
  let canonical: string;
  try {
    canonical = await realpath(options.path);
  } catch (error: unknown) {
    return { ok: false, error: describeFileError(options.path, error, "resolve") };
  }
  let metadata;
  try {
    metadata = await stat(canonical);
  } catch (error: unknown) {
    return { ok: false, error: describeFileError(canonical, error, "inspect") };
  }
  if (!metadata.isFile()) {
    return {
      ok: false,
      error: `The executable path ${safeDisplayPath(canonical)} is not a regular file.`,
    };
  }
  if (process.platform !== "win32" && (metadata.mode & 0o111) === 0) {
    return {
      ok: false,
      error: `The executable path ${safeDisplayPath(canonical)} is not executable.`,
    };
  }
  if (metadata.size > maxBytes) {
    return {
      ok: false,
      error: `The executable exceeds the ${formatBytes(maxBytes)} size limit.`,
    };
  }
  const workspacePrefix = workspacePrefixOf(options.workspaceRoot);
  const caseInsensitive = process.platform === "win32" || process.platform === "darwin";
  if (
    isWithin(canonical, workspacePrefix, caseInsensitive) ||
    isWithin(options.path, workspacePrefix, caseInsensitive)
  ) {
    return {
      ok: false,
      error:
        "The executable resolves inside the project workspace; workspace-contained engines are rejected by default.",
    };
  }
  const sha256 = await hashFile(canonical, options.signal);
  if (sha256 === null) {
    return { ok: false, error: "The executable could not be read for fingerprinting." };
  }
  return {
    ok: true,
    identity: {
      canonicalPath: canonical,
      sizeBytes: metadata.size,
      modifiedAtMs: metadata.mtimeMs,
      sha256,
    },
  };
}

/**
 * Lightweight identity revalidation before every probe: size and
 * modification time must be unchanged. A changed executable invalidates the
 * prepared profile and requires rediscovery.
 */
export async function revalidateExecutableIdentity(
  identity: ExecutableIdentity,
): Promise<{ readonly unchanged: true } | { readonly unchanged: false; readonly error: string }> {
  let metadata;
  try {
    metadata = await lstat(identity.canonicalPath);
  } catch (error: unknown) {
    return {
      unchanged: false,
      error: `The executable is no longer accessible: ${describeFileError(identity.canonicalPath, error, "inspect")}`,
    };
  }
  if (!metadata.isFile() || metadata.size !== identity.sizeBytes) {
    return {
      unchanged: false,
      error: "The executable changed after validation; rediscovery is required.",
    };
  }
  if (metadata.mtimeMs !== identity.modifiedAtMs) {
    return {
      unchanged: false,
      error: "The executable changed after validation; rediscovery is required.",
    };
  }
  return { unchanged: true };
}

export async function hashFile(
  path: string,
  signal?: AbortSignal,
  maxBytes?: number,
): Promise<string | null> {
  const { open } = await import("node:fs/promises");
  let handle;
  try {
    handle = await open(path, "r");
  } catch {
    return null;
  }
  const hash = createHash("sha256");
  const buffer = Buffer.allocUnsafe(64 * 1024);
  let total = 0;
  try {
    for (;;) {
      if (signal?.aborted) {
        return null;
      }
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, total);
      if (bytesRead === 0) {
        break;
      }
      total += bytesRead;
      if (maxBytes !== undefined && total > maxBytes) {
        return null;
      }
      hash.update(buffer.subarray(0, bytesRead));
    }
  } catch {
    return null;
  } finally {
    await handle.close().catch(() => undefined);
  }
  return hash.digest("hex");
}

function workspacePrefixOf(workspaceRoot: string): string {
  return workspaceRoot.endsWith(sep) ? workspaceRoot : `${workspaceRoot}${sep}`;
}

function isWithin(path: string, prefix: string, caseInsensitive: boolean): boolean {
  return caseInsensitive
    ? path.toLowerCase().startsWith(prefix.toLowerCase())
    : path.startsWith(prefix);
}

function describeFileError(path: string, error: unknown, operation: string): string {
  if (error instanceof Error && "code" in error && error.code === "ENOENT") {
    return `The executable does not exist: ${safeDisplayPath(path)}`;
  }
  if (error instanceof Error && "code" in error && error.code === "EACCES") {
    return `The executable is not accessible: ${safeDisplayPath(path)}`;
  }
  return `The executable could not be ${operation === "resolve" ? "resolved" : "inspected"}: ${safeDisplayPath(path)}`;
}

function safeDisplayPath(path: string): string {
  if (isAbsolute(path)) {
    return path;
  }
  return resolve(path);
}

function formatBytes(bytes: number): string {
  return `${Math.round(bytes / (1024 * 1024))} MiB`;
}
