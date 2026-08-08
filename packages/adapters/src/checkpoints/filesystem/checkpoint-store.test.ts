import { createHash, randomUUID } from "node:crypto";
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { PreparedCheckpoint } from "@solaris/core";
import {
  CheckpointStorageLimitError,
  createFilesystemCheckpointStore,
} from "./checkpoint-store.js";
import { reconcileWorkspaceCheckpoints } from "./reconciliation.js";
import { cleanupTempDirs, registerTempDir } from "../../git/cli/git-test-support.js";
import { SYMLINKS_SUPPORTED } from "../../tools/workspace/workspace-fixtures.js";

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

/**
 * Byte-for-byte recursive snapshot of a directory tree: relative path ->
 * SHA-256 of content. Any deletion, rename, or content change anywhere in
 * the tree changes the snapshot, so equality before/after an operation
 * proves no destructive filesystem operation occurred.
 */
async function snapshotTree(root: string): Promise<ReadonlyMap<string, string>> {
  const { createHash } = await import("node:crypto");
  const { readdir, readFile } = await import("node:fs/promises");
  const snapshot = new Map<string, string>();
  const walk = async (directory: string, relative: string): Promise<void> => {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const entryRelative = relative.length === 0 ? entry.name : `${relative}/${entry.name}`;
      const absolute = join(directory, entry.name);
      if (entry.isDirectory()) {
        await walk(absolute, entryRelative);
      } else {
        const content = await readFile(absolute);
        snapshot.set(entryRelative, createHash("sha256").update(content).digest("hex"));
      }
    }
  };
  await walk(root, "");
  return snapshot;
}

