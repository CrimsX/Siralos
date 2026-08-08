import { afterEach, describe, expect, it } from "vitest";
import { mkdtemp, readdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createWorkspaceEditFileTool } from "./workspace-edit-file-tool.js";
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

describe("workspace.edit_file fail-closed availability", () => {
  it("is unavailable before any write, approval, or checkpoint", async () => {
    const workspace = await withWorkspace();
    const store = await createTempCheckpointStore(workspace.root);
    const tool = createWorkspaceEditFileTool(workspace.root, createMutationLock(), store);
    const prepared = await tool.prepare(
      {
        path: "a.txt",
        expectedSha256: "f".repeat(64),
        replacements: [{ oldText: "old", newText: "new" }],
      },
      {},
    );
    expect(prepared.status).toBe("unavailable");
    if (prepared.status === "unavailable") {
      expect(prepared.message).toContain("fails closed before any write");
    }
    expect(await readdir(workspace.root)).toEqual([]);
    expect(await store.list()).toEqual([]);
  });

  it("is unavailable for apply even if a prepared mutation were obtained", async () => {
    const workspace = await withWorkspace();
    const store = await createTempCheckpointStore(workspace.root);
    const tool = createWorkspaceEditFileTool(workspace.root, createMutationLock(), store);
    const result = await tool.apply({} as never, {});
    expect(result.status).toBe("unavailable");
  });

  it("honours cancellation before the unavailable decision", async () => {
    const workspace = await withWorkspace();
    const store = await createTempCheckpointStore(workspace.root);
    const tool = createWorkspaceEditFileTool(workspace.root, createMutationLock(), store);
    const controller = new AbortController();
    controller.abort();
    const prepared = await tool.prepare(
      {
        path: "a.txt",
        expectedSha256: "f".repeat(64),
        replacements: [{ oldText: "old", newText: "new" }],
      },
      { signal: controller.signal },
    );
    expect(prepared.status).toBe("cancelled");
    expect(await readdir(workspace.root)).toEqual([]);
  });

  it("exposes an honest definition and capability", async () => {
    const workspaceRoot = await mkdtemp(join(tmpdir(), "solaris-edit-tool-"));
    const tool = createWorkspaceEditFileTool(workspaceRoot, createMutationLock(), {} as never);
    expect(tool.definition.name).toBe("workspace.edit_file");
    expect(tool.definition.description).toContain("Unavailable");
    expect(tool.capability).toBe("workspace.write");
  });
});
