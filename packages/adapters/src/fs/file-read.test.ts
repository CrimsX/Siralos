import { afterEach, describe, expect, it } from "vitest";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readBounded, readFileBounded, type BoundedReadSource } from "./file-read.js";

/**
 * Deterministic fake read source that serves a fixed logical stream in
 * chunks of at most `chunkSize` bytes per read at the explicit
 * position. A short read is always legal before EOF, so this seam
 * forces the exact condition the old single-shot implementation got
 * wrong: one read returning fewer bytes than the file contains must
 * never be treated as EOF-complete data.
 */
class ChunkedSource implements BoundedReadSource {
  constructor(
    private readonly data: Buffer,
    private readonly chunkSize: number,
  ) {}

  read(
    buffer: Buffer,
    offset: number,
    length: number,
    position: number,
  ): Promise<{ readonly bytesRead: number }> {
    if (position >= this.data.length) {
      return Promise.resolve({ bytesRead: 0 });
    }
    const count = Math.min(length, this.chunkSize, this.data.length - position);
    this.data.copy(buffer, offset, position, position + count);
    return Promise.resolve({ bytesRead: count });
  }
}

describe("readBounded (deterministic short-read seam)", () => {
  it("reconstructs the complete stream from 1-byte short reads", async () => {
    const data = Buffer.from("the quick brown fox jumps over the lazy dog", "utf8");
    const result = await readBounded(new ChunkedSource(data, 1), data.length);
    // Under the previous one-shot assumption this returned only the
    // first byte; the bounded complete loop returns the whole stream.
    expect(result).not.toBeNull();
    expect(result!.equals(data)).toBe(true);
  });

  it("reconstructs the complete stream from 2-byte and 3-byte short reads", async () => {
    const data = Buffer.from("short reads must never be EOF", "utf8");
    for (const chunkSize of [2, 3]) {
      const result = await readBounded(new ChunkedSource(data, chunkSize), data.length);
      expect(result!.equals(data), "chunk size " + chunkSize).toBe(true);
    }
  });

  it("reconstructs arbitrarily chunked streams at or below the bound", async () => {
    const data = Buffer.from(Array.from({ length: 300 }, (_, index) => index % 251));
    for (const chunkSize of [1, 2, 3, 7, 64]) {
      const result = await readBounded(new ChunkedSource(data, chunkSize), data.length);
      expect(result!.equals(data), "chunk size " + chunkSize).toBe(true);
    }
  });

  it("never returns a partial prefix for streams over the bound", async () => {
    const data = Buffer.from(Array.from({ length: 64 }, (_, index) => index % 251));
    for (const chunkSize of [1, 2, 3, 7, 64]) {
      expect(
        await readBounded(new ChunkedSource(data, chunkSize), 32),
        "chunk size " + chunkSize,
      ).toBeNull();
    }
    // Exactly one byte over the bound is still null, never a prefix.
    const over = Buffer.from("x".repeat(33), "utf8");
    expect(await readBounded(new ChunkedSource(over, 5), 32)).toBeNull();
  });

  it("returns the empty stream for an empty source", async () => {
    const result = await readBounded(new ChunkedSource(Buffer.alloc(0), 3), 10);
    expect(result).not.toBeNull();
    expect(result!.length).toBe(0);
  });
});

describe("readFileBounded (real files)", () => {
  let dir: string | null = null;

  afterEach(async () => {
    if (dir !== null) {
      await rm(dir, { recursive: true, force: true });
      dir = null;
    }
  });

  async function withDir(): Promise<string> {
    dir = await mkdtemp(join(tmpdir(), "siralos-read-bounded-"));
    return dir;
  }

  it("returns exact complete bytes at and below the bound", async () => {
    const root = await withDir();
    const limit = 16;
    const cases: ReadonlyArray<{ readonly name: string; readonly size: number }> = [
      { name: "empty.txt", size: 0 },
      { name: "one.txt", size: 1 },
      { name: "small.txt", size: 7 },
      { name: "exact.txt", size: limit },
    ];
    for (const { name, size } of cases) {
      const bytes = Buffer.alloc(size, 0x7a);
      await writeFile(join(root, name), bytes);
      const result = await readFileBounded(join(root, name), limit);
      expect(result, name).not.toBeNull();
      expect(result!.equals(bytes), name).toBe(true);
    }
  });

  it("returns null (never a prefix) for files over the bound", async () => {
    const root = await withDir();
    const limit = 16;
    await writeFile(join(root, "over.txt"), Buffer.alloc(limit + 1, 0x7a));
    await writeFile(join(root, "large.txt"), Buffer.alloc(limit * 8, 0x7a));
    expect(await readFileBounded(join(root, "over.txt"), limit)).toBeNull();
    expect(await readFileBounded(join(root, "large.txt"), limit)).toBeNull();
  });

  it("returns null for missing, linked, and non-regular targets", async () => {
    const root = await withDir();
    await writeFile(join(root, "a.txt"), "x");
    expect(await readFileBounded(join(root, "missing"), 16)).toBeNull();
    expect(await readFileBounded(root, 16)).toBeNull();
  });
});
