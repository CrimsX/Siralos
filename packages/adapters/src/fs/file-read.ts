import { lstat, open } from "node:fs/promises";

function isNotFoundError(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}

/**
 * Bounded single-shot file read. The target is lstat-verified first (a
 * non-regular file or a symbolic link is rejected without being opened, so
 * a FIFO can never block the read) and the read itself is capped at
 * `maxBytes + 1`, so a file grown or swapped after the lstat is never fully
 * materialized. Returns `null` when the file does not exist, is not a
 * regular file, is a symbolic link, or exceeds the bound.
 */
export async function readFileBounded(path: string, maxBytes: number): Promise<Buffer | null> {
  let metadata;
  try {
    metadata = await lstat(path);
  } catch (error: unknown) {
    if (isNotFoundError(error)) {
      return null;
    }
    return null;
  }
  if (metadata.isSymbolicLink() || !metadata.isFile()) {
    return null;
  }
  if (metadata.size > maxBytes) {
    return null;
  }
  let handle;
  try {
    handle = await open(path, "r");
  } catch {
    return null;
  }
  try {
    const buffer = Buffer.allocUnsafe(maxBytes + 1);
    const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
    if (bytesRead > maxBytes) {
      return null;
    }
    return buffer.subarray(0, bytesRead);
  } catch {
    return null;
  } finally {
    await handle.close().catch(() => undefined);
  }
}
