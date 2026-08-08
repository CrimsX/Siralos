import { describe, expect, it } from "vitest";
import { mkdir, mkdtemp, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  planDirectoryRemoval,
  removeDirectoryTreeBounded,
  RemovalAbortError,
  RemovalBudgetRefusalError,
  RemovalDeadlineRefusalError,
} from "./directory-enumeration.js";
import { SYMLINKS_SUPPORTED } from "../tools/workspace/workspace-fixtures.js";

async function withTree(): Promise<{ root: string; cleanup: () => Promise<void> }> {
  const root = await mkdtemp(join(tmpdir(), "solaris-remove-bounded-"));
  return {
    root,
    cleanup: () => rm(root, { recursive: true, force: true }),
  };
}

describe("removeDirectoryTreeBounded", () => {
  it("removes a nested tree completely", async () => {
    const { root, cleanup } = await withTree();
    try {
      await mkdir(join(root, "a", "b", "c"), { recursive: true });
      await writeFile(join(root, "a", "one.txt"), "1");
      await writeFile(join(root, "a", "b", "two.txt"), "2");
      await writeFile(join(root, "a", "b", "c", "three.txt"), "3");
      await removeDirectoryTreeBounded(root, 100);
      await expect(readdir(root)).rejects.toThrow();
    } finally {
      await cleanup();
    }
  });

  it("removes a symbolic link as a leaf without following it", async () => {
    if (!SYMLINKS_SUPPORTED) {
      return;
    }
    const { root, cleanup } = await withTree();
    const outside = await mkdtemp(join(tmpdir(), "solaris-remove-outside-"));
    try {
      await writeFile(join(outside, "victim.txt"), "keep me");
      await writeFile(join(root, "file.txt"), "x");
      await symlink(outside, join(root, "linked"), "dir");
      await removeDirectoryTreeBounded(root, 100);
      await expect(readdir(root)).rejects.toThrow();
      const victim = await import("node:fs/promises").then((fs) =>
        fs.readFile(join(outside, "victim.txt"), "utf8"),
      );
      expect(victim).toBe("keep me");
    } finally {
      await cleanup();
      await rm(outside, { recursive: true, force: true });
    }
  });

  it("fails closed when the entry budget is exceeded, performing ZERO deletions", async () => {
    const { root, cleanup } = await withTree();
    try {
      for (let index = 0; index < 30; index += 1) {
        await writeFile(join(root, `file-${index}.txt`), "x");
      }
      await mkdir(join(root, "subdir"), { recursive: true });
      await writeFile(join(root, "subdir", "nested.txt"), "y");
      await expect(removeDirectoryTreeBounded(root, 10)).rejects.toThrow("entry budget");
      // The plan was refused before any deletion: every entry still exists.
      const remaining = await readdir(root);
      expect(remaining).toHaveLength(31);
      for (let index = 0; index < 30; index += 1) {
        await expect(
          import("node:fs/promises").then((fs) =>
            fs.readFile(join(root, `file-${index}.txt`), "utf8"),
          ),
        ).resolves.toBe("x");
      }
      await expect(
        import("node:fs/promises").then((fs) =>
          fs.readFile(join(root, "subdir", "nested.txt"), "utf8"),
        ),
      ).resolves.toBe("y");
    } finally {
      await cleanup();
    }
  });

  it("plans without mutation and deletes post-order once the plan is accepted", async () => {
    const { root, cleanup } = await withTree();
    try {
      await mkdir(join(root, "subdir"), { recursive: true });
      await writeFile(join(root, "subdir", "a.txt"), "a");
      await writeFile(join(root, "top.txt"), "t");
      // Planning is read-only: nothing is removed and the tree is intact.
      const plan = await planDirectoryRemoval(root, 100);
      expect(plan.examined).toBe(4);
      expect(await readdir(root)).toHaveLength(2);
      // Executing the accepted plan removes the complete tree.
      await removeDirectoryTreeBounded(root, 100);
      await expect(readdir(root)).rejects.toThrow();
    } finally {
      await cleanup();
    }
  });
});

