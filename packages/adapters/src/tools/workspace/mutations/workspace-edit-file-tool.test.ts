import { afterEach, describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import type { CheckpointStore, ToolPreparationResult } from "@solaris/core";
import { createWorkspaceEditFileTool } from "./workspace-edit-file-tool.js";
import { createMutationLock } from "./mutation-lock.js";
import { WORKSPACE_LIMITS } from "../limits.js";
import type { ReplacementFsOps } from "./safe-replacement.js";
import {
  cleanupTempCheckpointDirs,
  createSymlink,
  createTempCheckpointStore,
  createTempWorkspace,
  expectSuccess,
  SYMLINKS_SUPPORTED,
  writeFixtureFiles,
  type TempWorkspace,
} from "../workspace-fixtures.js";

const workspaces: TempWorkspace[] = [];

async function withWorkspace(): Promise<TempWorkspace> {
  const workspace = await createTempWorkspace();
  workspaces.push(workspace);
  return workspace;
}

async function createTool(
  workspaceRoot: string,
  dependencies?: Parameters<typeof createWorkspaceEditFileTool>[3],
) {
  const store = await createTempCheckpointStore(workspaceRoot);
  return {
    tool: createWorkspaceEditFileTool(workspaceRoot, createMutationLock(), store, dependencies),
    store,
  };
}

async function hashOf(absolutePath: string): Promise<string> {
  const bytes = await readFile(absolutePath);
  return createHash("sha256").update(bytes).digest("hex");
}

async function prepareEdit(
  tool: ReturnType<typeof createWorkspaceEditFileTool>,
  filePath: string,
  hash: string,
  replacements: readonly { oldText: string; newText: string }[],
): Promise<ToolPreparationResult> {
  return tool.prepare({ path: filePath, expectedSha256: hash, replacements }, {});
}

afterEach(async () => {
  await cleanupTempCheckpointDirs();
  for (const workspace of workspaces.splice(0)) {
    await workspace.cleanup();
  }
});

describe("workspace.edit_file", () => {
  it("applies one exact replacement", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "Created text here\n" });
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "Created", newText: "Updated" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    const output = expectSuccess(result);
    expect(output["operation"]).toBe("update");
    const bytes = await readFile(path.join(workspace.root, "a.txt"));
    expect(bytes.toString("utf8")).toBe("Updated text here\n");
    expect(output["afterSha256"]).toBe(createHash("sha256").update(bytes).digest("hex"));
  });

  it("applies several sequential replacements", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "one two three\n" });
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "one", newText: "1" },
      { oldText: "two", newText: "2" },
      { oldText: "three", newText: "3" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("success");
    const bytes = await readFile(path.join(workspace.root, "a.txt"));
    expect(bytes.toString("utf8")).toBe("1 2 3\n");
  });

  it("produces a complete update preview", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "old line\n" });
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [{ oldText: "old", newText: "new" }]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    expect(prepared.preview.files[0]).toMatchObject({
      path: "a.txt",
      operation: "update",
      beforeSha256: hash,
      addedLines: 1,
      removedLines: 1,
    });
    expect(prepared.preview.files[0]?.unifiedDiff).toContain("-old line");
    expect(prepared.preview.files[0]?.unifiedDiff).toContain("+new line");
  });

  it("fails with zero matches", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "hello\n" });
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [{ oldText: "missing", newText: "x" }]);
    expect(prepared).toMatchObject({
      status: "conflict",
    });
    if (prepared.status === "conflict") {
      expect(prepared.message).toContain("not found");
    }
  });

  it("fails with multiple matches", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "dup\ndup\n" });
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [{ oldText: "dup", newText: "x" }]);
    expect(prepared).toMatchObject({
      status: "conflict",
    });
    if (prepared.status === "conflict") {
      expect(prepared.message).toContain("ambiguous");
    }
  });

  it("rejects empty oldText", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "hello\n" });
    const { tool } = await createTool(workspace.root);
    const prepared = await prepareEdit(tool, "a.txt", "a".repeat(64), [
      { oldText: "", newText: "x" },
    ]);
    expect(prepared).toMatchObject({ status: "invalid_input" });
  });

  it("rejects a no-op result", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "same\n" });
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [{ oldText: "same", newText: "same" }]);
    expect(prepared).toMatchObject({
      status: "failed",
    });
    if (prepared.status === "failed") {
      expect(prepared.message).toContain("identical");
    }
  });

  it("rejects protected files", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { ".env": "KEY=value\n" });
    const { tool } = await createTool(workspace.root);
    const prepared = await prepareEdit(tool, ".env", "a".repeat(64), [
      { oldText: "KEY", newText: "SECRET" },
    ]);
    expect(prepared).toMatchObject({ status: "denied" });
  });

  it("rejects a symbolic-link target", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "real.txt": "hello\n" });
    await createSymlink(
      path.join(workspace.root, "real.txt"),
      path.join(workspace.root, "link.txt"),
    );
    const { tool } = await createTool(workspace.root);
    const prepared = await prepareEdit(tool, "link.txt", "a".repeat(64), [
      { oldText: "hello", newText: "bye" },
    ]);
    expect(prepared).toMatchObject({ status: "denied" });
  });

  it("rejects binary files", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "bin.dat": Buffer.from([0x00, 0x01, 0x02]) });
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "bin.dat"));
    const prepared = await prepareEdit(tool, "bin.dat", hash, [{ oldText: "a", newText: "b" }]);
    expect(prepared).toMatchObject({
      status: "failed",
    });
    if (prepared.status === "failed") {
      expect(prepared.message).toContain("binary");
    }
  });

  it("rejects oversized files", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, {
      "big.txt": Buffer.alloc(WORKSPACE_LIMITS.maxTextFileSizeBytes + 1, 0x61),
    });
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "big.txt"));
    const prepared = await prepareEdit(tool, "big.txt", hash, [{ oldText: "a", newText: "b" }]);
    expect(prepared).toMatchObject({
      status: "failed",
    });
    if (prepared.status === "failed") {
      expect(prepared.message).toContain("too large");
    }
  });

  it("conflicts on a stale hash during preparation", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const { tool } = await createTool(workspace.root);
    const prepared = await prepareEdit(tool, "a.txt", "f".repeat(64), [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared).toMatchObject({
      status: "conflict",
    });
    if (prepared.status === "conflict") {
      expect(prepared.message).toContain("reread");
    }
  });

  it("conflicts when the file changes after approval", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    await writeFixtureFiles(workspace.root, { "a.txt": "external edit\n" });
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("conflict");
    const bytes = await readFile(path.join(workspace.root, "a.txt"));
    expect(bytes.toString("utf8")).toBe("external edit\n");
  });

  it("conflicts when the file disappears after approval", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const { rm } = await import("node:fs/promises");
    await rm(path.join(workspace.root, "a.txt"));
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("conflict");
  });

  it(
    "conflicts when the target becomes a symbolic link",
    { skip: !SYMLINKS_SUPPORTED },
    async () => {
      const workspace = await withWorkspace();
      await writeFixtureFiles(workspace.root, { "a.txt": "original\n", "other.txt": "other\n" });
      const { tool } = await createTool(workspace.root);
      const hash = await hashOf(path.join(workspace.root, "a.txt"));
      const prepared = await prepareEdit(tool, "a.txt", hash, [
        { oldText: "original", newText: "changed" },
      ]);
      expect(prepared.status).toBe("ready");
      if (prepared.status !== "ready") {
        return;
      }
      const { rm } = await import("node:fs/promises");
      await rm(path.join(workspace.root, "a.txt"));
      await createSymlink(
        path.join(workspace.root, "other.txt"),
        path.join(workspace.root, "a.txt"),
      );
      const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
      expect(result.status).toBe("conflict");
    },
  );

  it("verifies the final hash and leaves no temporary files", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("success");
    const entries = await (await import("node:fs/promises")).readdir(workspace.root);
    expect(entries.some((entry) => entry.startsWith(".solaris-mutation-"))).toBe(false);
  });

  it("cannot be applied twice", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    expect((await tool.apply(prepared.mutation, { approvedDigest: prepared.digest })).status).toBe(
      "success",
    );
    expect((await tool.apply(prepared.mutation, { approvedDigest: prepared.digest })).status).toBe(
      "failed",
    );
  });

  it("denies apply under a digest that does not match the prepared plan", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, {
      approvedDigest: "another-digest-entirely",
    });
    expect(result.status).toBe("denied");
    const content = await (
      await import("node:fs/promises")
    ).readFile(path.join(workspace.root, "a.txt"), "utf8");
    expect(content).toBe("original\n");
  });

  it("rejects replacements beyond the limit", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "x\n" });
    const { tool } = await createTool(workspace.root);
    const replacements = Array.from({ length: WORKSPACE_LIMITS.maxReplacements + 1 }, () => ({
      oldText: "x",
      newText: "y",
    }));
    const prepared = await prepareEdit(tool, "a.txt", "a".repeat(64), replacements);
    expect(prepared).toMatchObject({ status: "invalid_input" });
  });

  it(
    "edits a file through a case-variant spelling on case-insensitive platforms",
    { skip: process.platform === "linux" },
    async () => {
      const workspace = await withWorkspace();
      await writeFixtureFiles(workspace.root, { "docs/note.md": "original\n" });
      const { tool } = await createTool(workspace.root);
      const hash = await hashOf(path.join(workspace.root, "docs", "note.md"));
      const prepared = await prepareEdit(tool, "DOCS/NOTE.MD", hash, [
        { oldText: "original", newText: "changed" },
      ]);
      expect(prepared.status).toBe("ready");
      if (prepared.status !== "ready") {
        return;
      }
      const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
      expect(result.status).toBe("success");
      const bytes = await readFile(path.join(workspace.root, "docs", "note.md"));
      expect(bytes.toString("utf8")).toBe("changed\n");
    },
  );
});

