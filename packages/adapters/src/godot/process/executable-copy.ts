import { createHash } from "node:crypto";
import { chmod, lstat, open, realpath, unlink } from "node:fs/promises";
import type { FileHandle } from "node:fs/promises";
import { join } from "node:path";
import { samePathIdentity } from "../../fs-path-identity.js";

/**
 * Private-copy staging for Godot probe executables.
 *
 * The probe runner never executes the mutable configured executable path.
 * Instead the validated bytes are copied into the probe's own run
 * directory (which the sandbox grants exclusively) and that private copy is
 * executed. Staging is exclusive, size-bounded, hashed while streaming,
 * mode-restricted on POSIX, and re-verified by full SHA-256 before the copy
 * path is returned. Any failure — source read error, size overrun, disk
 * full, target collision, permission failure, or hash mismatch — fails the
 * probe closed; the mutable configured path is NEVER executed as a
 * fallback.
 *
 * Assumption (documented, not guaranteed): Linux and macOS Godot binaries
 * are self-contained and run correctly from a private copy for the fixed
 * Siralos probe invocations (`--version`, `--help`, `--dump-extension-api`).
 * The copy is byte-verified, so executing it executes exactly the validated
 * bytes; the enclosing `.app` bundle of a macOS installation is never
 * required at probe time.
 */
export interface StageVerifiedExecutableCopyOptions {
  /** Canonical validated source path (never executed directly). */
  readonly sourcePath: string;
  /** Verified Siralos-owned run root; the copy is created directly inside it. */
  readonly runRoot: string;
  /** Expected SHA-256 of the executed bytes (the validated identity). */
  readonly expectedSha256: string;
  readonly maxBytes: number;
  readonly signal?: AbortSignal;
}

export type StageVerifiedExecutableCopyResult =
  { readonly ok: true; readonly copyPath: string } | { readonly ok: false; readonly error: string };

/** Fixed private copy name inside each probe's run directory. */
export const PRIVATE_EXECUTABLE_COPY_NAME = "siralos-godot-executable";

const STREAM_CHUNK_BYTES = 64 * 1024;

export async function stageVerifiedExecutableCopy(
  options: StageVerifiedExecutableCopyOptions,
): Promise<StageVerifiedExecutableCopyResult> {
  if (options.signal?.aborted) {
    return { ok: false, error: "The Godot probe was aborted before the private copy was staged." };
  }
  const runRootVerified = await verifyRealDirectory(options.runRoot);
  if (!runRootVerified) {
    return {
      ok: false,
      error: "The probe run directory could not be verified; the probe did not run (fail closed).",
    };
  }
  const sourceVerified = await verifySourceExecutable(options.sourcePath);
  if (!sourceVerified) {
    return {
      ok: false,
      error:
        "The validated executable could not be re-verified before the probe; the probe did not run (fail closed).",
    };
  }
  const copyPath = join(options.runRoot, PRIVATE_EXECUTABLE_COPY_NAME);
  const hash = createHash("sha256");
  let sourceHandle;
  let targetHandle;
  try {
    sourceHandle = await open(options.sourcePath, "r");
  } catch (error: unknown) {
    return {
      ok: false,
      error: `The validated executable could not be opened for copying: ${describeFsError(error)}`,
    };
  }
  let total = 0;
  try {
    try {
      targetHandle = await open(copyPath, "wx", 0o700);
    } catch (error: unknown) {
      return {
        ok: false,
        error: `The private executable copy could not be created in the probe directory: ${describeFsError(error)}`,
      };
    }
    const buffer = Buffer.allocUnsafe(STREAM_CHUNK_BYTES);
    for (;;) {
      if (options.signal?.aborted) {
        await removeCopy(targetHandle, copyPath);
        return {
          ok: false,
          error: "The Godot probe was aborted while the private executable copy was staged.",
        };
      }
      let bytesRead;
      try {
        ({ bytesRead } = await sourceHandle.read(buffer, 0, buffer.length, total));
      } catch (error: unknown) {
        await removeCopy(targetHandle, copyPath);
        return {
          ok: false,
          error: `The validated executable could not be read for copying: ${describeFsError(error)}`,
        };
      }
      if (bytesRead === 0) {
        break;
      }
      total += bytesRead;
      if (total > options.maxBytes) {
        await removeCopy(targetHandle, copyPath);
        return {
          ok: false,
          error:
            "The validated executable exceeds the size limit while staging the private copy; the probe did not run.",
        };
      }
      hash.update(buffer.subarray(0, bytesRead));
      try {
        await targetHandle.write(buffer.subarray(0, bytesRead));
      } catch (error: unknown) {
        await removeCopy(targetHandle, copyPath);
        return {
          ok: false,
          error: `The private executable copy could not be written (disk full or filesystem error): ${describeFsError(error)}`,
        };
      }
    }
    try {
      await targetHandle.sync();
    } catch (error: unknown) {
      await removeCopy(targetHandle, copyPath);
      return {
        ok: false,
        error: `The private executable copy could not be synced: ${describeFsError(error)}`,
      };
    }
  } finally {
    await sourceHandle.close().catch(() => undefined);
    await targetHandle?.close().catch(() => undefined);
  }
  if (process.platform !== "win32") {
    try {
      await chmod(copyPath, 0o755);
    } catch (error: unknown) {
      await unlink(copyPath).catch(() => undefined);
      return {
        ok: false,
        error: `The private executable copy could not be made executable: ${describeFsError(error)}`,
      };
    }
  }
  const verified = await verifyCopyMatches(copyPath, options.expectedSha256, options.maxBytes);
  if (!verified.ok) {
    await unlink(copyPath).catch(() => undefined);
    return { ok: false, error: verified.error };
  }
  return { ok: true, copyPath };
}

