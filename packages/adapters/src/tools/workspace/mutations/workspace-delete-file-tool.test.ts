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
  type PreparedCheckpoint,
} from "@solaris/core";
import { createWorkspaceDeleteFileTool } from "./workspace-delete-file-tool.js";
import { createMutationLock } from "./mutation-lock.js";
import { WORKSPACE_LIMITS } from "../limits.js";
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
  dependencies?: Parameters<typeof createWorkspaceDeleteFileTool>[3],
) {
  const store = await createTempCheckpointStore(workspaceRoot);
  return {
    tool: createWorkspaceDeleteFileTool(workspaceRoot, createMutationLock(), store, dependencies),
    store,
  };
}

async function hashOf(absolutePath: string): Promise<string> {
  const bytes = await readFile(absolutePath);
  return createHash("sha256").update(bytes).digest("hex");
}

afterEach(async () => {
  await cleanupTempCheckpointDirs();
  for (const workspace of workspaces.splice(0)) {
    await workspace.cleanup();
  }
});

describe("workspace.delete_file", () => {
  it("prepares a complete deletion preview", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "obsolete.md": "line one\nline two\n" });
    const { tool } = await createTool(workspace.root);
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
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "obsolete.md"));
    const prepared = await tool.prepare({ path: "obsolete.md", expectedSha256: hash }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    const output = expectSuccess(result);
    expect(output["operation"]).toBe("delete");
    expect(output["removedLines"]).toBe(1);
    await expect(readFile(path.join(workspace.root, "obsolete.md"))).rejects.toThrow();
  });

  it("conflicts on a stale hash", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "obsolete.md": "line one\n" });
    const { tool } = await createTool(workspace.root);
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
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "obsolete.md"));
    const prepared = await tool.prepare({ path: "obsolete.md", expectedSha256: hash }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const { rm } = await import("node:fs/promises");
    await rm(path.join(workspace.root, "obsolete.md"));
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("conflict");
  });

  it("conflicts when the target changes after the checkpoint is created", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "obsolete.md": "line one\n" });
    const hash = await hashOf(path.join(workspace.root, "obsolete.md"));
    const store = await createTempCheckpointStore(workspace.root);
    const realPrepare = store.prepare.bind(store);
    const racingStore = new Proxy(store, {
      get(target, property: keyof typeof store) {
        if (property === "prepare") {
          return async (checkpoint: PreparedCheckpoint, signal?: AbortSignal) => {
            const result = await realPrepare(checkpoint, signal);
            const { writeFile } = await import("node:fs/promises");
            await writeFile(
              path.join(workspace.root, "obsolete.md"),
              "user edit after checkpoint\n",
            );
            return result;
          };
        }
        // eslint-disable-next-line @typescript-eslint/unbound-method -- store methods are closures without this
        return target[property] as never;
      },
    });
    const tool = createWorkspaceDeleteFileTool(workspace.root, createMutationLock(), racingStore);
    const prepared = await tool.prepare({ path: "obsolete.md", expectedSha256: hash }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("conflict");
    const content = await readFile(path.join(workspace.root, "obsolete.md"), "utf8");
    expect(content).toBe("user edit after checkpoint\n");
  });

  it("cancels before the destructive commit point without deleting", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "obsolete.md": "line one\n" });
    const hash = await hashOf(path.join(workspace.root, "obsolete.md"));
    const controller = new AbortController();
    const store = await createTempCheckpointStore(workspace.root);
    const realPrepare = store.prepare.bind(store);
    const abortingStore = new Proxy(store, {
      get(target, property: keyof typeof store) {
        if (property === "prepare") {
          return async (checkpoint: PreparedCheckpoint, signal?: AbortSignal) => {
            const result = await realPrepare(checkpoint, signal);
            controller.abort();
            return result;
          };
        }
        // eslint-disable-next-line @typescript-eslint/unbound-method -- store methods are closures without this
        return target[property] as never;
      },
    });
    const tool = createWorkspaceDeleteFileTool(workspace.root, createMutationLock(), abortingStore);
    const prepared = await tool.prepare({ path: "obsolete.md", expectedSha256: hash }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, {
      approvedDigest: prepared.digest,
      signal: controller.signal,
    });
    expect(result.status).toBe("cancelled");
    const content = await readFile(path.join(workspace.root, "obsolete.md"), "utf8");
    expect(content).toBe("line one\n");
  });

  it("rejects directories", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "dir/inner.txt": "x" });
    const { tool } = await createTool(workspace.root);
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
    const { tool } = await createTool(workspace.root);
    const prepared = await tool.prepare({ path: "link.txt", expectedSha256: "a".repeat(64) }, {});
    expect(prepared).toMatchObject({ status: "denied" });
  });

  it("rejects protected paths", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { ".env": "KEY=value\n" });
    const { tool } = await createTool(workspace.root);
    const prepared = await tool.prepare({ path: ".env", expectedSha256: "a".repeat(64) }, {});
    expect(prepared).toMatchObject({ status: "denied" });
  });

  it("rejects binary files", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "bin.dat": Buffer.from([0x00, 0x01]) });
    const { tool } = await createTool(workspace.root);
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
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "big.txt"));
    const prepared = await tool.prepare({ path: "big.txt", expectedSha256: hash }, {});
    expect(prepared).toMatchObject({ status: "failed" });
  });

  it("cannot be applied twice", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "obsolete.md": "x\n" });
    const { tool } = await createTool(workspace.root);
    const hash = await hashOf(path.join(workspace.root, "obsolete.md"));
    const prepared = await tool.prepare({ path: "obsolete.md", expectedSha256: hash }, {});
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

  it(
    "deletes a file through a case-variant spelling on case-insensitive platforms",
    { skip: process.platform === "linux" },
    async () => {
      const workspace = await withWorkspace();
      await writeFixtureFiles(workspace.root, { "docs/obsolete.md": "line one\n" });
      const { tool } = await createTool(workspace.root);
      const hash = await hashOf(path.join(workspace.root, "docs", "obsolete.md"));
      const prepared = await tool.prepare({ path: "DOCS/Obsolete.MD", expectedSha256: hash }, {});
      expect(prepared.status).toBe("ready");
      if (prepared.status !== "ready") {
        return;
      }
      const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
      expect(result.status).toBe("success");
      await expect(readFile(path.join(workspace.root, "docs", "obsolete.md"))).rejects.toThrow();
    },
  );
});

