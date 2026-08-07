import { afterEach, describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import path from "node:path";
import { createWorkspaceReadTool } from "./workspace-read-tool.js";
import { WORKSPACE_LIMITS } from "./limits.js";
import {
  createSymlink,
  createTempWorkspace,
  expectSuccess,
  fieldBoolean,
  fieldNumber,
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

afterEach(async () => {
  for (const workspace of workspaces.splice(0)) {
    await workspace.cleanup();
  }
});

describe("workspace.read", () => {
  it("reads a normal UTF-8 file", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "line one\nline two\n" });
    const tool = createWorkspaceReadTool(workspace.root);
    const result = await tool.execute({ path: "a.txt" }, {});
    const output = expectSuccess(result);
    expect(output["path"]).toBe("a.txt");
    expect(stringOf(output["content"])).toBe("line one\nline two");
    expect(fieldNumber(output, "startLine")).toBe(1);
    expect(fieldNumber(output, "endLine")).toBe(2);
    expect(fieldNumber(output, "totalLines")).toBe(2);
    expect(fieldBoolean(output, "truncated")).toBe(false);
  });

  it("reads a requested line range", async () => {
    const workspace = await withWorkspace();
    const content = Array.from({ length: 10 }, (_, index) => `line ${index + 1}`).join("\n");
    await writeFixtureFiles(workspace.root, { "a.txt": content });
    const tool = createWorkspaceReadTool(workspace.root);
    const result = await tool.execute({ path: "a.txt", startLine: 3, endLine: 5 }, {});
    const output = expectSuccess(result);
    expect(stringOf(output["content"])).toBe("line 3\nline 4\nline 5");
    expect(fieldNumber(output, "startLine")).toBe(3);
    expect(fieldNumber(output, "endLine")).toBe(5);
    expect(fieldNumber(output, "totalLines")).toBe(10);
  });

  it("clamps an endLine beyond the file length", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "one\ntwo\n" });
    const tool = createWorkspaceReadTool(workspace.root);
    const result = await tool.execute({ path: "a.txt", endLine: 999 }, {});
    const output = expectSuccess(result);
    expect(fieldNumber(output, "endLine")).toBe(2);
    expect(fieldNumber(output, "totalLines")).toBe(2);
  });

  it("handles a file without a trailing newline", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "hello" });
    const tool = createWorkspaceReadTool(workspace.root);
    const result = await tool.execute({ path: "a.txt" }, {});
    const output = expectSuccess(result);
    expect(stringOf(output["content"])).toBe("hello");
    expect(fieldNumber(output, "startLine")).toBe(1);
    expect(fieldNumber(output, "endLine")).toBe(1);
    expect(fieldNumber(output, "totalLines")).toBe(1);
  });

  it("rejects invalid line ranges", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "one\ntwo\n" });
    const tool = createWorkspaceReadTool(workspace.root);
    expect((await tool.execute({ path: "a.txt", startLine: 0 }, {})).status).toBe("invalid_input");
    expect((await tool.execute({ path: "a.txt", startLine: 2, endLine: 1 }, {})).status).toBe(
      "invalid_input",
    );
    expect((await tool.execute({ path: "a.txt", startLine: 1.5 }, {})).status).toBe(
      "invalid_input",
    );
    expect((await tool.execute({ path: "a.txt", endLine: "three" }, {})).status).toBe(
      "invalid_input",
    );
  });

  it("returns the complete-file SHA-256 for line ranges", async () => {
    const workspace = await withWorkspace();
    const content = "line one\nline two\nline three\n";
    await writeFixtureFiles(workspace.root, { "a.txt": content });
    const tool = createWorkspaceReadTool(workspace.root);
    const full = await tool.execute({ path: "a.txt" }, {});
    const ranged = await tool.execute({ path: "a.txt", startLine: 1, endLine: 1 }, {});
    const fullOutput = expectSuccess(full);
    const rangedOutput = expectSuccess(ranged);
    const expectedHash = createHash("sha256").update(content).digest("hex");
    expect(fullOutput["sha256"]).toBe(expectedHash);
    expect(rangedOutput["sha256"]).toBe(expectedHash);
    expect(stringOf(rangedOutput["content"])).toBe("line one");
  });

  it("produces different hashes for different bytes", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "same bytes" });
    const tool = createWorkspaceReadTool(workspace.root);
    const first = expectSuccess(await tool.execute({ path: "a.txt" }, {}));
    await writeFixtureFiles(workspace.root, { "a.txt": "same bytEZ" });
    const second = expectSuccess(await tool.execute({ path: "a.txt" }, {}));
    expect(first["sha256"]).not.toBe(second["sha256"]);
  });

  it("rejects a startLine beyond the file", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "one\n" });
    const tool = createWorkspaceReadTool(workspace.root);
    const result = await tool.execute({ path: "a.txt", startLine: 5 }, {});
    expect(result.status).toBe("failed");
  });

  it("rejects directories", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "dir/file.txt": "x" });
    const tool = createWorkspaceReadTool(workspace.root);
    const result = await tool.execute({ path: "dir" }, {});
    expect(result.status).toBe("failed");
    if (result.status === "failed") {
      expect(result.message).toContain("not a regular file");
    }
  });

  it("rejects binary files", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "bin.dat": Buffer.from([0x00, 0x01, 0x02, 0x03]) });
    const tool = createWorkspaceReadTool(workspace.root);
    const result = await tool.execute({ path: "bin.dat" }, {});
    expect(result.status).toBe("failed");
    if (result.status === "failed") {
      expect(result.message).toContain("binary");
    }
  });

  it("rejects oversized files", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, {
      "big.txt": Buffer.alloc(WORKSPACE_LIMITS.maxReadFileSizeBytes + 1, 0x61),
    });
    const tool = createWorkspaceReadTool(workspace.root);
    const result = await tool.execute({ path: "big.txt" }, {});
    expect(result.status).toBe("failed");
    if (result.status === "failed") {
      expect(result.message).toContain("too large");
    }
  });

  it("marks truncated content", async () => {
    const workspace = await withWorkspace();
    const oversizedLine = "x".repeat(WORKSPACE_LIMITS.maxReadContentChars + 1000);
    await writeFixtureFiles(workspace.root, { "long.txt": `${oversizedLine}\n` });
    const tool = createWorkspaceReadTool(workspace.root);
    const result = await tool.execute({ path: "long.txt" }, {});
    const output = expectSuccess(result);
    expect(fieldBoolean(output, "truncated")).toBe(true);
    expect(stringOf(output["content"]).length).toBe(WORKSPACE_LIMITS.maxReadContentChars);
  });

  it("rejects paths outside the workspace", async () => {
    const workspace = await withWorkspace();
    const tool = createWorkspaceReadTool(workspace.root);
    const result = await tool.execute({ path: "../secret.txt" }, {});
    expect(result.status).toBe("denied");
  });

  it("rejects reading inside excluded directories", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "node_modules/pkg/index.js": "x" });
    const tool = createWorkspaceReadTool(workspace.root);
    const result = await tool.execute({ path: "node_modules/pkg/index.js" }, {});
    expect(result.status).toBe("denied");
  });

  it("requires a path", async () => {
    const workspace = await withWorkspace();
    const tool = createWorkspaceReadTool(workspace.root);
    expect((await tool.execute({}, {})).status).toBe("invalid_input");
    expect((await tool.execute({ path: "" }, {})).status).toBe("invalid_input");
  });

  it("rejects a symlink that escapes the workspace", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const workspace = await withWorkspace();
    const outside = await createTempWorkspace();
    workspaces.push(outside);
    await writeFixtureFiles(outside.root, { "secret.txt": "secret" });
    await createSymlink(
      path.join(outside.root, "secret.txt"),
      path.join(workspace.root, "escape.txt"),
    );
    const tool = createWorkspaceReadTool(workspace.root);
    const result = await tool.execute({ path: "escape.txt" }, {});
    expect(result.status).toBe("denied");
  });

  it("responds to cancellation", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "hello\n" });
    const controller = new AbortController();
    controller.abort();
    const tool = createWorkspaceReadTool(workspace.root);
    const result = await tool.execute({ path: "a.txt" }, { signal: controller.signal });
    expect(result.status).toBe("cancelled");
  });
});
