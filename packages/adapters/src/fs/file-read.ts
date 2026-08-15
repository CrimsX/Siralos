import { lstat, open } from "node:fs/promises";

function isNotFoundError(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}

/** Minimal bounded read surface (FileHandle-compatible) for the
 * deterministic bounded complete-read loop. */
export interface BoundedReadSource {
  read(
    buffer: Buffer,
    offset: number,
    length: number,
    position: number,
  ): Promise<{ readonly bytesRead: number }>;
}

const READ_CHUNK_BYTES = 64 * 1024;

/**
 * Bounded complete read loop over one read source with the whole-file
 * contract:
 *
 * 1. at most `maxBytes + 1` bytes are ever read;
 * 2. reading continues until EOF or until the cap is reached, so one
 *    short read is never treated as EOF;
 * 3. the complete stream is returned only when EOF was reached at a
 *    size <= `maxBytes`;
 * 4. more than `maxBytes` bytes yields `null`, never a partial prefix
 *    presented as complete;
 * 5. allocation grows incrementally; no `maxBytes + 1` buffer is
 *    reserved up front.
 *
 * For a regular file, a zero-byte read means EOF; the explicit
 * `position` keeps the loop deterministic regardless of any platform
 * short-read behavior.
 */
export async function readBounded(
  source: BoundedReadSource,
  maxBytes: number,
): Promise<Buffer | null> {
  const chunks: Buffer[] = [];
  const chunkSize = Math.min(READ_CHUNK_BYTES, maxBytes + 1);
  let total = 0;
  while (total <= maxBytes) {
    const want = Math.min(chunkSize, maxBytes + 1 - total);
    const chunk = Buffer.allocUnsafe(want);
    const { bytesRead } = await source.read(chunk, 0, want, total);
    if (bytesRead === 0) {
      break; // EOF
    }
    chunks.push(chunk.subarray(0, bytesRead));
    total += bytesRead;
  }
  if (total > maxBytes) {
    return null;
  }
  return Buffer.concat(chunks, total);
}

/**
 * Bounded complete file read. The target is lstat-verified first (a
 * non-regular file or a symbolic link is rejected without being opened, so
 * a FIFO can never block the read), the declared size may reject early,
 * and the read itself is a bounded complete loop capped at
 * `maxBytes + 1` with EOF verification, so a file grown or swapped after
 * the lstat is never fully materialized and never reported as complete.
 * Returns `null` when the file does not exist, is not a regular file, is
 * a symbolic link, exceeds the bound, or cannot be read.
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
    return await readBounded(handle, maxBytes);
  } catch {
    return null;
  } finally {
    await handle.close().catch(() => undefined);
  }
}