describe("workspace.edit_file replacement recovery", () => {
  function failingOps(
    failRenameCalls: readonly number[] = [],
    failReadFileCalls: readonly number[] = [],
  ): { replacementOps: ReplacementFsOps } {
    let renameCalls = 0;
    let readFileCalls = 0;
    return {
      replacementOps: {
        async rename(from: string, to: string) {
          renameCalls += 1;
          if (failRenameCalls.includes(renameCalls)) {
            throw new Error(`injected rename failure (call ${renameCalls})`);
          }
          const { rename } = await import("node:fs/promises");
          await rename(from, to);
        },
        async link(from: string, to: string) {
          const { link } = await import("node:fs/promises");
          await link(from, to);
        },
        async unlink(p: string) {
          const { unlink } = await import("node:fs/promises");
          await unlink(p);
        },
        async readFile(p: string) {
          readFileCalls += 1;
          if (failReadFileCalls.includes(readFileCalls)) {
            throw new Error(`injected read failure (call ${readFileCalls})`);
          }
          const fs = await import("node:fs/promises");
          return fs.readFile(p);
        },
        async lstat(p: string) {
          const { lstat } = await import("node:fs/promises");
          return lstat(p);
        },
        async rm(p: string) {
          const { rm } = await import("node:fs/promises");
          await rm(p, { force: true });
        },
      },
    };
  }

  it("commits through quarantine on every platform and cleans up after finalization", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const { tool } = await createTool(workspace.root, failingOps());
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("success");
    const content = await readFile(path.join(workspace.root, "a.txt"), "utf8");
    expect(content).toBe("changed\n");
    const entries = await (await import("node:fs/promises")).readdir(workspace.root);
    expect(entries.some((entry) => entry.startsWith(".solaris-quarantine-"))).toBe(false);
    expect(entries.some((entry) => entry.startsWith(".solaris-mutation-"))).toBe(false);
  });

  it("preserves a later user edit made between final revalidation and the commit displacement", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    let committed = false;
    const { tool } = await createTool(workspace.root, {
      replacementOps: {
        ...failingOps().replacementOps,
        async rename(from: string, to: string) {
          if (!committed && to.includes(".solaris-quarantine-")) {
            // Simulate a concurrent external edit landing on the target
            // immediately before the commit displacement.
            const fs = await import("node:fs/promises");
            await fs.writeFile(from, "concurrent user edit\n");
            committed = true;
          }
          await failingOps().replacementOps.rename(from, to);
        },
      },
    });
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("failed");
    const content = await readFile(path.join(workspace.root, "a.txt"), "utf8");
    expect(content).toBe("concurrent user edit\n");
  });

  it("reports an uncertain finalize failure with the recoverable quarantine named", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const store = await createTempCheckpointStore(workspace.root);
    const guardedStore = new Proxy(store, {
      get(target, property: keyof CheckpointStore) {
        if (property === "finalizeApplied") {
          return () => Promise.reject(new Error("disk full"));
        }
        // eslint-disable-next-line @typescript-eslint/unbound-method -- store methods are closures without this
        return target[property] as never;
      },
    });
    const tool = createWorkspaceEditFileTool(workspace.root, createMutationLock(), guardedStore);
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("failed");
    if (result.status !== "failed") {
      return;
    }
    expect(result.message).toContain("recovery state is uncertain");
    expect(result.message).toContain(".solaris-quarantine-");
    const content = await readFile(path.join(workspace.root, "a.txt"), "utf8");
    expect(content).toBe("changed\n");
    const entries = await (await import("node:fs/promises")).readdir(workspace.root);
    expect(entries.some((entry) => entry.startsWith(".solaris-quarantine-"))).toBe(true);
  });

  it("surfaces quarantine cleanup failure without deleting the recoverable copy", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const { tool } = await createTool(workspace.root, {
      replacementOps: {
        ...failingOps().replacementOps,
        rm(p: string) {
          throw new Error(`injected rm failure: ${p}`);
        },
      },
    });
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("success");
    if (result.status !== "success") {
      return;
    }
    const output = result.output as Record<string, unknown> | null;
    expect(output?.["cleanupWarning"]).toContain(".solaris-quarantine-");
    const entries = await (await import("node:fs/promises")).readdir(workspace.root);
    expect(entries.some((entry) => entry.startsWith(".solaris-quarantine-"))).toBe(true);
  });

  it("detects a tampered staged temp file before the commit completes", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const { tool } = await createTool(workspace.root, {
      replacementOps: {
        ...failingOps().replacementOps,
        async link(from: string, to: string) {
          if (to.endsWith("a.txt") && from.includes(".solaris-mutation-")) {
            // The staged content is swapped for different bytes right
            // before the exclusive commit link consumes it (same inode).
            const fs = await import("node:fs/promises");
            await fs.writeFile(from, "injected staged content\n");
          }
          await failingOps().replacementOps.link(from, to);
        },
      },
    });
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("failed");
    if (result.status !== "failed") {
      return;
    }
    expect(result.message).toContain("does not match the expected staged content hash");
    // The committed object was rolled back: the original was restored and
    // no quarantine or temp file remains.
    const content = await readFile(path.join(workspace.root, "a.txt"), "utf8");
    expect(content).toBe("original\n");
    const entries = await (await import("node:fs/promises")).readdir(workspace.root);
    expect(entries.some((entry) => entry.startsWith(".solaris-quarantine-"))).toBe(false);
    expect(entries.some((entry) => entry.startsWith(".solaris-mutation-"))).toBe(false);
  });

  it("keeps the original recoverable when post-commit verification fails", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const store = await createTempCheckpointStore(workspace.root);
    const tool = createWorkspaceEditFileTool(workspace.root, createMutationLock(), store, {
      replacementOps: {
        ...failingOps().replacementOps,
        async link(from: string, to: string) {
          await failingOps().replacementOps.link(from, to);
          if (to.endsWith("a.txt")) {
            // The committed object is replaced by a different inode right
            // after the exclusive link, so the staged-hash verification
            // fails and rollback cannot prove the target is ours.
            const fs = await import("node:fs/promises");
            await fs.rm(to, { force: true });
            await fs.writeFile(to, "tampered after commit\n");
          }
        },
      },
    });
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("failed");
    if (result.status !== "failed") {
      return;
    }
    expect(result.message).toContain("does not match the expected staged content hash");
    expect(result.message).toContain(".solaris-quarantine-");
    expect(await readFile(path.join(workspace.root, "a.txt"), "utf8")).toBe(
      "tampered after commit\n",
    );
    const entries = await (await import("node:fs/promises")).readdir(workspace.root);
    expect(entries.some((entry) => entry.startsWith(".solaris-quarantine-"))).toBe(true);
  });

  it("never overwrites a target that reappears before the commit link", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const { tool } = await createTool(workspace.root, {
      replacementOps: {
        ...failingOps().replacementOps,
        async link(from: string, to: string) {
          if (to.endsWith("a.txt")) {
            // A new file lands at the target after the displacement and
            // quarantine verification, immediately before the commit link.
            const fs = await import("node:fs/promises");
            await fs.writeFile(to, "raced replacement\n");
          }
          await failingOps().replacementOps.link(from, to);
        },
      },
    });
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("failed");
    if (result.status !== "failed") {
      return;
    }
    expect(result.message).toContain(".solaris-quarantine-");
    expect(await readFile(path.join(workspace.root, "a.txt"), "utf8")).toBe("raced replacement\n");
    const entries = await (await import("node:fs/promises")).readdir(workspace.root);
    expect(entries.some((entry) => entry.startsWith(".solaris-quarantine-"))).toBe(true);
  });

  it("fails closed without touching the target when the quarantine cannot be created", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const { tool } = await createTool(workspace.root, failingOps([1, 2]));
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("failed");
    const content = await readFile(path.join(workspace.root, "a.txt"), "utf8");
    expect(content).toBe("original\n");
  });
});

