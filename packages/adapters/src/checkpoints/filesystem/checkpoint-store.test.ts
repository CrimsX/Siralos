import { createHash, randomUUID } from "node:crypto";
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { CheckpointOperation, PreparedCheckpoint } from "@solaris/core";
import {
  CheckpointStorageLimitError,
  checkedByteTotal,
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

async function expectRefusalPreservesTree(
  context: StoreContext,
  prepare: () => Promise<unknown>,
): Promise<void> {
  const before = await snapshotTree(context.rootDirectory);
  await expect(prepare()).rejects.toBeInstanceOf(CheckpointStorageLimitError);
  expect(await snapshotTree(context.rootDirectory)).toEqual(before);
}

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

/**
 * Byte-for-byte recursive snapshot of a directory tree: relative path ->
 * SHA-256 of content. Any deletion, rename, or content change anywhere in
 * the tree changes the snapshot, so equality before/after an operation
 * proves no destructive filesystem operation occurred. Symlinks and special
 * files are recorded by kind and never opened: reading a FIFO could block
 * forever, and following a symlink would observe the target rather than the
 * entry.
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
      } else if (entry.isFile()) {
        const content = await readFile(absolute);
        snapshot.set(entryRelative, createHash("sha256").update(content).digest("hex"));
      } else {
        const kind = entry.isSymbolicLink()
          ? "symlink"
          : entry.isFIFO()
            ? "fifo"
            : entry.isSocket()
              ? "socket"
              : entry.isBlockDevice()
                ? "block-device"
                : entry.isCharacterDevice()
                  ? "character-device"
                  : "special";
        snapshot.set(entryRelative, `entry-kind:${kind}`);
      }
    }
  };
  await walk(root, "");
  return snapshot;
}

describe("fail-closed capacity verification", () => {
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

  it("rejects a prepared checkpoint whose byte length disagrees with its bytes", async () => {
    const context = await withStore();
    const content = "before content\n";
    await expect(
      context.store.prepare(
        preparedUpdate({
          before: {
            exists: true,
            sha256: hashOf(content),
            byteLength: 99,
            bytes: Buffer.from(content),
          },
        }),
      ),
    ).rejects.toThrow(/byte length/);
  });

  it(
    "refuses when a preimage cannot be inspected at all",
    { skip: process.platform === "win32" },
    async () => {
      const context = await withStore();
      const created = await context.store.prepare({
        ...preparedUpdate(),
        operation: "create",
        before: { exists: false, sha256: null, byteLength: null, bytes: null },
        after: { exists: true, sha256: hashOf("new\n"), byteLength: 4 },
      });
      // An uninspectable checkpoint directory must make capacity
      // unverifiable even though the metadata declares no preimage: an
      // inspection failure is never assumed to consume zero bytes.
      await chmod(checkpointDirOf(context, created.id), 0o000);
      try {
        await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
      } finally {
        await chmod(checkpointDirOf(context, created.id), 0o700).catch(() => undefined);
      }
    },
  );

  it("exact byte boundary including metadata succeeds and one byte over refuses", async () => {
    // The byte limit covers the exact serialized metadata plus the
    // preimage for every checkpoint. The second and third checkpoints
    // serialize to the same metadata size as the first (same id length,
    // same timestamp format, same content), so the per-checkpoint
    // contribution is measured from the actual metadata.json the store
    // wrote for the first checkpoint.
    const roomy = await withStore({ maxStorageBytes: Number.MAX_SAFE_INTEGER });
    const first = await roomy.store.prepare(preparedUpdate());
    const metadataBytes = Buffer.byteLength(
      await readFile(metadataPathOf(roomy, first.id), "utf8"),
      "utf8",
    );
    const perCheckpoint = metadataBytes + Buffer.byteLength("before content\n", "utf8");
    // Two checkpoints fit exactly when the limit is 2x the contribution.
    const exactSecond = await withStore({
      workspaceRoot: roomy.workspaceRoot,
      rootDirectory: roomy.rootDirectory,
      maxStorageBytes: 2 * perCheckpoint,
    });
    expect((await exactSecond.store.prepare(preparedUpdate())).state).toBe("prepared");
    // One byte over the exact limit for three checkpoints refuses before
    // any write, with the tree byte-for-byte unchanged.
    const oneByteOver = await withStore({
      workspaceRoot: roomy.workspaceRoot,
      rootDirectory: roomy.rootDirectory,
      maxStorageBytes: 3 * perCheckpoint - 1,
    });
    await expectRefusalPreservesTree(oneByteOver, () =>
      oneByteOver.store.prepare(preparedUpdate()),
    );
    // Exactly at the limit for three checkpoints succeeds.
    const exactThird = await withStore({
      workspaceRoot: roomy.workspaceRoot,
      rootDirectory: roomy.rootDirectory,
      maxStorageBytes: 3 * perCheckpoint,
    });
    expect((await exactThird.store.prepare(preparedUpdate())).state).toBe("prepared");
  });

  it("accounts unicode metadata by exact UTF-8 bytes", async () => {
    const toolName = "workspace.edit_file-\u65e5\u672c\u8a9e-\u{1F600}";
    const roomy = await withStore({ maxStorageBytes: Number.MAX_SAFE_INTEGER });
    const first = await roomy.store.prepare(preparedUpdate({ toolName }));
    // The stored metadata contains multibyte characters; accounting must
    // use UTF-8 bytes, not string length.
    const metadataBytes = Buffer.byteLength(
      await readFile(metadataPathOf(roomy, first.id), "utf8"),
      "utf8",
    );
    const perCheckpoint = metadataBytes + Buffer.byteLength("before content\n", "utf8");
    const exactSecond = await withStore({
      workspaceRoot: roomy.workspaceRoot,
      rootDirectory: roomy.rootDirectory,
      maxStorageBytes: 2 * perCheckpoint,
    });
    expect((await exactSecond.store.prepare(preparedUpdate({ toolName }))).state).toBe("prepared");
    const oneByteOver = await withStore({
      workspaceRoot: roomy.workspaceRoot,
      rootDirectory: roomy.rootDirectory,
      maxStorageBytes: 3 * perCheckpoint - 1,
    });
    await expectRefusalPreservesTree(oneByteOver, () =>
      oneByteOver.store.prepare(preparedUpdate({ toolName })),
    );
  });
});

describe("prepared record validation", () => {
  const beforeContent = "before content\n";
  const afterContent = "after content\n";

  async function expectInputRefusal(
    context: StoreContext,
    overrides: Partial<PreparedCheckpoint>,
    expected?: RegExp,
  ): Promise<void> {
    const before = await snapshotTree(context.rootDirectory);
    const pending = context.store.prepare(preparedUpdate(overrides));
    if (expected === undefined) {
      await expect(pending).rejects.toThrow();
    } else {
      await expect(pending).rejects.toThrow(expected);
    }
    // Refusal happens before any capacity inspection or filesystem
    // activity: no directory or file was created or changed.
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
    // A subsequent valid preparation still works.
    const valid = await context.store.prepare(preparedUpdate());
    expect(valid.state).toBe("prepared");
  }

  it("SELF_POISON_REFUSED: rejects exists true with missing preimage bytes", async () => {
    const context = await withStore();
    await expectInputRefusal(
      context,
      {
        before: {
          exists: true,
          sha256: hashOf(beforeContent),
          byteLength: Buffer.byteLength(beforeContent),
          bytes: null,
        },
      },
      /preimage bytes are missing/,
    );
  });

  it("rejects exists false with non-null preimage bytes", async () => {
    const context = await withStore();
    await expectInputRefusal(context, {
      before: { exists: false, sha256: null, byteLength: null, bytes: Buffer.from("x") },
    });
  });

  it("rejects exists false with a hash", async () => {
    const context = await withStore();
    await expectInputRefusal(context, {
      before: { exists: false, sha256: hashOf("x"), byteLength: null, bytes: null },
    });
  });

  it("rejects exists false with a byte length", async () => {
    const context = await withStore();
    await expectInputRefusal(context, {
      before: { exists: false, sha256: null, byteLength: 4, bytes: null },
    });
  });

  it("rejects bytes whose length differs from the metadata", async () => {
    const context = await withStore();
    await expectInputRefusal(context, {
      before: {
        exists: true,
        sha256: hashOf(beforeContent),
        byteLength: 99,
        bytes: Buffer.from(beforeContent),
      },
    });
  });

  it("rejects bytes whose SHA-256 differs", async () => {
    const context = await withStore();
    await expectInputRefusal(context, {
      before: {
        exists: true,
        sha256: "f".repeat(64),
        byteLength: Buffer.byteLength(beforeContent),
        bytes: Buffer.from(beforeContent),
      },
    });
  });

  it("rejects an invalid before hash", async () => {
    const context = await withStore();
    await expectInputRefusal(
      context,
      {
        before: {
          exists: true,
          sha256: "not-hex",
          byteLength: Buffer.byteLength(beforeContent),
          bytes: Buffer.from(beforeContent),
        },
      },
      /before sha256 is invalid/,
    );
  });

  it("rejects an invalid after hash", async () => {
    const context = await withStore();
    await expectInputRefusal(
      context,
      { after: { exists: true, sha256: "not-hex", byteLength: Buffer.byteLength(afterContent) } },
      /after sha256 is invalid/,
    );
  });

  it("rejects negative, fractional, unsafe, string, and null lengths in invalid positions", async () => {
    const context = await withStore();
    for (const byteLength of [-1, 1.5, "14", null, Number.MAX_SAFE_INTEGER + 1]) {
      await expectInputRefusal(context, {
        before: {
          exists: true,
          sha256: hashOf(beforeContent),
          byteLength: byteLength as unknown as number,
          bytes: Buffer.from(beforeContent),
        },
      });
      await expectInputRefusal(context, {
        after: {
          exists: true,
          sha256: hashOf(afterContent),
          byteLength: byteLength as unknown as number,
        },
      });
    }
  });

  it("rejects oversized preimage bytes", async () => {
    const context = await withStore({ maxPreimageBytes: 16 });
    await expectInputRefusal(
      context,
      {
        before: {
          exists: true,
          sha256: hashOf("x".repeat(24)),
          byteLength: 24,
          bytes: Buffer.from("x".repeat(24)),
        },
      },
      /configured maximum/,
    );
  });

  it("rejects an invalid operation", async () => {
    const context = await withStore();
    for (const operation of ["append", 42, null]) {
      await expectInputRefusal(context, {
        operation: operation as unknown as PreparedCheckpoint["operation"],
      });
    }
  });

  it("rejects an invalid relative path", async () => {
    const context = await withStore();
    for (const relativePath of ["", "..", "/absolute", null]) {
      await expectInputRefusal(context, {
        relativePath: relativePath as unknown as string,
      });
    }
  });

  it("rejects an invalid tool name", async () => {
    const context = await withStore();
    for (const toolName of ["", "x".repeat(257), 42, null]) {
      await expectInputRefusal(context, { toolName: toolName as unknown as string });
    }
  });

  it("rejects invalid preview counts", async () => {
    const context = await withStore();
    for (const count of [
      -1,
      1.5,
      "1",
      Number.NaN,
      Number.POSITIVE_INFINITY,
      null,
      Number.MAX_SAFE_INTEGER + 1,
    ]) {
      await expectInputRefusal(context, {
        preview: { addedLines: count as unknown as number, removedLines: 1 },
      });
      await expectInputRefusal(context, {
        preview: { addedLines: 1, removedLines: count as unknown as number },
      });
    }
  });

  it("rejects a relative path whose serialized metadata would exceed the metadata size limit", async () => {
    const context = await withStore();
    await expectInputRefusal(context, { relativePath: "a".repeat(80 * 1024) }, /metadata/);
  });

  it("accepts a valid zero-byte preimage", async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare(
      preparedUpdate({
        before: { exists: true, sha256: hashOf(""), byteLength: 0, bytes: Buffer.alloc(0) },
      }),
    );
    expect(checkpoint.before.byteLength).toBe(0);
    expect((await context.store.loadPreimage(checkpoint.id))?.byteLength).toBe(0);
  });

  it("accepts a valid create checkpoint with no preimage", async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare({
      ...preparedUpdate(),
      operation: "create",
      before: { exists: false, sha256: null, byteLength: null, bytes: null },
      after: { exists: true, sha256: hashOf("new\n"), byteLength: 4 },
    });
    expect(checkpoint.before.exists).toBe(false);
    expect(await context.store.loadPreimage(checkpoint.id)).toBeNull();
  });

  it("accepts a valid delete checkpoint", async () => {
    const context = await withStore();
    const checkpoint = await context.store.prepare({
      ...preparedUpdate(),
      operation: "delete",
      after: { exists: false, sha256: null, byteLength: null },
    });
    expect(checkpoint.after.exists).toBe(false);
    expect(await context.store.loadPreimage(checkpoint.id)).not.toBeNull();
  });
});

describe("operation-state invariants", () => {
  const beforeContent = "before content\n";
  const afterContent = "after content\n";

  function combinationBefore(beforeExists: boolean): PreparedCheckpoint["before"] {
    if (beforeExists) {
      return {
        exists: true,
        sha256: hashOf(beforeContent),
        byteLength: Buffer.byteLength(beforeContent),
        bytes: Buffer.from(beforeContent),
      };
    }
    return { exists: false, sha256: null, byteLength: null, bytes: null };
  }

  function combinationAfter(afterExists: boolean): PreparedCheckpoint["after"] {
    if (afterExists) {
      return {
        exists: true,
        sha256: hashOf(afterContent),
        byteLength: Buffer.byteLength(afterContent),
      };
    }
    return { exists: false, sha256: null, byteLength: null };
  }

  const MATRIX_COMBINATIONS: ReadonlyArray<
    readonly [CheckpointOperation, boolean, boolean, boolean]
  > = [
    ["create", false, false, false],
    ["create", false, true, true],
    ["create", true, false, false],
    ["create", true, true, false],
    ["update", false, false, false],
    ["update", false, true, false],
    ["update", true, false, false],
    ["update", true, true, true],
    ["delete", false, false, false],
    ["delete", false, true, false],
    ["delete", true, false, true],
    ["delete", true, true, false],
  ];

  it("CREATE_WITH_PREIMAGE_REFUSED: rejects create with an existing before-state", async () => {
    const context = await withStore();
    const before = await snapshotTree(context.rootDirectory);
    await expect(context.store.prepare(preparedUpdate({ operation: "create" }))).rejects.toThrow(
      /existence transition/,
    );
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
    const valid = await context.store.prepare({
      ...preparedUpdate(),
      operation: "create",
      before: { exists: false, sha256: null, byteLength: null, bytes: null },
      after: { exists: true, sha256: hashOf("new\n"), byteLength: 4 },
    });
    expect(valid.state).toBe("prepared");
  });

  it("enforces the operation-state matrix for prepared records with zero filesystem changes on refusal", async () => {
    for (const [operation, beforeExists, afterExists, shouldPass] of MATRIX_COMBINATIONS) {
      const context = await withStore();
      const before = await snapshotTree(context.rootDirectory);
      const pending = context.store.prepare(
        preparedUpdate({
          operation,
          before: combinationBefore(beforeExists),
          after: combinationAfter(afterExists),
        }),
      );
      if (shouldPass) {
        const checkpoint = await pending;
        expect(checkpoint.operation).toBe(operation);
      } else {
        await expect(pending).rejects.toThrow(/existence transition/);
        expect(await snapshotTree(context.rootDirectory)).toEqual(before);
        const valid = await context.store.prepare(preparedUpdate());
        expect(valid.state).toBe("prepared");
      }
    }
  });

  it("enforces the operation-state matrix for stored metadata: get() refuses and capacity is unverifiable", async () => {
    for (const [operation, beforeExists, afterExists, shouldPass] of MATRIX_COMBINATIONS) {
      const context = await withStore();
      const first = await context.store.prepare(preparedUpdate());
      await rewriteMetadata(context, first.id, (record) => {
        record["operation"] = operation;
        record["before"] = {
          exists: beforeExists,
          sha256: beforeExists ? hashOf(beforeContent) : null,
          byteLength: beforeExists ? Buffer.byteLength(beforeContent) : null,
        };
        record["after"] = {
          exists: afterExists,
          sha256: afterExists ? hashOf(afterContent) : null,
          byteLength: afterExists ? Buffer.byteLength(afterContent) : null,
        };
      });
      if (shouldPass) {
        expect((await context.store.get(first.id))?.operation).toBe(operation);
      } else {
        expect(await context.store.get(first.id)).toBeNull();
        await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
      }
    }
  });

  it("rejects delete without a required preimage", async () => {
    const context = await withStore();
    const before = await snapshotTree(context.rootDirectory);
    await expect(
      context.store.prepare(
        preparedUpdate({
          operation: "delete",
          before: { exists: false, sha256: null, byteLength: null, bytes: null },
          after: { exists: false, sha256: null, byteLength: null },
        }),
      ),
    ).rejects.toThrow(/existence transition/);
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
  });

  it("rejects create whose after-state is absent", async () => {
    const context = await withStore();
    const before = await snapshotTree(context.rootDirectory);
    await expect(
      context.store.prepare(
        preparedUpdate({
          operation: "create",
          before: { exists: false, sha256: null, byteLength: null, bytes: null },
          after: { exists: false, sha256: null, byteLength: null },
        }),
      ),
    ).rejects.toThrow(/existence transition/);
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
  });

  it("rejects update whose before-state is absent", async () => {
    const context = await withStore();
    const before = await snapshotTree(context.rootDirectory);
    await expect(
      context.store.prepare(
        preparedUpdate({
          operation: "update",
          before: { exists: false, sha256: null, byteLength: null, bytes: null },
        }),
      ),
    ).rejects.toThrow(/existence transition/);
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
  });

  it("rejects update whose after-state is absent", async () => {
    const context = await withStore();
    const before = await snapshotTree(context.rootDirectory);
    await expect(
      context.store.prepare(
        preparedUpdate({
          operation: "update",
          after: { exists: false, sha256: null, byteLength: null },
        }),
      ),
    ).rejects.toThrow(/existence transition/);
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
  });

  it("rejects delete whose after-state is present", async () => {
    const context = await withStore();
    const before = await snapshotTree(context.rootDirectory);
    await expect(context.store.prepare(preparedUpdate({ operation: "delete" }))).rejects.toThrow(
      /existence transition/,
    );
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
  });
});

describe("capacity preimage content verification", () => {
  const originalContent = "before content\n";

  function preimagePathOf(context: StoreContext, checkpointId: string): string {
    return join(checkpointDirOf(context, checkpointId), "preimage.bin");
  }

  function restorePreimage(
    context: StoreContext,
    checkpointId: string,
    content: string,
  ): Promise<void> {
    return writeFile(preimagePathOf(context, checkpointId), content, "utf8");
  }

  async function expectCorruptionRefusal(context: StoreContext): Promise<void> {
    const before = await snapshotTree(context.rootDirectory);
    await expect(context.store.prepare(preparedUpdate())).rejects.toBeInstanceOf(
      CheckpointStorageLimitError,
    );
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
  }

  it("CORRUPT_PREIMAGE_REFUSED: refuses a same-size corrupted preimage and accepts after repair", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    await restorePreimage(context, first.id, "tampered bytes!");
    await expectCorruptionRefusal(context);
    // Repair: the original bytes make capacity provable again.
    await restorePreimage(context, first.id, originalContent);
    const valid = await context.store.prepare(preparedUpdate());
    expect(valid.state).toBe("prepared");
  });

  it("refuses a shorter corrupted preimage and accepts after repair", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    await restorePreimage(context, first.id, "short");
    await expectCorruptionRefusal(context);
    await restorePreimage(context, first.id, originalContent);
    expect((await context.store.prepare(preparedUpdate())).state).toBe("prepared");
  });

  it("refuses a longer corrupted preimage and accepts after repair", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    await restorePreimage(context, first.id, "this content is far too long");
    await expectCorruptionRefusal(context);
    await restorePreimage(context, first.id, originalContent);
    expect((await context.store.prepare(preparedUpdate())).state).toBe("prepared");
  });

  it("refuses an empty preimage replaced with non-empty content and accepts after repair", async () => {
    const context = await withStore();
    const first = await context.store.prepare(
      preparedUpdate({
        before: { exists: true, sha256: hashOf(""), byteLength: 0, bytes: Buffer.alloc(0) },
      }),
    );
    await restorePreimage(context, first.id, "x");
    await expectCorruptionRefusal(context);
    await writeFile(preimagePathOf(context, first.id), "");
    expect((await context.store.prepare(preparedUpdate())).state).toBe("prepared");
  });

  it("refuses a non-empty preimage replaced with empty content and accepts after repair", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    await writeFile(preimagePathOf(context, first.id), "");
    await expectCorruptionRefusal(context);
    await restorePreimage(context, first.id, originalContent);
    expect((await context.store.prepare(preparedUpdate())).state).toBe("prepared");
  });

  it("refuses when the metadata hash is changed while bytes remain unchanged and accepts after repair", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    const originalHash = (await readMetadata(context, first.id))["before"] as Record<
      string,
      unknown
    >;
    await rewriteMetadata(context, first.id, (record) => {
      (record["before"] as Record<string, unknown>)["sha256"] = "a".repeat(64);
    });
    await expectCorruptionRefusal(context);
    await rewriteMetadata(context, first.id, (record) => {
      (record["before"] as Record<string, unknown>)["sha256"] = originalHash["sha256"];
    });
    expect((await context.store.prepare(preparedUpdate())).state).toBe("prepared");
  });

  it("refuses a preimage substituted during inspection via the deterministic hook and accepts after repair", async () => {
    let substituted = false;
    const context = await withStore({
      retentionPreimageInspectionHook: async (preimagePath: string) => {
        if (!substituted) {
          substituted = true;
          await writeFile(preimagePath, "swapped bytes!", "utf8");
        }
      },
    });
    const first = await context.store.prepare(preparedUpdate());
    // The hook swaps the preimage between the lstat and the open: the
    // bounded verifier reads the substituted content and its hash does not
    // match the metadata.
    const before = await snapshotTree(context.rootDirectory);
    await expect(context.store.prepare(preparedUpdate())).rejects.toBeInstanceOf(
      CheckpointStorageLimitError,
    );
    // Repair the hook's own substitution, then prove the store changed
    // nothing beyond it and capacity is provable again.
    await restorePreimage(context, first.id, originalContent);
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
    expect((await context.store.prepare(preparedUpdate())).state).toBe("prepared");
  });

  it("refuses a preimage that disappears during inspection via the deterministic hook", async () => {
    let deleted = false;
    const context = await withStore({
      retentionPreimageInspectionHook: async (preimagePath: string) => {
        if (!deleted) {
          deleted = true;
          await rm(preimagePath, { force: true });
        }
      },
    });
    const first = await context.store.prepare(preparedUpdate());
    const before = await snapshotTree(context.rootDirectory);
    await expect(context.store.prepare(preparedUpdate())).rejects.toBeInstanceOf(
      CheckpointStorageLimitError,
    );
    // Repair the hook's own deletion, then prove the store changed nothing
    // beyond it and capacity is provable again.
    await restorePreimage(context, first.id, originalContent);
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
    expect((await context.store.prepare(preparedUpdate())).state).toBe("prepared");
  });

  it(
    "refuses a preimage that becomes unreadable",
    { skip: process.platform === "win32" },
    async () => {
      const context = await withStore();
      const first = await context.store.prepare(preparedUpdate());
      await chmod(preimagePathOf(context, first.id), 0o000);
      try {
        await expectCorruptionRefusal(context);
      } finally {
        await chmod(preimagePathOf(context, first.id), 0o600).catch(() => undefined);
      }
      expect((await context.store.prepare(preparedUpdate())).state).toBe("prepared");
    },
  );

  it("refuses an oversized preimage and accepts after repair", async () => {
    const context = await withStore({ maxPreimageBytes: 16 });
    const first = await context.store.prepare(preparedUpdate());
    await writeFile(preimagePathOf(context, first.id), "x".repeat(24), "utf8");
    await expectCorruptionRefusal(context);
    await restorePreimage(context, first.id, originalContent);
    expect((await context.store.prepare(preparedUpdate())).state).toBe("prepared");
  });

  it(
    "refuses a junction substituted for the preimage and accepts after repair",
    { skip: process.platform !== "win32" },
    async () => {
      const context = await withStore();
      const first = await context.store.prepare(preparedUpdate());
      const preimagePath = preimagePathOf(context, first.id);
      const outside = await mkdtemp(join(tmpdir(), "solaris-cp-junction-"));
      registerTempDir(outside);
      await rm(preimagePath, { force: true });
      const { execFileSync } = await import("node:child_process");
      try {
        execFileSync("cmd", ["/c", "mklink", "/J", preimagePath, outside], { stdio: "ignore" });
      } catch {
        return; // junction creation unsupported in this environment
      }
      await expectCorruptionRefusal(context);
      await rm(preimagePath, { recursive: true, force: true });
      await restorePreimage(context, first.id, originalContent);
      expect((await context.store.prepare(preparedUpdate())).state).toBe("prepared");
    },
  );

  it("cancellation during bounded verification aborts without mutation", async () => {
    const controller = new AbortController();
    const context = await withStore({
      retentionPreimageInspectionHook: () => {
        controller.abort();
      },
    });
    await context.store.prepare(preparedUpdate());
    const before = await snapshotTree(context.rootDirectory);
    await expect(context.store.prepare(preparedUpdate(), controller.signal)).rejects.toMatchObject({
      name: "AbortError",
    });
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
    // A fresh (non-aborted) preparation succeeds.
    expect((await context.store.prepare(preparedUpdate())).state).toBe("prepared");
  });

  it("deadline expiry during bounded verification refuses without mutation", async () => {
    const context = await withStore({
      retentionDeadlineMs: 60,
      retentionPreimageInspectionHook: async () => {
        await new Promise((resolve) => setTimeout(resolve, 200));
      },
    });
    await context.store.prepare(preparedUpdate());
    const before = await snapshotTree(context.rootDirectory);
    await expect(context.store.prepare(preparedUpdate())).rejects.toBeInstanceOf(
      CheckpointStorageLimitError,
    );
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
    // A fresh store over the same tree (default deadline, no sleeping hook)
    // proves capacity and accepts a new checkpoint.
    const refreshed = await withStore({
      workspaceRoot: context.workspaceRoot,
      rootDirectory: context.rootDirectory,
    });
    expect((await refreshed.store.prepare(preparedUpdate())).state).toBe("prepared");
  });
});

describe("storage accounting and unexpected entries", () => {
  it("STRAY_STORAGE_REFUSED: refuses the known reproduction with a 1 MiB stray file", async () => {
    // The limit is generous enough that the byte total alone would never
    // refuse; the refusal is attributable to the unexpected entry, and the
    // tree is preserved byte-for-byte.
    const context = await withStore({ maxStorageBytes: 2 * 1024 * 1024 });
    const first = await context.store.prepare(preparedUpdate());
    await writeFile(
      join(checkpointDirOf(context, first.id), "junk.bin"),
      Buffer.alloc(1024 * 1024, 0x62),
    );
    const before = await snapshotTree(context.rootDirectory);
    await expect(context.store.prepare(preparedUpdate())).rejects.toThrow(/junk\.bin/);
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
  });

  it("refuses on a one-byte unexpected file", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    await writeFile(join(checkpointDirOf(context, first.id), "x"), "x", "utf8");
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("refuses on an unexpected nested directory", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    await mkdir(join(checkpointDirOf(context, first.id), "nested"), { recursive: true });
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("refuses on a leftover temporary metadata file", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    await writeFile(
      join(checkpointDirOf(context, first.id), `metadata.json.tmp-${randomUUID()}`),
      "partial",
      "utf8",
    );
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("refuses on unexpected files at the workspace fingerprint level", async () => {
    const context = await withStore();
    await context.store.prepare(preparedUpdate());
    await writeFile(join(context.rootDirectory, context.fingerprint, "junk.txt"), "junk", "utf8");
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("refuses case-variant duplicate names of known checkpoint files", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    await writeFile(join(checkpointDirOf(context, first.id), "Metadata.json"), "{}", "utf8");
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("refuses when a checkpoint directory holds more entries than the known layout", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    for (let index = 0; index < 5; index += 1) {
      await writeFile(join(checkpointDirOf(context, first.id), `extra-${index}.bin`), "x");
    }
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it(
    "refuses capacity when a checkpoint directory is swapped to a junction",
    { skip: process.platform !== "win32" },
    async () => {
      const context = await withStore();
      const first = await context.store.prepare(preparedUpdate());
      const checkpointDir = checkpointDirOf(context, first.id);
      const outside = await mkdtemp(join(tmpdir(), "solaris-cp-junction-"));
      registerTempDir(outside);
      await rm(checkpointDir, { recursive: true });
      const { execFileSync } = await import("node:child_process");
      try {
        execFileSync("cmd", ["/c", "mklink", "/J", checkpointDir, outside], { stdio: "ignore" });
      } catch {
        return; // junction creation unsupported in this environment
      }
      await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
    },
  );

  it(
    "refuses when a special file occupies a known checkpoint file slot",
    { skip: process.platform === "win32" },
    async () => {
      const context = await withStore();
      const created = await context.store.prepare({
        ...preparedUpdate(),
        operation: "create",
        before: { exists: false, sha256: null, byteLength: null, bytes: null },
        after: { exists: true, sha256: hashOf("new\n"), byteLength: 4 },
      });
      const { execFileSync } = await import("node:child_process");
      try {
        execFileSync("mkfifo", [join(checkpointDirOf(context, created.id), "preimage.bin")], {
          stdio: "ignore",
        });
      } catch {
        return; // mkfifo unsupported in this environment
      }
      await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
    },
  );

  it("refuses when metadata is missing", async () => {
    const context = await withStore();
    const id = `cp_${randomUUID()}`;
    const dir = await entryDirOf(context, id);
    await writeFile(join(dir, "preimage.bin"), Buffer.alloc(1024, 0x61));
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("refuses when a required preimage is missing", async () => {
    const context = await withStore();
    const first = await context.store.prepare(preparedUpdate());
    await rm(join(checkpointDirOf(context, first.id), "preimage.bin"), { force: true });
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it("refuses an unexpected preimage for a create checkpoint", async () => {
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

  it("refuses oversized existing metadata", async () => {
    const context = await withStore();
    const id = `cp_${randomUUID()}`;
    await entryDirOf(context, id);
    await writeFile(
      metadataPathOf(context, id),
      JSON.stringify({ padding: "x".repeat(70 * 1024) }),
      "utf8",
    );
    await expectRefusalPreservesTree(context, () => context.store.prepare(preparedUpdate()));
  });

  it(
    "refuses a substitute checkpoint that is a symbolic link without touching its target",
    { skip: !SYMLINKS_SUPPORTED },
    async () => {
      const context = await withStore({ maxCheckpoints: 1 });
      const first = await context.store.prepare(preparedUpdate());
      const checkpointDir = checkpointDirOf(context, first.id);
      const { symlink } = await import("node:fs/promises");
      const outside = await mkdtemp(join(tmpdir(), "solaris-cp-outside-"));
      registerTempDir(outside);
      await writeFile(join(outside, "victim.txt"), "keep me");
      await rm(checkpointDir, { recursive: true });
      await symlink(outside, checkpointDir);
      const before = await snapshotTree(context.rootDirectory);
      await expect(context.store.prepare(preparedUpdate())).rejects.toBeInstanceOf(
        CheckpointStorageLimitError,
      );
      expect(await snapshotTree(context.rootDirectory)).toEqual(before);
      const { readFile } = await import("node:fs/promises");
      expect(await readFile(join(outside, "victim.txt"), "utf8")).toBe("keep me");
    },
  );

  it("proves the pass is bounded: per-checkpoint enumeration cap and global inspected-entry cap never materialize hostile fanout", async () => {
    const context = await withStore({ maxCheckpoints: 2 });
    const first = await context.store.prepare(preparedUpdate());
    // A hostile checkpoint directory with far more entries than the known
    // layout: enumeration stops at the cap and refuses without ever
    // materializing the full listing.
    const dir = checkpointDirOf(context, first.id);
    for (let index = 0; index < 1000; index += 1) {
      await writeFile(join(dir, `f-${index}.bin`), "x");
    }
    const before = await snapshotTree(context.rootDirectory);
    await expect(context.store.prepare(preparedUpdate())).rejects.toBeInstanceOf(
      CheckpointStorageLimitError,
    );
    expect(await snapshotTree(context.rootDirectory)).toEqual(before);
  });
});

describe("checkedByteTotal overflow-safe accounting", () => {
  it("adds safe non-negative totals", () => {
    expect(checkedByteTotal(0, 0)).toBe(0);
    expect(checkedByteTotal(5, 7)).toBe(12);
    expect(checkedByteTotal(Number.MAX_SAFE_INTEGER, 0)).toBe(Number.MAX_SAFE_INTEGER);
  });

  it("rejects non-safe or negative operands", () => {
    for (const value of [
      -1,
      1.5,
      Number.NaN,
      Number.POSITIVE_INFINITY,
      Number.MAX_SAFE_INTEGER + 1,
    ]) {
      expect(() => checkedByteTotal(value, 0)).toThrow(RangeError);
      expect(() => checkedByteTotal(0, value)).toThrow(RangeError);
    }
  });

  it("rejects totals that exceed the safe integer range", () => {
    expect(() => checkedByteTotal(Number.MAX_SAFE_INTEGER, 1)).toThrow(RangeError);
    expect(() => checkedByteTotal(Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER)).toThrow(
      RangeError,
    );
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
    // The byte limit counts the exact serialized metadata and the preimage
    // for every checkpoint: two checkpoints fit exactly, and a third is
    // refused with zero deletion.
    const roomy = await withStore({ maxStorageBytes: Number.MAX_SAFE_INTEGER });
    const first = await roomy.store.prepare(preparedUpdate());
    const metadataBytes = Buffer.byteLength(
      await readFile(metadataPathOf(roomy, first.id), "utf8"),
      "utf8",
    );
    const perCheckpoint = metadataBytes + Buffer.byteLength("before content\n", "utf8");
    const tight = await withStore({
      workspaceRoot: roomy.workspaceRoot,
      rootDirectory: roomy.rootDirectory,
      maxStorageBytes: 2 * perCheckpoint,
    });
    await tight.store.prepare(preparedUpdate());
    const before = await snapshotTree(tight.rootDirectory);
    await expect(tight.store.prepare(preparedUpdate())).rejects.toBeInstanceOf(
      CheckpointStorageLimitError,
    );
    expect(await snapshotTree(tight.rootDirectory)).toEqual(before);
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
