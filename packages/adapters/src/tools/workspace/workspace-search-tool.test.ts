import { afterEach, describe, expect, it } from "vitest";
import path from "node:path";
import { createWorkspaceSearchTool } from "./workspace-search-tool.js";
import { WORKSPACE_LIMITS } from "./limits.js";
import {
  createFile,
  createSymlink,
  createTempWorkspace,
  expectSuccess,
  fieldArray,
  fieldBoolean,
  fieldNumber,
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

function matchPaths(result: Parameters<typeof expectSuccess>[0]): string[] {
  const output = expectSuccess(result);
  return fieldArray(output, "matches").map((match) => stringOf(objectOf(match)["path"]));
}

afterEach(async () => {
  for (const workspace of workspaces.splice(0)) {
    await workspace.cleanup();
  }
});

describe("workspace.search", () => {
  it("finds literal matches with line and column metadata", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, {
      "a.txt": "first line\nhas needle here\nlast\n",
    });
    const tool = createWorkspaceSearchTool(workspace.root);
    const result = await tool.execute({ query: "needle", path: "." }, {});
    const output = expectSuccess(result);
    expect(output["query"]).toBe("needle");
    expect(output["path"]).toBe(".");
    expect(fieldNumber(output, "scannedFiles")).toBe(1);
    expect(fieldNumber(output, "skippedFiles")).toBe(0);
    expect(fieldBoolean(output, "truncated")).toBe(false);
    expect(fieldArray(output, "matches")).toEqual([
      { path: "a.txt", line: 2, column: 5, text: "has needle here" },
    ]);
  });

  it("searches recursively", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, {
      "root.txt": "needle\n",
      "packages/core/deep.txt": "deep needle\n",
    });
    const tool = createWorkspaceSearchTool(workspace.root);
    const result = await tool.execute({ query: "needle" }, {});
    expect(matchPaths(result).sort()).toEqual(["packages/core/deep.txt", "root.txt"]);
  });

  it("applies directory exclusions", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, {
      "keep.txt": "needle\n",
      "node_modules/pkg/index.js": "needle\n",
      "dist/out.js": "needle\n",
      ".git/config": "needle\n",
    });
    const tool = createWorkspaceSearchTool(workspace.root);
    const result = await tool.execute({ query: "needle" }, {});
    expect(matchPaths(result)).toEqual(["keep.txt"]);
  });

  it("skips binary files", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, {
      "bin.dat": Buffer.from([0x00, 0x01, 0x02]),
      "a.txt": "needle\n",
    });
    const tool = createWorkspaceSearchTool(workspace.root);
    const result = await tool.execute({ query: "needle" }, {});
    expect(matchPaths(result)).toEqual(["a.txt"]);
    expect(fieldNumber(expectSuccess(result), "skippedFiles")).toBe(1);
  });

  it("skips oversized files", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, {
      "big.txt": Buffer.alloc(WORKSPACE_LIMITS.maxSearchFileSizeBytes + 1, 0x61),
      "a.txt": "needle\n",
    });
    const tool = createWorkspaceSearchTool(workspace.root);
    const result = await tool.execute({ query: "needle" }, {});
    expect(matchPaths(result)).toEqual(["a.txt"]);
    expect(fieldNumber(expectSuccess(result), "skippedFiles")).toBe(1);
  });

  it("does not follow symlink directories", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const workspace = await withWorkspace();
    const outside = await createTempWorkspace();
    workspaces.push(outside);
    await writeFixtureFiles(outside.root, { "secret.txt": "needle\n" });
    await createSymlink(
      path.join(outside.root, "secret.txt"),
      path.join(workspace.root, "linked.txt"),
    );
    await writeFixtureFiles(workspace.root, { "a.txt": "needle\n" });
    const tool = createWorkspaceSearchTool(workspace.root);
    const result = await tool.execute({ query: "needle" }, {});
    expect(matchPaths(result)).toEqual(["a.txt"]);
  });

  it("stops at the requested match limit", async () => {
    const workspace = await withWorkspace();
    const content = Array.from({ length: 10 }, () => "needle").join("\n");
    await writeFixtureFiles(workspace.root, { "a.txt": `${content}\n` });
    const tool = createWorkspaceSearchTool(workspace.root);
    const result = await tool.execute({ query: "needle", maxResults: 3 }, {});
    const output = expectSuccess(result);
    expect(fieldArray(output, "matches")).toHaveLength(3);
    expect(fieldBoolean(output, "truncated")).toBe(true);
  });

  it("stops at the file-scan limit", async () => {
    const workspace = await withWorkspace();
    for (let index = 0; index < WORKSPACE_LIMITS.maxSearchFiles + 1; index += 1) {
      await createFile(workspace.root, `file-${index}.txt`, "no matches here\n");
    }
    const tool = createWorkspaceSearchTool(workspace.root);
    const result = await tool.execute({ query: "needle" }, {});
    const output = expectSuccess(result);
    expect(fieldNumber(output, "scannedFiles")).toBe(WORKSPACE_LIMITS.maxSearchFiles);
    expect(fieldBoolean(output, "truncated")).toBe(true);
  });

  it("truncates long matching lines", async () => {
    const workspace = await withWorkspace();
    const longLine = `needle${"x".repeat(WORKSPACE_LIMITS.maxSearchLineLengthChars + 500)}`;
    await writeFixtureFiles(workspace.root, { "a.txt": `${longLine}\n` });
    const tool = createWorkspaceSearchTool(workspace.root);
    const result = await tool.execute({ query: "needle" }, {});
    const match = fieldArray(expectSuccess(result), "matches")[0];
    expect(match).toBeDefined();
    if (match === undefined) {
      return;
    }
    expect(stringOf(objectOf(match)["text"]).length).toBe(
      WORKSPACE_LIMITS.maxSearchLineLengthChars,
    );
  });

  it("responds to cancellation", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "needle\n" });
    const controller = new AbortController();
    controller.abort();
    const tool = createWorkspaceSearchTool(workspace.root);
    const result = await tool.execute({ query: "needle" }, { signal: controller.signal });
    expect(result.status).toBe("cancelled");
  });

  it("returns deterministic ordering", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, {
      "b.txt": "needle\n",
      "a.txt": "needle\nneedle\n",
    });
    const tool = createWorkspaceSearchTool(workspace.root);
    const first = await tool.execute({ query: "needle" }, {});
    const second = await tool.execute({ query: "needle" }, {});
    expect(first).toEqual(second);
  });

  it("requires a non-empty query", async () => {
    const workspace = await withWorkspace();
    const tool = createWorkspaceSearchTool(workspace.root);
    expect((await tool.execute({}, {})).status).toBe("invalid_input");
    expect((await tool.execute({ query: "" }, {})).status).toBe("invalid_input");
    expect((await tool.execute({ query: 42 }, {})).status).toBe("invalid_input");
  });

  it("rejects paths outside the workspace", async () => {
    const workspace = await withWorkspace();
    const tool = createWorkspaceSearchTool(workspace.root);
    const result = await tool.execute({ query: "needle", path: ".." }, {});
    expect(result.status).toBe("denied");
  });

  it("matches case-sensitively", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "Needle\nneedle\n" });
    const tool = createWorkspaceSearchTool(workspace.root);
    const result = await tool.execute({ query: "needle" }, {});
    const matches = fieldArray(expectSuccess(result), "matches");
    expect(matches).toHaveLength(1);
    const first = matches[0];
    expect(first).toBeDefined();
    if (first === undefined) {
      return;
    }
    expect(objectOf(first)["line"]).toBe(2);
  });
});
