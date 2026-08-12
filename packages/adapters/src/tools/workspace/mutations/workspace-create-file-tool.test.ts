import { afterEach, describe, expect, it } from "vitest";
import { mkdtemp, readdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createWorkspaceCreateFileTool } from "./workspace-create-file-tool.js";
import { createMutationLock } from "./mutation-lock.js";
import {
  cleanupTempCheckpointDirs,
  createTempCheckpointStore,
  createTempWorkspace,
} from "../workspace-fixtures.js";

const workspaces: Awaited<ReturnType<typeof createTempWorkspace>>[] = [];

async function withWorkspace() {
  const workspace = await createTempWorkspace();
  workspaces.push(workspace);
  return workspace;
}

afterEach(async () => {
  await cleanupTempCheckpointDirs();
  for (const workspace of workspaces.splice(0)) {
    await workspace.cleanup();
  }
});

describe("workspace.create_file fail-closed availability", () => {
  it("is unavailable before any write, approval, or checkpoint", async () => {
    const workspace = await withWorkspace();
    const store = await createTempCheckpointStore(workspace.root);
    const tool = createWorkspaceCreateFileTool(workspace.root, createMutationLock(), store);
    const prepared = await tool.prepare({ path: "new.txt", content: "hello" }, {});
    expect(prepared.status).toBe("unavailable");
    if (prepared.status === "unavailable") {
      expect(prepared.message).toContain("fails closed before any write");
    }
    // Nothing was created in the workspace and no checkpoint was recorded.
    const entries = await readdir(workspace.root);
    expect(entries).toEqual([]);
    expect(await store.list()).toEqual([]);
  });

  it("is unavailable for apply even if a prepared mutation were obtained", async () => {
    const workspace = await withWorkspace();
    const store = await createTempCheckpointStore(workspace.root);
    const tool = createWorkspaceCreateFileTool(workspace.root, createMutationLock(), store);
    const result = await tool.apply({} as never, {});
    expect(result.status).toBe("unavailable");
  });

  it("honours cancellation before the unavailable decision", async () => {
    const workspace = await withWorkspace();
    const store = await createTempCheckpointStore(workspace.root);
    const tool = createWorkspaceCreateFileTool(workspace.root, createMutationLock(), store);
    const controller = new AbortController();
    controller.abort();
    const prepared = await tool.prepare(
      { path: "new.txt", content: "hello" },
      {
        signal: controller.signal,
      },
    );
    expect(prepared.status).toBe("cancelled");
    expect(await readdir(workspace.root)).toEqual([]);
  });

  it("exposes an honest definition and capability", async () => {
    const workspaceRoot = await mkdtemp(join(tmpdir(), "siralos-create-tool-"));
    const tool = createWorkspaceCreateFileTool(workspaceRoot, createMutationLock(), {
      prepare(): Promise<never> {
        return Promise.reject(new Error("checkpoint store must not be reached"));
      },
    } as never);
    expect(tool.definition.name).toBe("workspace.create_file");
    expect(tool.definition.description).toContain("Unavailable");
    expect(tool.capability).toBe("workspace.write");
  });
});
