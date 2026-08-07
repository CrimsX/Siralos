import { createHash } from "node:crypto";
import { lstat, open } from "node:fs/promises";
import { join } from "node:path";
import { GODOT_LIMITS } from "@solaris/core";

export interface GodotProjectFileInfo {
  readonly exists: boolean;
  readonly sha256: string | null;
  /** Size in bytes when the file exists. */
  readonly sizeBytes: number | null;
}

export type GodotProjectFileRead =
  | {
      readonly ok: true;
      readonly content: string;
      readonly sha256: string;
      readonly sizeBytes: number;
    }
  | {
      readonly ok: false;
      readonly reason:
        "missing" | "not-regular" | "symlink" | "oversized" | "read-failed" | "cancelled";
      readonly message: string;
    };

/**
 * Root-only static project detection. A Godot project is detected only when
 * a regular, non-symlinked `project.godot` exists at the workspace root.
 * Parents and children are never searched and `--upwards` is never used.
 */
export async function readProjectFile(
  workspaceRoot: string,
  signal?: AbortSignal,
): Promise<GodotProjectFileRead> {
  const path = join(workspaceRoot, "project.godot");
  let metadata;
  try {
    metadata = await lstat(path);
  } catch (error: unknown) {
    if (isNotFoundError(error)) {
      return {
        ok: false,
        reason: "missing",
        message: "No project.godot exists at the workspace root.",
      };
    }
    return { ok: false, reason: "read-failed", message: describeError(error) };
  }
  if (metadata.isSymbolicLink()) {
    return {
      ok: false,
      reason: "symlink",
      message: "project.godot must be a regular file; symbolic links are rejected.",
    };
  }
  if (!metadata.isFile()) {
    return {
      ok: false,
      reason: "not-regular",
      message: "project.godot is not a regular file.",
    };
  }
  if (metadata.size > GODOT_LIMITS.maxProjectFileBytes) {
    return {
      ok: false,
      reason: "oversized",
      message: `project.godot exceeds the ${Math.round(GODOT_LIMITS.maxProjectFileBytes / (1024 * 1024))} MiB limit.`,
    };
  }
  let handle;
  try {
    handle = await open(path, "r");
  } catch (error: unknown) {
    return { ok: false, reason: "read-failed", message: describeError(error) };
  }
  const hash = createHash("sha256");
  const buffer = Buffer.allocUnsafe(64 * 1024);
  let content = "";
  let total = 0;
  try {
    for (;;) {
      if (signal?.aborted) {
        return { ok: false, reason: "cancelled", message: "The project file read was cancelled." };
      }
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, total);
      if (bytesRead === 0) {
        break;
      }
      total += bytesRead;
      hash.update(buffer.subarray(0, bytesRead));
      content += buffer.subarray(0, bytesRead).toString("utf8");
      if (content.length > GODOT_LIMITS.maxProjectFileBytes) {
        return {
          ok: false,
          reason: "oversized",
          message: `project.godot exceeds the ${Math.round(GODOT_LIMITS.maxProjectFileBytes / (1024 * 1024))} MiB limit.`,
        };
      }
    }
  } catch (error: unknown) {
    return { ok: false, reason: "read-failed", message: describeError(error) };
  } finally {
    await handle.close().catch(() => undefined);
  }
  return {
    ok: true,
    content,
    sha256: hash.digest("hex"),
    sizeBytes: total,
  };
}

/** Convenience for callers that only need existence and the hash. */
export async function inspectProjectFileInfo(
  workspaceRoot: string,
  signal?: AbortSignal,
): Promise<GodotProjectFileInfo> {
  const read = await readProjectFile(workspaceRoot, signal);
  if (!read.ok) {
    return { exists: read.reason !== "missing", sha256: null, sizeBytes: null };
  }
  return { exists: true, sha256: read.sha256, sizeBytes: read.sizeBytes };
}

function isNotFoundError(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "an unknown filesystem error occurred";
}
