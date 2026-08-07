import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { PreparedCheckpoint } from "@solaris/core";
import { createFilesystemCheckpointStore } from "./checkpoint-store.js";
import { reconcileWorkspaceCheckpoints } from "./reconciliation.js";
import { cleanupTempDirs, registerTempDir } from "../../git/cli/git-test-support.js";

afterEach(async () => {
  await cleanupTempDirs();
});

interface StoreContext {
  store: Awaited<ReturnType<typeof createFilesystemCheckpointStore>>;
  workspaceRoot: string;
  rootDirectory: string;
  fingerprint: string;
}

async function withStore(overrides: Record<string, unknown> = {}): Promise<StoreContext> {
  const workspaceRoot = await mkdtemp(join(tmpdir(), "solaris-cp-workspace-"));
  registerTempDir(workspaceRoot);
  const rootDirectory = await mkdtemp(join(tmpdir(), "solaris-cp-store-"));
  registerTempDir(rootDirectory);
  const store = await createFilesystemCheckpointStore({
    workspaceRoot,
    rootDirectory,
    ...overrides,
  });
  return {
    store,
    workspaceRoot,
    rootDirectory,
    fingerprint: createHash("sha256").update(workspaceRoot).digest("hex"),
  };
}

function hashOf(content: string): string {
  return createHash("sha256").update(content).digest("hex");
}

function preparedUpdate(overrides: Partial<PreparedCheckpoint> = {}): PreparedCheckpoint {
  const beforeContent = "before content\n";
  return {
    relativePath: "docs/note.md",
    operation: "update",
    toolName: "workspace.edit_file",
    before: {
      exists: true,
      sha256: hashOf(beforeContent),
      byteLength: Buffer.byteLength(beforeContent),
      bytes: Buffer.from(beforeContent),
    },
    after: { exists: true, sha256: hashOf("after content\n"), byteLength: 14 },
    preview: { addedLines: 1, removedLines: 1 },
    ...overrides,
  };
}

function checkpointDirOf(context: StoreContext, checkpointId: string): string {
  return join(context.rootDirectory, context.fingerprint, checkpointId);
}

describe("filesystem checkpoint store", () => {
  it("stores exact preimage bytes for an update", async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare(preparedUpdate());
    expect(checkpoint.state).toBe("prepared");
    expect(checkpoint.id).toMatch(/^cp_/);
    const preimage = await context.store.loadPreimage(checkpoint.id);
    expect(Buffer.from(preimage ?? []).toString("utf8")).toBe("before content\n");
    const dir = checkpointDirOf(context, checkpoint.id);
    expect((await readdir(dir)).sort()).toContain("preimage.bin");
  });

  it("stores no preimage for a create", async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare({
      ...preparedUpdate(),
      operation: "create",
      before: { exists: false, sha256: null, byteLength: null, bytes: null },
      after: { exists: true, sha256: hashOf("new\n"), byteLength: 4 },
    });
    expect(await context.store.loadPreimage(checkpoint.id)).toBeNull();
  });

  it("finalizes applied checkpoints only from prepared", async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare(preparedUpdate());
    const applied = await context.store.finalizeApplied(checkpoint.id, {
      afterSha256: checkpoint.after.sha256,
      absent: false,
    });
    expect(applied.state).toBe("applied");
    await expect(
      context.store.finalizeApplied(checkpoint.id, {
        afterSha256: checkpoint.after.sha256,
        absent: false,
      }),
    ).rejects.toThrow();
  });

  it("rejects finalizing with a mismatched after-state", async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare(preparedUpdate());
    await expect(
      context.store.finalizeApplied(checkpoint.id, {
        afterSha256: "f".repeat(64),
        absent: false,
      }),
    ).rejects.toThrow();
    expect((await context.store.get(checkpoint.id))?.state).toBe("prepared");
  });

  it("marks undone from applied and refuses double undo", async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare(preparedUpdate());
    await context.store.finalizeApplied(checkpoint.id, {
      afterSha256: checkpoint.after.sha256,
      absent: false,
    });
    const undone = await context.store.markUndone(checkpoint.id);
    expect(undone.state).toBe("undone");
    await expect(context.store.markUndone(checkpoint.id)).rejects.toThrow();
  });

  it("validates lifecycle transitions through markState", async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare(preparedUpdate());
    const abandoned = await context.store.markState(checkpoint.id, "abandoned");
    expect(abandoned.state).toBe("abandoned");
    await expect(context.store.markState(checkpoint.id, "applied")).rejects.toThrow();
  });

  it("generates unique ids", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    const second = await context.store.prepare(preparedUpdate());
    expect(first.id).not.toBe(second.id);
  });

  it("lists without loading preimages and filters states", async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare(preparedUpdate());
    const all = await context.store.list();
    expect(all.map((entry) => entry.id)).toContain(checkpoint.id);
    const applied = await context.store.list({ states: ["applied"] });
    expect(applied).toEqual([]);
  });

  it("rejects preimage hash corruption", async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare(preparedUpdate());
    const dir = checkpointDirOf(context, checkpoint.id);
    await writeFile(join(dir, "preimage.bin"), "corrupted bytes");
    await expect(context.store.loadPreimage(checkpoint.id)).rejects.toThrow();
  });

  it("rejects a checkpoint root inside the workspace", async () => {
    const workspaceRoot = await mkdtemp(join(tmpdir(), "solaris-cp-workspace-"));
    registerTempDir(workspaceRoot);
    const rootDirectory = join(workspaceRoot, ".solaris", "checkpoints");
    await mkdir(rootDirectory, { recursive: true });
    await expect(
      createFilesystemCheckpointStore({ workspaceRoot, rootDirectory }),
    ).rejects.toThrow();
  });

  it("rejects invalid metadata files safely", async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare(preparedUpdate());
    const dir = checkpointDirOf(context, checkpoint.id);
    await writeFile(join(dir, "metadata.json"), "{ not json");
    expect(await context.store.get(checkpoint.id)).toBeNull();
    await expect(context.store.loadPreimage(checkpoint.id)).rejects.toThrow();
  });

  it("rejects a symlinked checkpoint directory", async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare(preparedUpdate());
    const dir = checkpointDirOf(context, checkpoint.id);
    const { rm, symlink } = await import("node:fs/promises");
    await rm(dir, { recursive: true, force: true });
    const outside = await mkdtemp(join(tmpdir(), "solaris-cp-outside-"));
    registerTempDir(outside);
    await symlink(outside, dir).catch(() => {});
    await expect(context.store.loadPreimage(checkpoint.id)).rejects.toThrow();
  });

  it("prunes oldest terminal checkpoints first", async () => {
    const context = await withStore({ maxCheckpoints: 2 });
    const first = await context.store.prepare(preparedUpdate());
    await context.store.finalizeApplied(first.id, {
      afterSha256: first.after.sha256,
      absent: false,
    });
    await context.store.markUndone(first.id);
    const second = await context.store.prepare(preparedUpdate());
    await context.store.finalizeApplied(second.id, {
      afterSha256: second.after.sha256,
      absent: false,
    });
    const third = await context.store.prepare(preparedUpdate());
    expect(await context.store.get(first.id)).toBeNull();
    expect((await context.store.get(third.id))?.state).toBe("prepared");
  });

  it("retains prepared and uncertain checkpoints during pruning", async () => {
    const context = await withStore({ maxCheckpoints: 2 });
    const prepared = await context.store.prepare(preparedUpdate());
    const uncertain = await context.store.prepare(preparedUpdate());
    await context.store.markState(uncertain.id, "uncertain");
    await expect(context.store.prepare(preparedUpdate())).rejects.toThrow();
    expect((await context.store.get(prepared.id))?.state).toBe("prepared");
    expect((await context.store.get(uncertain.id))?.state).toBe("uncertain");
  });

  it("blocks preparation when storage cannot be freed", async () => {
    const context = await withStore({ maxStorageBytes: 10 });
    await expect(context.store.prepare(preparedUpdate())).rejects.toThrow();
  });
});

