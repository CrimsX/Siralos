import { afterEach, describe, expect, it } from "vitest";
import path from "node:path";
import { createWorkspaceListTool } from "./workspace-list-tool.js";
import { WORKSPACE_LIMITS } from "./limits.js";
import {
  createFile,
  createSymlink,
  createTempWorkspace,
  expectSuccess,
  fieldArray,
  fieldBoolean,
  objectOf,
  stringOf,
  SYMLINKS_SUPPORTED,
  writeFixtureFiles,
  type TempWorkspace,
} from "./workspace-fixtures.js";

const workspaces: TempWorkspace[] = [];

async function withWorkspace(): Promise<TempWorkspace> {
  const workspace = await createTempWorkspace();
  workspaces.push(workspace);
  return workspace;
}

function entryNames(result: Parameters<typeof expectSuccess>[0]): string[] {
  const output = expectSuccess(result);
  return fieldArray(output, "entries").map((entry) => stringOf(objectOf(entry)["name"]));
}

afterEach(async () => {
  for (const workspace of workspaces.splice(0)) {
    await workspace.cleanup();
  }
});

describe("workspace.list", () => {
  it("lists direct children with types and sizes", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, {
      "README.md": "hello",
      "packages/core/index.ts": "x",
    });
    const tool = createWorkspaceListTool(workspace.root);
    const result = await tool.execute({ path: "." }, {});
    const output = expectSuccess(result);
    expect(output["path"]).toBe(".");
    expect(fieldBoolean(output, "truncated")).toBe(false);
    expect(fieldArray(output, "entries")).toEqual([
      { name: "README.md", path: "README.md", type: "file", size: 5 },
      { name: "packages", path: "packages", type: "directory" },
    ]);
  });

  it("defaults the path to the workspace root", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "x" });
    const tool = createWorkspaceListTool(workspace.root);
    const result = await tool.execute({}, {});
    const output = expectSuccess(result);
    expect(output["path"]).toBe(".");
    expect(fieldArray(output, "entries")).toEqual([
      { name: "a.txt", path: "a.txt", type: "file", size: 1 },
    ]);
  });

  it("sorts results deterministically", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, {
      "b.txt": "x",
      "a.txt": "x",
      "c.txt": "x",
    });
    const tool = createWorkspaceListTool(workspace.root);
    const first = await tool.execute({ path: "." }, {});
    const second = await tool.execute({ path: "." }, {});
    expect(first).toEqual(second);
    expect(entryNames(first)).toEqual(["a.txt", "b.txt", "c.txt"]);
  });

  it("applies default exclusions by component", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, {
      "keep.txt": "x",
      "node_modules/pkg/index.js": "x",
      ".git/config": "x",
      "dist/out.js": "x",
      "coverage/lcov.info": "x",
    });
    const tool = createWorkspaceListTool(workspace.root);
    const result = await tool.execute({ path: "." }, {});
    expect(entryNames(result)).toEqual(["keep.txt"]);
  });

  it("does not recurse into subdirectories", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, {
      "packages/core/index.ts": "x",
      "packages/core/deep.ts": "x",
    });
    const tool = createWorkspaceListTool(workspace.root);
    const result = await tool.execute({ path: "packages" }, {});
    expect(fieldArray(expectSuccess(result), "entries")).toEqual([
      { name: "core", path: "packages/core", type: "directory" },
    ]);
  });

  it("lists symlink entries without following them", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "real.txt": "x" });
    await createSymlink(
      path.join(workspace.root, "real.txt"),
      path.join(workspace.root, "link.txt"),
    );
    const tool = createWorkspaceListTool(workspace.root);
    const result = await tool.execute({ path: "." }, {});
    const linkEntry = fieldArray(expectSuccess(result), "entries").find(
      (entry) => objectOf(entry)["name"] === "link.txt",
    );
    expect(linkEntry).toMatchObject({ type: "symlink" });
  });

  it("rejects a file target", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "file.txt": "x" });
    const tool = createWorkspaceListTool(workspace.root);
    const result = await tool.execute({ path: "file.txt" }, {});
    expect(result.status).toBe("failed");
    if (result.status === "failed") {
      expect(result.message).toContain("not a directory");
    }
  });

  it("rejects paths outside the workspace", async () => {
    const workspace = await withWorkspace();
    const tool = createWorkspaceListTool(workspace.root);
    const result = await tool.execute({ path: "../outside" }, {});
    expect(result.status).toBe("denied");
  });

  it("rejects excluded directories as targets", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "node_modules/pkg/package.json": "{}" });
    const tool = createWorkspaceListTool(workspace.root);
    const result = await tool.execute({ path: "node_modules" }, {});
    expect(result.status).toBe("denied");
  });

  it("rejects non-object input", async () => {
    const workspace = await withWorkspace();
    const tool = createWorkspaceListTool(workspace.root);
    const result = await tool.execute(42, {});
    expect(result.status).toBe("invalid_input");
  });

  it("truncates listings at the entry limit", async () => {
    const workspace = await withWorkspace();
    for (let index = 0; index < WORKSPACE_LIMITS.maxDirectoryEntries + 1; index += 1) {
      await createFile(workspace.root, `file-${index}.txt`, "x");
    }
    const tool = createWorkspaceListTool(workspace.root);
    const result = await tool.execute({ path: "." }, {});
    const output = expectSuccess(result);
    expect(fieldBoolean(output, "truncated")).toBe(true);
    expect(fieldArray(output, "entries")).toHaveLength(WORKSPACE_LIMITS.maxDirectoryEntries);
  });
});