/**
 * Verifies the staged copy immediately before execution: it must still be a
 * regular non-link file whose complete bounded SHA-256 equals the validated
 * fingerprint. A replacement of the copy between staging and execution is
 * detected here, and a mismatch always fails the probe closed.
 */
async function verifyCopyMatches(
  copyPath: string,
  expectedSha256: string,
  maxBytes: number,
): Promise<{ readonly ok: true } | { readonly ok: false; readonly error: string }> {
  let metadata;
  try {
    metadata = await lstat(copyPath);
  } catch (error: unknown) {
    return {
      ok: false,
      error: `The private executable copy could not be inspected before execution: ${describeFsError(error)}`,
    };
  }
  if (metadata.isSymbolicLink() || !metadata.isFile()) {
    return {
      ok: false,
      error: "The private executable copy is not a regular file; the probe did not run.",
    };
  }
  if (metadata.size > maxBytes) {
    return {
      ok: false,
      error: "The private executable copy exceeds the size limit; the probe did not run.",
    };
  }
  const copyHash = await hashFileBounded(copyPath, maxBytes);
  if (copyHash === null) {
    return {
      ok: false,
      error:
        "The private executable copy could not be read for verification; the probe did not run.",
    };
  }
  if (copyHash !== expectedSha256) {
    return {
      ok: false,
      error:
        "The private executable copy does not match the validated fingerprint; the probe did not run (fail closed).",
    };
  }
  return { ok: true };
}

async function verifyRealDirectory(path: string): Promise<boolean> {
  let metadata;
  try {
    metadata = await lstat(path);
  } catch {
    return false;
  }
  if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
    return false;
  }
  let canonical: string;
  try {
    canonical = await realpath(path);
  } catch {
    return false;
  }
  return samePathIdentity(canonical, path);
}

async function verifySourceExecutable(path: string): Promise<boolean> {
  let metadata;
  try {
    metadata = await lstat(path);
  } catch {
    return false;
  }
  if (metadata.isSymbolicLink() || !metadata.isFile()) {
    return false;
  }
  let canonical: string;
  try {
    canonical = await realpath(path);
  } catch {
    return false;
  }
  return samePathIdentity(canonical, path);
}

async function removeCopy(targetHandle: FileHandle | undefined, copyPath: string): Promise<void> {
  await targetHandle?.close().catch(() => undefined);
  await unlink(copyPath).catch(() => undefined);
}

async function hashFileBounded(path: string, maxBytes: number): Promise<string | null> {
  let handle;
  try {
    handle = await open(path, "r");
  } catch {
    return null;
  }
  const hash = createHash("sha256");
  const buffer = Buffer.allocUnsafe(STREAM_CHUNK_BYTES);
  let total = 0;
  try {
    for (;;) {
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, total);
      if (bytesRead === 0) {
        break;
      }
      total += bytesRead;
      if (total > maxBytes) {
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

function describeFsError(error: unknown): string {
  if (error instanceof Error && "code" in error && typeof error.code === "string") {
    return `${error.code}${error.message.length > 0 ? `: ${error.message}` : ""}`;
  }
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "an unknown filesystem error occurred";
}
