import { afterEach, describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import { mkdir, readFile } from "node:fs/promises";
import path from "node:path";
import {
  createDefaultPolicy,
  createSolarisApplication,
  createToolRegistry,
  DEVELOP_OFFLINE_PROFILE,
  type ApprovalReviewer,
  type CheckpointStore,
  type PreparedCheckpoint,
} from "@solaris/core";
import { createWorkspaceCreateFileTool } from "./workspace-create-file-tool.js";
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
  storeOptions: { maxCheckpoints?: number; maxStorageBytes?: number } = {},
): Promise<{ tool: ReturnType<typeof createWorkspaceCreateFileTool>; store: CheckpointStore }> {
  const store = await createTempCheckpointStore(workspaceRoot, storeOptions);
  return {
    tool: createWorkspaceCreateFileTool(workspaceRoot, createMutationLock(), store),
    store,
  };
}

const CONTENT = "Created by the deterministic Solaris test provider.\n";

afterEach(async () => {
  await cleanupTempCheckpointDirs();
  for (const workspace of workspaces.splice(0)) {
    await workspace.cleanup();
  }
});

describe("workspace.create_file", () => {
  it("prepares a complete creation preview", async () => {
    const workspace = await withWorkspace();
    const { tool } = await createTool(workspace.root);
    const prepared = await tool.prepare({ path: "new.txt", content: CONTENT }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    expect(prepared.preview.files[0]).toMatchObject({
      path: "new.txt",
      operation: "create",
      beforeSha256: null,
      addedLines: 1,
      removedLines: 0,
    });
    expect(prepared.preview.files[0]?.unifiedDiff).toContain(`+${CONTENT.trim()}`);
    expect(prepared.preview.truncated).toBe(false);
  });

  it("creates the file after apply with verified bytes and hash", async () => {
    const workspace = await withWorkspace();
    const { tool } = await createTool(workspace.root);
    const prepared = await tool.prepare({ path: "new.txt", content: CONTENT }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result).toMatchObject({ status: "success" });
    if (result.status !== "success") {
      return;
    }
    const output = expectSuccess(result);
    const bytes = await readFile(path.join(workspace.root, "new.txt"));
    expect(bytes.toString("utf8")).toBe(CONTENT);
    expect(createHash("sha256").update(bytes).digest("hex")).toBe(output["sha256"]);
  });

  it("cannot be applied twice", async () => {
    const workspace = await withWorkspace();
    const { tool } = await createTool(workspace.root);
    const prepared = await tool.prepare({ path: "new.txt", content: CONTENT }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const first = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    const second = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(first.status).toBe("success");
    expect(second.status).toBe("failed");
  });

  it("rejects a prepared mutation from another tool", async () => {
    const workspace = await withWorkspace();
    const { tool: toolA } = await createTool(workspace.root);
    const { tool: toolB } = await createTool(workspace.root);
    const prepared = await toolA.prepare({ path: "new.txt", content: CONTENT }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await toolB.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("failed");
  });

  it("returns conflict when the target already exists", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "new.txt": "existing\n" });
    const { tool } = await createTool(workspace.root);
    const prepared = await tool.prepare({ path: "new.txt", content: CONTENT }, {});
    expect(prepared).toMatchObject({ status: "conflict" });
  });

  it("returns conflict when the target appears before apply", async () => {
    const workspace = await withWorkspace();
    const { tool } = await createTool(workspace.root);
    const prepared = await tool.prepare({ path: "new.txt", content: CONTENT }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    await writeFixtureFiles(workspace.root, { "new.txt": "raced\n" });
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("conflict");
    const bytes = await readFile(path.join(workspace.root, "new.txt"));
    expect(bytes.toString("utf8")).toBe("raced\n");
  });

  it(
    "detects a parent directory swapped to a symlink after preparation",
    {
      skip: !SYMLINKS_SUPPORTED,
    },
    async () => {
      const workspace = await withWorkspace();
      await writeFixtureFiles(workspace.root, { "docs/keep.txt": "keep\n" });
      const { tool } = await createTool(workspace.root);
      const prepared = await tool.prepare({ path: "docs/new.txt", content: CONTENT }, {});
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
      expect(await readFile(path.join(outside.root, "keep.txt"), "utf8")).toBe("keep\n");
    },
  );

  it("cancels before the commit point without creating the file", async () => {
    const workspace = await withWorkspace();
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
    const tool = createWorkspaceCreateFileTool(workspace.root, createMutationLock(), abortingStore);
    const prepared = await tool.prepare({ path: "new.txt", content: CONTENT }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, {
      approvedDigest: prepared.digest,
      signal: controller.signal,
    });
    expect(result.status).toBe("cancelled");
    await expect(readFile(path.join(workspace.root, "new.txt"))).rejects.toThrow();
  });

  it("rejects a missing parent directory", async () => {
    const workspace = await withWorkspace();
    const { tool } = await createTool(workspace.root);
    const prepared = await tool.prepare({ path: "missing/new.txt", content: CONTENT }, {});
    expect(prepared).toMatchObject({ status: "denied" });
  });

  it("rejects protected targets", async () => {
    const workspace = await withWorkspace();
    const { tool } = await createTool(workspace.root);
    for (const target of [".env", ".git/config", ".solaris/state.json", "cert.pem", "id.key"]) {
      const prepared = await tool.prepare({ path: target, content: CONTENT }, {});
      expect(prepared, target).toMatchObject({ status: "denied" });
    }
  });

  it("rejects a symlinked parent", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const workspace = await withWorkspace();
    const outside = await createTempWorkspace();
    workspaces.push(outside);
    await createSymlink(outside.root, path.join(workspace.root, "link-dir"));
    const { tool } = await createTool(workspace.root);
    const prepared = await tool.prepare({ path: "link-dir/new.txt", content: CONTENT }, {});
    expect(prepared).toMatchObject({ status: "denied" });
  });

  it("rejects oversized content", async () => {
    const workspace = await withWorkspace();
    const { tool } = await createTool(workspace.root);
    const huge = "x".repeat(WORKSPACE_LIMITS.maxCreatedContentBytes + 1);
    const prepared = await tool.prepare({ path: "huge.txt", content: huge }, {});
    expect(prepared).toMatchObject({ status: "invalid_input" });
  });

  it("rejects binary content with null bytes", async () => {
    const workspace = await withWorkspace();
    const { tool } = await createTool(workspace.root);
    const prepared = await tool.prepare({ path: "bin.txt", content: "a\0b" }, {});
    expect(prepared).toMatchObject({ status: "invalid_input" });
  });

  it("rejects empty content", async () => {
    const workspace = await withWorkspace();
    const { tool } = await createTool(workspace.root);
    const prepared = await tool.prepare({ path: "empty.txt", content: "" }, {});
    expect(prepared).toMatchObject({ status: "invalid_input" });
  });

  it("creates files in nested existing directories", async () => {
    const workspace = await withWorkspace();
    await mkdir(path.join(workspace.root, "docs", "deep"), { recursive: true });
    const { tool } = await createTool(workspace.root);
    const prepared = await tool.prepare({ path: "docs/deep/note.md", content: "# Note\n" }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("success");
  });

  it("records an applied checkpoint before the mutation and returns its id", async () => {
    const workspace = await withWorkspace();
    const { tool, store } = await createTool(workspace.root);
    const prepared = await tool.prepare({ path: "new.txt", content: CONTENT }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    const output = expectSuccess(result);
    expect(typeof output["checkpointId"]).toBe("string");
    const checkpoints = await store.list();
    expect(checkpoints).toHaveLength(1);
    expect(checkpoints[0]).toMatchObject({
      operation: "create",
      state: "applied",
      relativePath: "new.txt",
    });
    expect(checkpoints[0]?.before.exists).toBe(false);
  });

  it("does not mutate when checkpoint recording fails", async () => {
    const workspace = await withWorkspace();
    const { tool } = await createTool(workspace.root, { maxCheckpoints: 0 });
    const prepared = await tool.prepare({ path: "new.txt", content: CONTENT }, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const result = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
    expect(result.status).toBe("failed");
    if (result.status === "failed") {
      expect(result.message).toContain("Checkpoint could not be recorded");
    }
    await expect(readFile(path.join(workspace.root, "new.txt"))).rejects.toThrow();
  });
});

describe("workspace.create_file through the application", () => {
  function createReviewer(decision: "approve" | "deny"): ApprovalReviewer {
    return {
      review(): Promise<{ type: "approve_once" } | { type: "deny" }> {
        return Promise.resolve(
          decision === "approve" ? { type: "approve_once" } : { type: "deny" },
        );
      },
    };
  }

  it("does not create the file before approval", async () => {
    const workspace = await withWorkspace();
    const application = createSolarisApplication({
      provider: {
        id: "create-provider",
        async *stream(): AsyncIterable<
          | { type: "tool_call"; callId: string; toolName: string; input: unknown }
          | { type: "completed" }
        > {
          yield {
            type: "tool_call",
            callId: "c1",
            toolName: "workspace.create_file",
            input: { path: "new.txt", content: CONTENT },
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
    for await (const _event of application.sendPrompt("create a file")) {
      // drain
    }
    await expect(readFile(path.join(workspace.root, "new.txt"))).rejects.toThrow();
  });

  it("creates the file after approval", async () => {
    const workspace = await withWorkspace();
    const application = createSolarisApplication({
      provider: {
        id: "create-provider",
        async *stream(): AsyncIterable<
          | { type: "tool_call"; callId: string; toolName: string; input: unknown }
          | { type: "completed" }
        > {
          yield {
            type: "tool_call",
            callId: "c1",
            toolName: "workspace.create_file",
            input: { path: "new.txt", content: CONTENT },
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
    for await (const _event of application.sendPrompt("create a file")) {
      // drain
    }
    const bytes = await readFile(path.join(workspace.root, "new.txt"));
    expect(bytes.toString("utf8")).toBe(CONTENT);
  });

  it("requires approval for writes", async () => {
    const workspace = await withWorkspace();
    let reviewed = false;
    const application = createSolarisApplication({
      provider: {
        id: "create-provider",
        async *stream(): AsyncIterable<
          | { type: "tool_call"; callId: string; toolName: string; input: unknown }
          | { type: "completed" }
        > {
          yield {
            type: "tool_call",
            callId: "c1",
            toolName: "workspace.create_file",
            input: { path: "new.txt", content: CONTENT },
          };
          await Promise.resolve();
          yield { type: "completed" };
        },
      },
      tools: createToolRegistry([(await createTool(workspace.root)).tool]),
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      reviewer: {
        review(): Promise<{ type: "approve_once" }> {
          reviewed = true;
          return Promise.resolve({ type: "approve_once" });
        },
      },
    });
    for await (const _event of application.sendPrompt("create a file")) {
      // drain
    }
    expect(reviewed).toBe(true);
  });
});
