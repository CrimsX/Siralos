import { describe, expect, it } from "vitest";
import { mkdir, mkdtemp, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { removeDirectoryTreeBounded } from "./directory-enumeration.js";
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
      const { planDirectoryRemoval } = await import("./directory-enumeration.js");
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
