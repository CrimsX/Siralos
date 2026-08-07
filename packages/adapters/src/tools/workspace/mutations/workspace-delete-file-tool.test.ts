import { afterEach, describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import {
  createDefaultPolicy,
  createSolarisApplication,
  createToolRegistry,
  DEVELOP_OFFLINE_PROFILE,
  type ApprovalReviewer,
} from "@solaris/core";
import { createWorkspaceDeleteFileTool } from "./workspace-delete-file-tool.js";
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
  return createWorkspaceDeleteFileTool(workspaceRoot, createMutationLock());
}

async function hashOf(absolutePath: string): Promise<string> {
  const bytes = await readFile(absolutePath);
  return createHash("sha256").update(bytes).digest("hex");
}

afterEach(async () => {
  for (const workspace of workspaces.splice(0)) {
    await workspace.cleanup();
  }
});

describe("workspace.delete_file", () => {
  it("prepares a complete deletion preview", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "obsolete.md": "line one\nline two\n" });
    const tool = createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "obsolete.md"));
    const prepared = await tool.prepare({ path: "obsolete.md", expectedSha256: hash }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    expect(prepared.preview.files[0]).toMatchObject({
      path: "obsolete.md",
      operation: "delete",
      beforeSha256: hash,
      afterSha256: null,
      addedLines: 0,
      removedLines: 2,
    });
    expect(prepared.preview.files[0]?.unifiedDiff).toContain("-line one");
  });

  it("deletes the approved file and verifies absence", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "obsolete.md": "line one\n" });
    const tool = createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "obsolete.md"));
    const prepared = await tool.prepare({ path: "obsolete.md", expectedSha256: hash }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, {});
    const output = expectSuccess(result);
    expect(output["operation"]).toBe("delete");
    expect(output["removedLines"]).toBe(1);
    await expect(readFile(path.join(workspace.root, "obsolete.md"))).rejects.toThrow();
  });

  it("conflicts on a stale hash", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "obsolete.md": "line one\n" });
    const tool = createTool(workspace.root);
    const prepared = await tool.prepare(
      { path: "obsolete.md", expectedSha256: "f".repeat(64) },
      {},
    );
    expect(prepared).toMatchObject({ status: "conflict" });
    if (prepared.status === "conflict") {
      expect(prepared.message).toContain("reread");
    }
  });

  it("conflicts when the file disappears before apply", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "obsolete.md": "line one\n" });
    const tool = createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "obsolete.md"));
    const prepared = await tool.prepare({ path: "obsolete.md", expectedSha256: hash }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const { rm } = await import("node:fs/promises");
    await rm(path.join(workspace.root, "obsolete.md"));
    const result = await tool.apply(prepared.mutation, {});
    expect(result.status).toBe("conflict");
  });

  it("rejects directories", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "dir/inner.txt": "x" });
    const tool = createTool(workspace.root);
    const prepared = await tool.prepare({ path: "dir", expectedSha256: "a".repeat(64) }, {});
    expect(prepared).toMatchObject({ status: "denied" });
  });

  it("rejects a symbolic-link target", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "real.txt": "x\n" });
    await createSymlink(
      path.join(workspace.root, "real.txt"),
      path.join(workspace.root, "link.txt"),
    );
    const tool = createTool(workspace.root);
    const prepared = await tool.prepare({ path: "link.txt", expectedSha256: "a".repeat(64) }, {});
    expect(prepared).toMatchObject({ status: "denied" });
  });

  it("rejects protected paths", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { ".env": "KEY=value\n" });
    const tool = createTool(workspace.root);
    const prepared = await tool.prepare({ path: ".env", expectedSha256: "a".repeat(64) }, {});
    expect(prepared).toMatchObject({ status: "denied" });
  });

  it("rejects binary files", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "bin.dat": Buffer.from([0x00, 0x01]) });
    const tool = createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "bin.dat"));
    const prepared = await tool.prepare({ path: "bin.dat", expectedSha256: hash }, {});
    expect(prepared).toMatchObject({ status: "failed" });
    if (prepared.status === "failed") {
      expect(prepared.message).toContain("binary");
    }
  });

  it("rejects oversized files", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, {
      "big.txt": Buffer.alloc(WORKSPACE_LIMITS.maxTextFileSizeBytes + 1, 0x61),
    });
    const tool = createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "big.txt"));
    const prepared = await tool.prepare({ path: "big.txt", expectedSha256: hash }, {});
    expect(prepared).toMatchObject({ status: "failed" });
  });

  it("cannot be applied twice", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "obsolete.md": "x\n" });
    const tool = createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "obsolete.md"));
    const prepared = await tool.prepare({ path: "obsolete.md", expectedSha256: hash }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    expect((await tool.apply(prepared.mutation, {})).status).toBe("success");
    expect((await tool.apply(prepared.mutation, {})).status).toBe("failed");
  });
});

describe("workspace.delete_file through the application", () => {
  function createReviewer(decision: "approve" | "deny"): ApprovalReviewer {
    return {
      review(): Promise<{ type: "approve_once" } | { type: "deny" }> {
        return Promise.resolve(
          decision === "approve" ? { type: "approve_once" } : { type: "deny" },
        );
      },
    };
  }

  it("leaves the file unchanged when approval is denied", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "obsolete.md": "keep me\n" });
    const hash = await hashOf(path.join(workspace.root, "obsolete.md"));
    const application = createSolarisApplication({
      provider: {
        id: "delete-provider",
        async *stream(): AsyncIterable<{
          type: "tool_call";
          callId: string;
          toolName: string;
          input: unknown;
        }> {
          yield {
            type: "tool_call",
            callId: "c1",
            toolName: "workspace.delete_file",
            input: { path: "obsolete.md", expectedSha256: hash },
          };
          await Promise.resolve();
        },
      },
      tools: createToolRegistry([createTool(workspace.root)]),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer: createReviewer("deny"),
    });
    for await (const _event of application.sendPrompt("delete the file")) {
      // drain
    }
    const bytes = await readFile(path.join(workspace.root, "obsolete.md"));
    expect(bytes.toString("utf8")).toBe("keep me\n");
  });

  it("deletes the file after approval", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "obsolete.md": "remove me\n" });
    const hash = await hashOf(path.join(workspace.root, "obsolete.md"));
    const application = createSolarisApplication({
      provider: {
        id: "delete-provider",
        async *stream(): AsyncIterable<{
          type: "tool_call";
          callId: string;
          toolName: string;
          input: unknown;
        }> {
          yield {
            type: "tool_call",
            callId: "c1",
            toolName: "workspace.delete_file",
            input: { path: "obsolete.md", expectedSha256: hash },
          };
          await Promise.resolve();
        },
      },
      tools: createToolRegistry([createTool(workspace.root)]),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer: createReviewer("approve"),
    });
    for await (const _event of application.sendPrompt("delete the file")) {
      // drain
    }
    await expect(readFile(path.join(workspace.root, "obsolete.md"))).rejects.toThrow();
  });
});
