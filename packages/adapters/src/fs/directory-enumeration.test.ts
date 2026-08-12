import { describe, expect, it } from "vitest";
import { mkdtemp, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { enumerateDirectoryBounded } from "./directory-enumeration.js";
import { SYMLINKS_SUPPORTED } from "../tools/workspace/workspace-fixtures.js";

async function withTree(): Promise<{ root: string; cleanup: () => Promise<void> }> {
  const root = await mkdtemp(join(tmpdir(), "siralos-enumerate-bounded-"));
  return {
    root,
    cleanup: () => rm(root, { recursive: true, force: true }),
  };
}

describe("enumerateDirectoryBounded", () => {
  it("reads every entry once and reports the exact examined count", async () => {
    const { root, cleanup } = await withTree();
    try {
      for (let index = 0; index < 12; index += 1) {
        await writeFile(join(root, `file-${index}.txt`), "x");
      }
      const names: string[] = [];
      const outcome = await enumerateDirectoryBounded({
        directory: root,
        maxEntries: 100,
        onEntry: (entry) => names.push(entry.name),
      });
      expect(outcome).toEqual({ entriesExamined: 12, truncated: false, missing: false });
      expect(names).toHaveLength(12);
      expect(await readdir(root)).toHaveLength(12);
    } finally {
      await cleanup();
    }
  });

  it("caps the examined count incrementally and reports truncation", async () => {
    const { root, cleanup } = await withTree();
    try {
      for (let index = 0; index < 30; index += 1) {
        await writeFile(join(root, `file-${index}.txt`), "x");
      }
      const names: string[] = [];
      const outcome = await enumerateDirectoryBounded({
        directory: root,
        maxEntries: 5,
        onEntry: (entry) => names.push(entry.name),
      });
      expect(outcome.truncated).toBe(true);
      expect(outcome.entriesExamined).toBe(5);
      expect(names).toHaveLength(5);
    } finally {
      await cleanup();
    }
  });

  it("reports a missing directory without throwing", async () => {
    const outcome = await enumerateDirectoryBounded({
      directory: join(tmpdir(), "siralos-enumerate-missing-" + Date.now()),
      maxEntries: 10,
      onEntry: () => {},
    });
    expect(outcome).toEqual({ entriesExamined: 0, truncated: false, missing: true });
  });

  it("reports a symlink entry without following it", async () => {
    if (!SYMLINKS_SUPPORTED) {
      return;
    }
    const { root, cleanup } = await withTree();
    const outside = await mkdtemp(join(tmpdir(), "siralos-enumerate-outside-"));
    try {
      await writeFile(join(outside, "victim.txt"), "keep me");
      await symlink(outside, join(root, "linked"), "dir");
      const names: string[] = [];
      await enumerateDirectoryBounded({
        directory: root,
        maxEntries: 10,
        onEntry: (entry) => names.push(entry.name),
      });
      expect(names).toContain("linked");
    } finally {
      await cleanup();
      await rm(outside, { recursive: true, force: true });
    }
  });

  it("aborts mid-enumeration on cancellation and never hides it", async () => {
    const { root, cleanup } = await withTree();
    try {
      for (let index = 0; index < 10; index += 1) {
        await writeFile(join(root, `file-${index}.txt`), "x");
      }
      const controller = new AbortController();
      let seen = 0;
      const pending = enumerateDirectoryBounded({
        directory: root,
        maxEntries: 100,
        signal: controller.signal,
        onEntry: () => {
          seen += 1;
          if (seen === 1) {
            controller.abort();
          }
        },
      });
      await expect(pending).rejects.toMatchObject({ name: "AbortError" });
    } finally {
      await cleanup();
    }
  });

  it("truncates when the deadline expires", async () => {
    const { root, cleanup } = await withTree();
    try {
      await writeFile(join(root, "a.txt"), "x");
      const outcome = await enumerateDirectoryBounded({
        directory: root,
        maxEntries: 100,
        deadline: Date.now() - 1,
        onEntry: () => {},
      });
      expect(outcome.truncated).toBe(true);
      expect(outcome.entriesExamined).toBe(0);
    } finally {
      await cleanup();
    }
  });
});