describe("checkpoint reconciliation", () => {
  it("marks prepared checkpoints by comparing current state", async () => {
    const context = await withStore();
    await mkdir(join(context.workspaceRoot, "docs"), { recursive: true });
    const beforeA = "state a\n";
    const afterB = "state b\n";
    const unrelated = "state c\n";
    const beforeHash = hashOf(beforeA);
    const afterHash = hashOf(afterB);
    await writeFile(join(context.workspaceRoot, "docs", "a.txt"), beforeA);
    const abandonedCheckpoint = await context.store.prepare({
      ...preparedUpdate(),
      relativePath: "docs/a.txt",
      before: {
        exists: true,
        sha256: beforeHash,
        byteLength: beforeA.length,
        bytes: Buffer.from(beforeA),
      },
      after: { exists: true, sha256: afterHash, byteLength: afterB.length },
    });
    await writeFile(join(context.workspaceRoot, "docs", "b.txt"), beforeA);
    const appliedCheckpoint = await context.store.prepare({
      ...preparedUpdate(),
      relativePath: "docs/b.txt",
      before: {
        exists: true,
        sha256: beforeHash,
        byteLength: beforeA.length,
        bytes: Buffer.from(beforeA),
      },
      after: { exists: true, sha256: afterHash, byteLength: afterB.length },
    });
    await writeFile(join(context.workspaceRoot, "docs", "b.txt"), afterB);
    await writeFile(join(context.workspaceRoot, "docs", "c.txt"), beforeA);
    const uncertainCheckpoint = await context.store.prepare({
      ...preparedUpdate(),
      relativePath: "docs/c.txt",
      before: {
        exists: true,
        sha256: beforeHash,
        byteLength: beforeA.length,
        bytes: Buffer.from(beforeA),
      },
      after: { exists: true, sha256: afterHash, byteLength: afterB.length },
    });
    await writeFile(join(context.workspaceRoot, "docs", "c.txt"), unrelated);

    const report = await reconcileWorkspaceCheckpoints({
      workspaceRoot: context.workspaceRoot,
      store: context.store,
    });
    expect(report.checked).toBe(3);
    expect(report.abandoned).toBe(1);
    expect(report.applied).toBe(1);
    expect(report.uncertain).toBe(1);
    expect((await context.store.get(abandonedCheckpoint.id))?.state).toBe("abandoned");
    expect((await context.store.get(appliedCheckpoint.id))?.state).toBe("applied");
    expect((await context.store.get(uncertainCheckpoint.id))?.state).toBe("uncertain");
  });

  it("reconciliation is bounded to prepared checkpoints only", async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare(preparedUpdate());
    await context.store.markState(checkpoint.id, "abandoned");
    const report = await reconcileWorkspaceCheckpoints({
      workspaceRoot: context.workspaceRoot,
      store: context.store,
    });
    expect(report.checked).toBe(0);
  });
});