describe("workspace.delete_file adversarial commit", () => {
  function realOps() {
    return {
      async rename(from: string, to: string) {
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
    };
  }

  it("preserves a target replaced immediately before the deletion displacement", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "obsolete.md": "approved content\n" });
    const hash = await hashOf(path.join(workspace.root, "obsolete.md"));
    let displaced = false;
    const { tool } = await createTool(workspace.root, {
      replacementOps: {
        ...realOps(),
        async rename(from: string, to: string) {
          if (!displaced && to.includes(".solaris-quarantine-")) {
            const fs = await import("node:fs/promises");
            await fs.writeFile(from, "user content replaced after approval\n");
            displaced = true;
          }
          await realOps().rename(from, to);
        },
      },
    });
    const prepared = await tool.prepare({ path: "obsolete.md", expectedSha256: hash }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("failed");
    const content = await readFile(path.join(workspace.root, "obsolete.md"), "utf8");
    expect(content).toBe("user content replaced after approval\n");
    const entries = await (await import("node:fs/promises")).readdir(workspace.root);
    expect(entries.some((entry) => entry.startsWith(".solaris-quarantine-"))).toBe(false);
  });

  it("reports an uncertain finalize failure after the file is deleted", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "obsolete.md": "line one\n" });
    const hash = await hashOf(path.join(workspace.root, "obsolete.md"));
    const store = await createTempCheckpointStore(workspace.root);
    const guardedStore = new Proxy(store, {
      get(target, property: keyof typeof store) {
        if (property === "finalizeApplied") {
          return () => Promise.reject(new Error("metadata write failed"));
        }
        // eslint-disable-next-line @typescript-eslint/unbound-method -- store methods are closures without this
        return target[property] as never;
      },
    });
    const tool = createWorkspaceDeleteFileTool(workspace.root, createMutationLock(), guardedStore);
    const prepared = await tool.prepare({ path: "obsolete.md", expectedSha256: hash }, {});
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
    await expect(readFile(path.join(workspace.root, "obsolete.md"))).rejects.toThrow();
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
        async *stream(): AsyncIterable<
          | { type: "tool_call"; callId: string; toolName: string; input: unknown }
          | { type: "completed" }
        > {
          yield {
            type: "tool_call",
            callId: "c1",
            toolName: "workspace.delete_file",
            input: { path: "obsolete.md", expectedSha256: hash },
          };
          await Promise.resolve();
          yield { type: "completed" };
        },
      },
      tools: createToolRegistry([(await createTool(workspace.root)).tool]),
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
        async *stream(): AsyncIterable<
          | { type: "tool_call"; callId: string; toolName: string; input: unknown }
          | { type: "completed" }
        > {
          yield {
            type: "tool_call",
            callId: "c1",
            toolName: "workspace.delete_file",
            input: { path: "obsolete.md", expectedSha256: hash },
          };
          await Promise.resolve();
          yield { type: "completed" };
        },
      },
      tools: createToolRegistry([(await createTool(workspace.root)).tool]),
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