describe("fail-closed capacity verification", () => {
  async function entryDirOf(context: StoreContext, id: string): Promise<string> {
    const dir = join(context.rootDirectory, context.fingerprint, id);
    await mkdir(dir, { recursive: true });
    return dir;
  }

  function metadataPathOf(context: StoreContext, id: string): string {
    return join(context.rootDirectory, context.fingerprint, id, "metadata.json");
  }

  async function readMetadata(context: StoreContext, id: string): Promise<Record<string, unknown>> {
    const raw = await readFile(metadataPathOf(context, id), "utf8");
    return JSON.parse(raw) as Record<string, unknown>;
  }

  async function rewriteMetadata(
    context: StoreContext,
    id: string,
    mutate: (record: Record<string, unknown>) => void,
  ): Promise<void> {
    const record = await readMetadata(context, id);
    mutate(record);
    await writeFile(metadataPathOf(context, id), JSON.stringify(record, null, 2), "utf8");
  }

  async function expectRefusalPreservesTree(
    context: StoreContext,
    prepare: () => Promise<unknown>,
  ): Promise<void> {
    const before = await snapshotTree(context.rootDirectory);
    await expect(prepare()).rejects.toBeInstanceOf(CheckpointStorageLimitError);
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
  }

  it("the known reproduction refuses: malformed metadata with a large preimage under a small byte limit", async () => {
    const context = await withStore({ maxStorageBytes: 20 });
    const id = `cp_${randomUUID()}`;
    await entryDirOf(context, id);
    await writeFile(metadataPathOf(context, id), "{ this is not json", "utf8");
    await writeFile(
      join(context.rootDirectory, context.fingerprint, id, "preimage.bin"),
      Buffer.alloc(1024 * 1024, 0x61),
    );
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("refuses when a checkpoint-like entry has missing metadata", async () => {
    const context = await withStore({ maxStorageBytes: 20 });
    const id = `cp_${randomUUID()}`;
    const dir = await entryDirOf(context, id);
    await writeFile(join(dir, "preimage.bin"), Buffer.alloc(1024 * 1024, 0x61));
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("refuses when metadata is oversized", async () => {
    const context = await withStore({ maxStorageBytes: 20 });
    const id = `cp_${randomUUID()}`;
    await entryDirOf(context, id);
    await writeFile(
      metadataPathOf(context, id),
      JSON.stringify({ padding: "x".repeat(70 * 1024) }),
      "utf8",
    );
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("refuses when metadata is unreadable", { skip: process.platform === "win32" }, async () => {
    const context = await withStore({ maxStorageBytes: 20 });
    const id = `cp_${randomUUID()}`;
    await entryDirOf(context, id);
    await writeFile(metadataPathOf(context, id), "{}", "utf8");
    await chmod(metadataPathOf(context, id), 0o000);
    try {
      await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
    } finally {
      await chmod(metadataPathOf(context, id), 0o600).catch(() => undefined);
    }
  });

  it("refuses when metadata is missing required fields", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    for (const missing of ["toolName", "createdAt", "preview", "before", "relativePath"] as const) {
      await rewriteMetadata(context, first.id, (record) => {
        delete record[missing];
      });
      await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
    }
  });

  it("refuses on negative, fractional, and string byte lengths", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    for (const byteLength of [-1, 1.5, "14"]) {
      await rewriteMetadata(context, first.id, (record) => {
        (record["before"] as Record<string, unknown>)["byteLength"] = byteLength;
      });
      await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
    }
  });

  it("refuses when the before byte length exceeds the configured preimage maximum", async () => {
    const content = "x".repeat(24);
    const wide = await withStore({ maxPreimageBytes: 1024 });
    await wide.store.prepare(
      preparedUpdate({
        before: {
          exists: true,
          sha256: hashOf(content),
          byteLength: 24,
          bytes: Buffer.from(content),
        },
      }),
    );
    const tight = await withStore({
      workspaceRoot: wide.workspaceRoot,
      rootDirectory: wide.rootDirectory,
      maxPreimageBytes: 16,
    });
    await expectRefusalPreservesTree(tight, () => tight.store.prepare(preparedUpdate()));
  });

  it("refuses when metadata declares zero bytes while a preimage exists", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    await rewriteMetadata(context, first.id, (record) => {
      (record["before"] as Record<string, unknown>)["byteLength"] = 0;
    });
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("refuses when the preimage size disagrees with metadata", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    await rewriteMetadata(context, first.id, (record) => {
      (record["before"] as Record<string, unknown>)["byteLength"] = 99;
    });
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("refuses when a preimage declared by metadata is missing", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    await rm(join(checkpointDirOf(context, first.id), "preimage.bin"), { force: true });
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("refuses when metadata declares no preimage but preimage.bin exists", async () => {
    const context = await withStore();
    const created = await context.store.prepare({
      ...preparedUpdate(),
      operation: "create",
      before: { exists: false, sha256: null, byteLength: null, bytes: null },
      after: { exists: true, sha256: hashOf("new\n"), byteLength: 4 },
    });
    await writeFile(join(checkpointDirOf(context, created.id), "preimage.bin"), "stray", "utf8");
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("refuses when a preimage is substituted with a directory", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    await rm(join(checkpointDirOf(context, first.id), "preimage.bin"), { force: true });
    await mkdir(join(checkpointDirOf(context, first.id), "preimage.bin"), { recursive: true });
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("refuses when metadata is a symbolic link", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    const metadataPath = metadataPathOf(context, first.id);
    const target = join(context.rootDirectory, "metadata-target.json");
    await writeFile(target, "{}", "utf8");
    await rm(metadataPath, { force: true });
    const { symlink } = await import("node:fs/promises");
    await symlink(target, metadataPath);
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("refuses when the preimage is a symbolic link", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    const preimagePath = join(checkpointDirOf(context, first.id), "preimage.bin");
    const target = join(context.rootDirectory, "preimage-target.bin");
    await writeFile(target, "before content\n", "utf8");
    await rm(preimagePath, { force: true });
    const { symlink } = await import("node:fs/promises");
    await symlink(target, preimagePath);
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("exact valid capacity boundary succeeds and one byte over refuses", async () => {
    // "before content\n" is 15 bytes, so two checkpoints use exactly 30.
    const context = await withStore({ maxStorageBytes: 30 });
    await context.store.prepare(preparedUpdate());
    // 15 + 15 == 30: exactly at the limit succeeds.
    const second = await context.store.prepare(preparedUpdate());
    expect(second.state).toBe("prepared");
    // 30 + 15 > 30: a third refuses.
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("one byte over capacity refuses before any write", async () => {
    const context = await withStore({ maxStorageBytes: 29 });
    await context.store.prepare(preparedUpdate());
    // 15 + 15 = 30 > 29: one byte over the limit refuses.
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });
});

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

  it("refuses new checkpoints at the count limit and deletes nothing", async () => {
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
    // Reaching the count limit is a typed storage-limit refusal, never a
    // pruning pass: nothing is deleted and every existing checkpoint
    // directory and file remains byte-for-byte unchanged.
    const before = await snapshotTree(context.rootDirectory);
    await expect(context.store.prepare(preparedUpdate())).rejects.toBeInstanceOf(
      CheckpointStorageLimitError,
    );
    expect((await context.store.get(first.id))?.state).toBe("undone");
    expect((await context.store.get(second.id))?.state).toBe("applied");
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
  });

  it("retains prepared and uncertain checkpoints under storage pressure and deletes nothing", async () => {
    const context = await withStore({ maxCheckpoints: 2 });
    const prepared = await context.store.prepare(preparedUpdate());
    const uncertain = await context.store.prepare(preparedUpdate());
    await context.store.markState(uncertain.id, "uncertain");
    const before = await snapshotTree(context.rootDirectory);
    await expect(context.store.prepare(preparedUpdate())).rejects.toBeInstanceOf(
      CheckpointStorageLimitError,
    );
    expect((await context.store.get(prepared.id))?.state).toBe("prepared");
    expect((await context.store.get(uncertain.id))?.state).toBe("uncertain");
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
  });

  it("blocks preparation when the byte limit is reached, deleting nothing", async () => {
    // The default preimage is 14 bytes: the first fits under 20, the
    // second would exceed it and is refused with zero deletion.
    const context = await withStore({ maxStorageBytes: 20 });
    await context.store.prepare(preparedUpdate());
    const before = await snapshotTree(context.rootDirectory);
    await expect(context.store.prepare(preparedUpdate())).rejects.toBeInstanceOf(
      CheckpointStorageLimitError,
    );
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
  });

  it("never deletes on storage pressure with large fanout and deep trees present", async () => {
    const context = await withStore({ maxCheckpoints: 1 });
    await context.store.prepare(preparedUpdate());
    // A checkpoint-like directory with wide fanout and a deep chain that a
    // removal pass could have descended into; it must never be touched.
    const wide = join(context.rootDirectory, context.fingerprint, "cp_wide-0001");
    await mkdir(join(wide, "deep", "chain"), { recursive: true });
    for (let index = 0; index < 50; index += 1) {
      await writeFile(join(wide, `file-${index}.txt`), "x");
    }
    await writeFile(join(wide, "deep", "chain", "leaf.txt"), "y");
    const before = await snapshotTree(context.rootDirectory);
    await expect(context.store.prepare(preparedUpdate())).rejects.toBeInstanceOf(
      CheckpointStorageLimitError,
    );
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
  });

  it("fails closed when the checkpoint directory enumeration is truncated (fanout beyond the cap)", async () => {
    const context = await withStore({ maxCheckpoints: 2 });
    // More than maxCheckpoints + 1 entries: the bounded enumeration cannot
    // prove capacity, so preparation fails closed without deleting anything.
    for (const id of ["cp_aaaa", "cp_bbbb", "cp_cccc", "cp_dddd"]) {
      await mkdir(join(context.rootDirectory, context.fingerprint, id), { recursive: true });
    }
    const before = await snapshotTree(context.rootDirectory);
    await expect(context.store.prepare(preparedUpdate())).rejects.toBeInstanceOf(
      CheckpointStorageLimitError,
    );
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
  });

  it("fails closed when the retention pass deadline is expired, deleting nothing", async () => {
    const context = await withStore({ maxCheckpoints: 2, retentionDeadlineMs: -1 });
    await context.store.prepare(preparedUpdate());
    const before = await snapshotTree(context.rootDirectory);
    await expect(context.store.prepare(preparedUpdate())).rejects.toBeInstanceOf(
      CheckpointStorageLimitError,
    );
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
  });

  it("never deletes anything when preparation is cancelled", async () => {
    const context = await withStore({ maxCheckpoints: 1 });
    await context.store.prepare(preparedUpdate());
    const controller = new AbortController();
    controller.abort();
    const before = await snapshotTree(context.rootDirectory);
    await expect(context.store.prepare(preparedUpdate(), controller.signal)).rejects.toMatchObject({
      name: "AbortError",
    });
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
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

  it("reconciliation is bounded to prepared and applied checkpoints only", async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare(preparedUpdate());
    await context.store.markState(checkpoint.id, "abandoned");
    const report = await reconcileWorkspaceCheckpoints({
      workspaceRoot: context.workspaceRoot,
      store: context.store,
    });
    expect(report.checked).toBe(0);
  });

  it("reconciles an applied checkpoint whose file matches the before-state as undone", async () => {
    const context = await withStore();
    await mkdir(join(context.workspaceRoot, "docs"), { recursive: true });
    const before = "before content\n";
    const after = "after content\n";
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), before);
    const checkpoint = await context.store.prepare(preparedUpdate());
    await context.store.finalizeApplied(checkpoint.id, {
      afterSha256: hashOf(after),
      absent: false,
    });
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), before);

    const report = await reconcileWorkspaceCheckpoints({
      workspaceRoot: context.workspaceRoot,
      store: context.store,
    });
    expect(report.undoneAfterRestore).toBe(1);
    expect((await context.store.get(checkpoint.id))?.state).toBe("undone");
  });

  it("leaves an applied checkpoint alone when the file still matches the after-state", async () => {
    const context = await withStore();
    await mkdir(join(context.workspaceRoot, "docs"), { recursive: true });
    const before = "before content\n";
    const after = "after content\n";
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), before);
    const checkpoint = await context.store.prepare(preparedUpdate());
    await context.store.finalizeApplied(checkpoint.id, {
      afterSha256: hashOf(after),
      absent: false,
    });
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), after);

    const report = await reconcileWorkspaceCheckpoints({
      workspaceRoot: context.workspaceRoot,
      store: context.store,
    });
    expect(report.undoneAfterRestore).toBe(0);
    expect((await context.store.get(checkpoint.id))?.state).toBe("applied");
  });
});

describe("checkpoint storage link rejection", () => {
  it(
    "rejects a fingerprint directory swapped to a symlink after startup",
    {
      skip: !SYMLINKS_SUPPORTED,
    },
    async () => {
      const context = await withStore();
      const checkpoint = await context.store.prepare(preparedUpdate());
      const fingerprintDir = join(context.rootDirectory, context.fingerprint);
      const { rm, symlink, mkdtemp } = await import("node:fs/promises");
      const outside = await mkdtemp(join(tmpdir(), "solaris-cp-outside-"));
      registerTempDir(outside);
      await rm(fingerprintDir, { recursive: true });
      await symlink(outside, fingerprintDir);
      expect(await context.store.get(checkpoint.id)).toBeNull();
      await expect(context.store.loadPreimage(checkpoint.id)).rejects.toThrow();
    },
  );

  it(
    "rejects a checkpoint directory swapped to a junction after startup",
    {
      skip: process.platform !== "win32",
    },
    async () => {
      const context = await withStore();
      const checkpoint = await context.store.prepare(preparedUpdate());
      const checkpointDir = checkpointDirOf(context, checkpoint.id);
      const { rm, mkdtemp } = await import("node:fs/promises");
      const outside = await mkdtemp(join(tmpdir(), "solaris-cp-junction-"));
      registerTempDir(outside);
      await rm(checkpointDir, { recursive: true });
      const { execFileSync } = await import("node:child_process");
      try {
        execFileSync("cmd", ["/c", "mklink", "/J", checkpointDir, outside], { stdio: "ignore" });
      } catch {
        return; // junction creation unsupported in this environment
      }
      expect(await context.store.get(checkpoint.id)).toBeNull();
      await expect(context.store.loadPreimage(checkpoint.id)).rejects.toThrow();
    },
  );

  it("rejects metadata path substitution", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare(preparedUpdate());
    const metadataPath = join(checkpointDirOf(context, checkpoint.id), "metadata.json");
    const { rm, symlink } = await import("node:fs/promises");
    const outside = join(context.rootDirectory, "outside-metadata.json");
    await writeFile(outside, JSON.stringify({ evil: true }));
    await rm(metadataPath);
    await symlink(outside, metadataPath);
    expect(await context.store.get(checkpoint.id)).toBeNull();
  });

  it("rejects preimage path substitution", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare(preparedUpdate());
    const preimagePath = join(checkpointDirOf(context, checkpoint.id), "preimage.bin");
    const { rm, symlink } = await import("node:fs/promises");
    const outside = join(context.rootDirectory, "outside-preimage.bin");
    await writeFile(outside, "attacker bytes");
    await rm(preimagePath);
    await symlink(outside, preimagePath);
    await expect(context.store.loadPreimage(checkpoint.id)).rejects.toThrow();
  });

  it(
    "storage pressure never deletes malicious symlinked entries",
    {
      skip: !SYMLINKS_SUPPORTED,
    },
    async () => {
      const tight = await withStore({ maxCheckpoints: 2 });
      await tight.store.prepare(preparedUpdate({ relativePath: "docs/a.md" }));
      await tight.store.prepare(preparedUpdate({ relativePath: "docs/b.md" }));
      const outsideTarget = join(tight.rootDirectory, "attacker-target.txt");
      await writeFile(outsideTarget, "do not delete me");
      const maliciousDir = join(tight.rootDirectory, tight.fingerprint, "cp_malicious-0001");
      const { mkdir, symlink } = await import("node:fs/promises");
      await mkdir(maliciousDir, { recursive: true });
      await symlink(outsideTarget, join(maliciousDir, "metadata.json"));
      await expect(
        tight.store.prepare(preparedUpdate({ relativePath: "docs/c.md" })),
      ).rejects.toBeInstanceOf(CheckpointStorageLimitError);
      // The refusal deleted nothing: the malicious entry, its symlink, and
      // the outside target all remain byte-identical.
      const entries = await readdir(join(tight.rootDirectory, tight.fingerprint));
      expect(entries).toContain("cp_malicious-0001");
      const { readFile } = await import("node:fs/promises");
      expect(await readFile(outsideTarget, "utf8")).toBe("do not delete me");
    },
  );

  it(
    "storage pressure never deletes through a substituted (symlinked) checkpoint directory",
    {
      skip: !SYMLINKS_SUPPORTED,
    },
    async () => {
      const context = await withStore({ maxCheckpoints: 1 });
      const first = await context.store.prepare(preparedUpdate());
      const checkpointDir = checkpointDirOf(context, first.id);
      const { rm, symlink, lstat } = await import("node:fs/promises");
      const outside = await mkdtemp(join(tmpdir(), "solaris-cp-outside-"));
      registerTempDir(outside);
      await writeFile(join(outside, "victim.txt"), "keep me");
      await rm(checkpointDir, { recursive: true });
      await symlink(outside, checkpointDir);
      // Storage pressure after substitution: the retention pass must fail
      // closed and never touch the substituted entry or its target.
      await expect(context.store.prepare(preparedUpdate())).rejects.toBeInstanceOf(
        CheckpointStorageLimitError,
      );
      expect((await lstat(checkpointDir)).isSymbolicLink()).toBe(true);
      const { readFile } = await import("node:fs/promises");
      expect(await readFile(join(outside, "victim.txt"), "utf8")).toBe("keep me");
    },
  );
});