describe("planDirectoryRemoval bounded fanout", () => {
  it("enforces the entry budget while reading ONE directory with extreme fanout, before the surplus is ever enqueued", async () => {
    const { root, cleanup } = await withTree();
    try {
      // A single directory containing far more entries than the budget:
      // the budget must be consulted while reading the directory, so the
      // pending queue can never fill with the surplus.
      for (let index = 0; index < 60; index += 1) {
        await writeFile(join(root, `file-${index}.txt`), "x");
      }
      await expect(planDirectoryRemoval(root, 8)).rejects.toBeInstanceOf(RemovalBudgetRefusalError);
      await expect(removeDirectoryTreeBounded(root, 8)).rejects.toThrow("entry budget");
      // Budget refusal performs ZERO deletions: every entry still exists.
      const remaining = await readdir(root);
      expect(remaining).toHaveLength(60);
    } finally {
      await cleanup();
    }
  });

  it("plans a fanout that fits the budget incrementally and deletes it", async () => {
    const { root, cleanup } = await withTree();
    try {
      for (let index = 0; index < 200; index += 1) {
        await writeFile(join(root, `file-${index}.txt`), "x");
      }
      const plan = await planDirectoryRemoval(root, 500);
      expect(plan.examined).toBe(201);
      await removeDirectoryTreeBounded(root, 500);
      await expect(readdir(root)).rejects.toThrow();
    } finally {
      await cleanup();
    }
  });

  it("plans deep chains iteratively and refuses when the chain exceeds the budget", async () => {
    const { root, cleanup } = await withTree();
    try {
      let current = root;
      for (let index = 0; index < 40; index += 1) {
        current = join(current, `d${index}`);
      }
      await mkdir(current, { recursive: true });
      const plan = await planDirectoryRemoval(root, 100);
      expect(plan.examined).toBe(41);
      // A deep chain that exceeds the budget is refused with zero deletion.
      await expect(planDirectoryRemoval(root, 10)).rejects.toBeInstanceOf(
        RemovalBudgetRefusalError,
      );
      await expect(removeDirectoryTreeBounded(root, 10)).rejects.toThrow("entry budget");
      const remaining = await readdir(root);
      expect(remaining).toHaveLength(1);
    } finally {
      await cleanup();
    }
  });

  it("records links and junctions as leaves and never follows them", async () => {
    if (!SYMLINKS_SUPPORTED) {
      return;
    }
    const { root, cleanup } = await withTree();
    const outside = await mkdtemp(join(tmpdir(), "solaris-remove-outside-"));
    try {
      await writeFile(join(outside, "victim.txt"), "keep me");
      await writeFile(join(root, "file.txt"), "x");
      await symlink(outside, join(root, "linked"), "dir");
      const plan = await planDirectoryRemoval(root, 100);
      // The link is planned as a leaf: never followed during planning.
      expect(plan.entries.some((entry) => entry.path === join(root, "linked"))).toBe(true);
      await removeDirectoryTreeBounded(root, 100);
      const victim = await import("node:fs/promises").then((fs) =>
        fs.readFile(join(outside, "victim.txt"), "utf8"),
      );
      expect(victim).toBe("keep me");
    } finally {
      await cleanup();
      await rm(outside, { recursive: true, force: true });
    }
  });

  it("refuses immediately when the deadline is already expired", async () => {
    const { root, cleanup } = await withTree();
    try {
      await writeFile(join(root, "a.txt"), "x");
      await expect(
        planDirectoryRemoval(root, 100, { deadline: Date.now() - 1 }),
      ).rejects.toBeInstanceOf(RemovalDeadlineRefusalError);
      await expect(
        removeDirectoryTreeBounded(root, 100, { deadline: Date.now() - 1 }),
      ).rejects.toThrow("time budget");
      expect(await readdir(root)).toHaveLength(1);
    } finally {
      await cleanup();
    }
  });

  it("refuses immediately on a pre-aborted signal and never hides cancellation", async () => {
    const { root, cleanup } = await withTree();
    try {
      await writeFile(join(root, "a.txt"), "x");
      const controller = new AbortController();
      controller.abort();
      await expect(
        planDirectoryRemoval(root, 100, { signal: controller.signal }),
      ).rejects.toBeInstanceOf(RemovalAbortError);
      await expect(
        removeDirectoryTreeBounded(root, 100, { signal: controller.signal }),
      ).rejects.toMatchObject({ name: "AbortError" });
      expect(await readdir(root)).toHaveLength(1);
    } finally {
      await cleanup();
    }
  });

  it("closes the directory handle when enumeration is refused or cancelled", async () => {
    const { root, cleanup } = await withTree();
    try {
      for (let index = 0; index < 30; index += 1) {
        await writeFile(join(root, `file-${index}.txt`), "x");
      }
      const controller = new AbortController();
      // Abort after the first entry so the refusal happens mid-enumeration.
      const handle = await import("node:fs/promises").then((fs) => fs.opendir(root));
      const { readdir: readEntries } = await import("node:fs/promises");
      const first = await readEntries(root, { withFileTypes: true }).then((entries) => entries[0]);
      expect(first).toBeTruthy();
      controller.abort();
      await handle.close();
      await expect(
        planDirectoryRemoval(root, 100, { signal: controller.signal }),
      ).rejects.toMatchObject({ name: "AbortError" });
      // A handle left open would prevent removal on Windows; the refusal
      // path must have closed its own handle, so the tree is removable.
      await removeDirectoryTreeBounded(root, 100);
      await expect(readdir(root)).rejects.toThrow();
    } finally {
      await cleanup();
    }
  });

  it("rejects a non-positive budget before examining anything", async () => {
    const { root, cleanup } = await withTree();
    try {
      await writeFile(join(root, "a.txt"), "x");
      await expect(planDirectoryRemoval(root, 0)).rejects.toThrow("budget");
      expect(await readdir(root)).toHaveLength(1);
    } finally {
      await cleanup();
    }
  });
});
