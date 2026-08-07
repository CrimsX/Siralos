import { afterEach, describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import type { ToolPreparationResult } from "@solaris/core";
import { createWorkspaceEditFileTool } from "./workspace-edit-file-tool.js";
import { createMutationLock } from "./mutation-lock.js";
import { WORKSPACE_LIMITS } from "../limits.js";
import {
  createSymlink,
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

function createTool(workspaceRoot: string) {
  return createWorkspaceEditFileTool(workspaceRoot, createMutationLock());
}

async function hashOf(absolutePath: string): Promise<string> {
  const bytes = await readFile(absolutePath);
  return createHash("sha256").update(bytes).digest("hex");
}

async function prepareEdit(
  tool: ReturnType<typeof createTool>,
  filePath: string,
  hash: string,
  replacements: readonly { oldText: string; newText: string }[],
): Promise<ToolPreparationResult> {
  return tool.prepare({ path: filePath, expectedSha256: hash, replacements }, {});
}

afterEach(async () => {
  for (const workspace of workspaces.splice(0)) {
    await workspace.cleanup();
  }
});

describe("workspace.edit_file", () => {
  it("applies one exact replacement", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "Created text here\n" });
    const tool = createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "Created", newText: "Updated" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, {});
    const output = expectSuccess(result);
    expect(output["operation"]).toBe("update");
    const bytes = await readFile(path.join(workspace.root, "a.txt"));
    expect(bytes.toString("utf8")).toBe("Updated text here\n");
    expect(output["afterSha256"]).toBe(createHash("sha256").update(bytes).digest("hex"));
  });

  it("applies several sequential replacements", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "one two three\n" });
    const tool = createTool(workspace.root);
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
    const result = await tool.apply(prepared.mutation, {});
    expect(result.status).toBe("success");
    const bytes = await readFile(path.join(workspace.root, "a.txt"));
    expect(bytes.toString("utf8")).toBe("1 2 3\n");
  });

  it("produces a complete update preview", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "old line\n" });
    const tool = createTool(workspace.root);
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
    const tool = createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [{ oldText: "missing", newText: "x" }]);
    expect(prepared).toMatchObject({
      status: "conflict",
      message: expect.stringContaining("not found"),
    });
  });

  it("fails with multiple matches", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "dup\ndup\n" });
    const tool = createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [{ oldText: "dup", newText: "x" }]);
    expect(prepared).toMatchObject({
      status: "conflict",
      message: expect.stringContaining("ambiguous"),
    });
  });

  it("rejects empty oldText", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "hello\n" });
    const tool = createTool(workspace.root);
    const prepared = await prepareEdit(tool, "a.txt", "a".repeat(64), [
      { oldText: "", newText: "x" },
    ]);
    expect(prepared).toMatchObject({ status: "invalid_input" });
  });

  it("rejects a no-op result", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "same\n" });
    const tool = createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [{ oldText: "same", newText: "same" }]);
    expect(prepared).toMatchObject({
      status: "failed",
      message: expect.stringContaining("identical"),
    });
  });

  it("rejects protected files", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { ".env": "KEY=value\n" });
    const tool = createTool(workspace.root);
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
    const tool = createTool(workspace.root);
    const prepared = await prepareEdit(tool, "link.txt", "a".repeat(64), [
      { oldText: "hello", newText: "bye" },
    ]);
    expect(prepared).toMatchObject({ status: "denied" });
  });

  it("rejects binary files", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "bin.dat": Buffer.from([0x00, 0x01, 0x02]) });
    const tool = createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "bin.dat"));
    const prepared = await prepareEdit(tool, "bin.dat", hash, [{ oldText: "a", newText: "b" }]);
    expect(prepared).toMatchObject({
      status: "failed",
      message: expect.stringContaining("binary"),
    });
  });

  it("rejects oversized files", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, {
      "big.txt": Buffer.alloc(WORKSPACE_LIMITS.maxTextFileSizeBytes + 1, 0x61),
    });
    const tool = createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "big.txt"));
    const prepared = await prepareEdit(tool, "big.txt", hash, [{ oldText: "a", newText: "b" }]);
    expect(prepared).toMatchObject({
      status: "failed",
      message: expect.stringContaining("too large"),
    });
  });

  it("conflicts on a stale hash during preparation", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const tool = createTool(workspace.root);
    const prepared = await prepareEdit(tool, "a.txt", "f".repeat(64), [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared).toMatchObject({
      status: "conflict",
      message: expect.stringContaining("reread"),
    });
  });

  it("conflicts when the file changes after approval", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const tool = createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    await writeFixtureFiles(workspace.root, { "a.txt": "external edit\n" });
    const result = await tool.apply(prepared.mutation, {});
    expect(result.status).toBe("conflict");
    const bytes = await readFile(path.join(workspace.root, "a.txt"));
    expect(bytes.toString("utf8")).toBe("external edit\n");
  });

  it("conflicts when the file disappears after approval", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const tool = createTool(workspace.root);
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
    const result = await tool.apply(prepared.mutation, {});
    expect(result.status).toBe("conflict");
  });

  it(
    "conflicts when the target becomes a symbolic link",
    { skip: !SYMLINKS_SUPPORTED },
    async () => {
      const workspace = await withWorkspace();
      await writeFixtureFiles(workspace.root, { "a.txt": "original\n", "other.txt": "other\n" });
      const tool = createTool(workspace.root);
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
      const result = await tool.apply(prepared.mutation, {});
      expect(result.status).toBe("conflict");
    },
  );

  it("verifies the final hash and leaves no temporary files", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const tool = createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, {});
    expect(result.status).toBe("success");
    const entries = await (await import("node:fs/promises")).readdir(workspace.root);
    expect(entries.some((entry) => entry.startsWith(".solaris-mutation-"))).toBe(false);
  });

  it("cannot be applied twice", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "original\n" });
    const tool = createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "a.txt"));
    const prepared = await prepareEdit(tool, "a.txt", hash, [
      { oldText: "original", newText: "changed" },
    ]);
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    expect((await tool.apply(prepared.mutation, {})).status).toBe("success");
    expect((await tool.apply(prepared.mutation, {})).status).toBe("failed");
  });

  it("rejects replacements beyond the limit", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "x\n" });
    const tool = createTool(workspace.root);
    const replacements = Array.from({ length: WORKSPACE_LIMITS.maxReplacements + 1 }, () => ({
      oldText: "x",
      newText: "y",
    }));
    const prepared = await prepareEdit(tool, "a.txt", "a".repeat(64), replacements);
    expect(prepared).toMatchObject({ status: "invalid_input" });
  });
});