describe("workspace.edit_file commit-point cancellation", () => {
  it("cancels before the commit point without changing the target", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const { tool } = await createTool(workspace.root);
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const controller = new AbortController();
    controller.abort();
    const result = await tool.apply(prepared.mutation, {
      approvedDigest: prepared.digest,
      signal: controller.signal,
    });
    expect(result.status).toBe("cancelled");
    const content = await readFile(path.join(workspace.root, "a.txt"), "utf8");
    expect(content).toBe("original\n");
    const entries = await (await import("node:fs/promises")).readdir(workspace.root);
    expect(entries.some((entry) => entry.startsWith(".solaris-mutation-"))).toBe(false);
  });

  it("finalizes lifecycle state even when cancellation arrives after the commit point", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const store = await createTempCheckpointStore(workspace.root);
    const controller = new AbortController();
    const guardedStore = new Proxy(store, {
      get(target, property: keyof CheckpointStore) {
        if (property === "finalizeApplied") {
          return (
            checkpointId: string,
            result: Parameters<CheckpointStore["finalizeApplied"]>[1],
          ) => {
            controller.abort();
            const finalize = target.finalizeApplied.bind(target);
            return finalize(checkpointId, result);
          };
        }
        // eslint-disable-next-line @typescript-eslint/unbound-method -- store methods are closures without this
        return target[property] as never;
      },
    });
    const tool = createWorkspaceEditFileTool(workspace.root, createMutationLock(), guardedStore);
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, {
      approvedDigest: prepared.digest,
      signal: controller.signal,
    });
    expect(result.status).toBe("success");
    expect(controller.signal.aborted).toBe(true);
    const checkpoints = await store.list();
    expect(checkpoints[0]?.state).toBe("applied");
    const content = await readFile(path.join(workspace.root, "a.txt"), "utf8");
    expect(content).toBe("changed\n");
  });

  it(
    "detects a parent directory swapped to a symlink after preparation",
    {
      skip: !SYMLINKS_SUPPORTED,
    },
    async () => {
      const workspace = await withWorkspace();
      await writeFixtureFiles(workspace.root, { "docs/a.txt": "original\n" });
      const hash = await hashOf(path.join(workspace.root, "docs", "a.txt"));
      const { tool } = await createTool(workspace.root);
      const prepared = await prepareEdit(tool, "docs/a.txt", hash, [
        { oldText: "original", newText: "changed" },
      ]);
      expect(prepared.status).toBe("ready");
      if (prepared.status !== "ready") {
        return;
      }
      const outside = await createTempWorkspace();
      workspaces.push(outside);
      const { rm } = await import("node:fs/promises");
      await rm(path.join(workspace.root, "docs"), { recursive: true });
      await createSymlink(outside.root, path.join(workspace.root, "docs"));
      const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
      expect(result.status).toBe("conflict");
    },
  );
});
