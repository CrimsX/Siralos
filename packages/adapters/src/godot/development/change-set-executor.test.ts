import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  ChangeSetApplyRequest,
  ChangeSetFilePrimitives,
  CheckpointStore,
} from "@solaris/core";
import { createHash } from "node:crypto";
import {
  applyChangeSetProtocol,
  CHANGE_SET_EXECUTION_UNAVAILABLE_MESSAGE,
  createDevelopmentChangeSetApplier,
} from "./change-set-executor.js";
import {
  cleanupTempCheckpointDirs,
  createTempCheckpointStore,
  createTempWorkspace,
  type TempWorkspace,
} from "../../tools/workspace/workspace-fixtures.js";

function sha256Of(text: string): string {
  return createHash("sha256").update(text).digest("hex");
}

/** In-memory file primitives implementing the exact-apply semantics. */
class InMemoryPrimitives implements ChangeSetFilePrimitives {
  readonly files = new Map<string, string>();

  constructor(initial: Readonly<Record<string, string>> = {}) {
    for (const [path, content] of Object.entries(initial)) {
      this.files.set(path, content);
    }
  }

  readFile(path: string): Promise<{ readonly exists: boolean; readonly sha256: string | null }> {
    const content = this.files.get(path);
    return Promise.resolve(
      content === undefined
        ? { exists: false, sha256: null }
        : { exists: true, sha256: sha256Of(content) },
    );
  }

  readContent(path: string): Promise<{
    readonly exists: boolean;
    readonly sha256: string | null;
    readonly content: string | null;
  }> {
    const content = this.files.get(path);
    return Promise.resolve(
      content === undefined
        ? { exists: false, sha256: null, content: null }
        : { exists: true, sha256: sha256Of(content), content },
    );
  }

  writeFile(path: string, content: string): Promise<void> {
    this.files.set(path, content);
    return Promise.resolve();
  }

  deleteFile(path: string): Promise<void> {
    this.files.delete(path);
    return Promise.resolve();
  }
}

describe("createDevelopmentChangeSetApplier", () => {
  it("reports unavailable when the platform gate is closed", async () => {
    const workspace = await createTempWorkspace();
    try {
      const applier = createDevelopmentChangeSetApplier({
        store: await createTempCheckpointStore(workspace.root),
        lock: createTestLock(),
        toolName: "workspace.apply_text_changeset",
        canApplyIdentityBound: false,
      });
      expect(await applier.isAvailable()).toBe(false);
      const outcome = await applier.apply(
        { changeSetId: "cs-1", files: [], toolName: "workspace.apply_text_changeset" },
        new InMemoryPrimitives(),
      );
      expect(outcome.status).toBe("unavailable");
      if (outcome.status === "unavailable") {
        expect(outcome.message).toBe(CHANGE_SET_EXECUTION_UNAVAILABLE_MESSAGE);
      }
    } finally {
      await workspace.cleanup();
      await cleanupTempCheckpointDirs();
    }
  });
});

describe("applyChangeSetProtocol", () => {
  let workspace: TempWorkspace;
  let store: CheckpointStore;
  beforeEach(async () => {
    workspace = await createTempWorkspace();
    store = await createTempCheckpointStore(workspace.root);
  });
  afterEach(async () => {
    await workspace.cleanup();
    await cleanupTempCheckpointDirs();
  });

  const A_BEFORE = "extends Node\n";
  const B_BEFORE = "extends CharacterBody2D\n";
  const A_AFTER = "extends Node\nfunc heal():\n\tpass\n";

  const deps = (canApplyIdentityBound = true) => ({
    store,
    lock: createTestLock(),
    toolName: "workspace.apply_text_changeset",
    canApplyIdentityBound,
  });

  const applyRequest = (
    files: ChangeSetApplyRequest["files"],
    changeSetId = "cs-test",
  ): ChangeSetApplyRequest => ({ changeSetId, files, toolName: "workspace.apply_text_changeset" });

  it("applies a one-file edit, verifies the post-state hash, and checkpoints it", async () => {
    const primitives = new InMemoryPrimitives({ "a.gd": A_BEFORE });
    const request = applyRequest([
      {
        path: "a.gd",
        operation: "update",
        expectedSha256: sha256Of(A_BEFORE),
        content: A_AFTER,
        beforeSha256: sha256Of(A_BEFORE),
        afterSha256: sha256Of(A_AFTER),
        addedLines: 2,
        removedLines: 0,
      },
    ]);
    const outcome = await applyChangeSetProtocol(request, primitives, deps());
    expect(outcome.status).toBe("applied");
    if (outcome.status !== "applied") {
      return;
    }
    expect(primitives.files.get("a.gd")).toBe(A_AFTER);
    expect(outcome.checkpointIds).toHaveLength(1);
    const checkpoints = await store.list();
    expect(checkpoints).toHaveLength(1);
    expect(checkpoints[0]?.operation).toBe("update");
    expect(checkpoints[0]?.before.sha256).toBe(sha256Of(A_BEFORE));
    expect(checkpoints[0]?.after.sha256).toBe(sha256Of(A_AFTER));
    expect(await store.loadPreimage(outcome.checkpointIds[0]!)).toEqual(
      new TextEncoder().encode(A_BEFORE),
    );
  });

  it("applies create + edit + delete and checkpoints every file", async () => {
    const primitives = new InMemoryPrimitives({ "a.gd": A_BEFORE, "b.gd": B_BEFORE });
    const outcome = await applyChangeSetProtocol(
      applyRequest([
        {
          path: "c.gd",
          operation: "create",
          expectedSha256: null,
          content: "extends Node2D\n",
          beforeSha256: null,
          afterSha256: sha256Of("extends Node2D\n"),
          addedLines: 1,
          removedLines: 0,
        },
        {
          path: "a.gd",
          operation: "update",
          expectedSha256: sha256Of(A_BEFORE),
          content: A_AFTER,
          beforeSha256: sha256Of(A_BEFORE),
          afterSha256: sha256Of(A_AFTER),
          addedLines: 2,
          removedLines: 0,
        },
        {
          path: "b.gd",
          operation: "delete",
          expectedSha256: sha256Of(B_BEFORE),
          content: null,
          beforeSha256: sha256Of(B_BEFORE),
          afterSha256: null,
          addedLines: 0,
          removedLines: 1,
        },
      ]),
      primitives,
      deps(),
    );
    expect(outcome.status).toBe("applied");
    if (outcome.status !== "applied") {
      return;
    }
    expect(primitives.files.has("c.gd")).toBe(true);
    expect(primitives.files.get("a.gd")).toBe(A_AFTER);
    expect(primitives.files.has("b.gd")).toBe(false);
    const checkpoints = await store.list();
    expect(checkpoints.map((entry) => entry.operation).sort()).toEqual(["create", "delete", "update"]);
  });

  it("conflicts before any write when a stale pre-state is detected", async () => {
    const primitives = new InMemoryPrimitives({ "a.gd": A_BEFORE });
    const outcome = await applyChangeSetProtocol(
      applyRequest([
        {
          path: "a.gd",
          operation: "update",
          expectedSha256: "f".repeat(64),
          content: A_AFTER,
          beforeSha256: "f".repeat(64),
          afterSha256: sha256Of(A_AFTER),
          addedLines: 1,
          removedLines: 0,
        },
      ]),
      primitives,
      deps(),
    );
    expect(outcome.status).toBe("conflict");
    expect(primitives.files.get("a.gd")).toBe(A_BEFORE);
    expect(await store.list()).toHaveLength(0);
  });

  it("conflicts when a create target appeared since preparation", async () => {
    const primitives = new InMemoryPrimitives({ "a.gd": "exists\n" });
    const outcome = await applyChangeSetProtocol(
      applyRequest([
        {
          path: "a.gd",
          operation: "create",
          expectedSha256: null,
          content: "new\n",
          beforeSha256: null,
          afterSha256: sha256Of("new\n"),
          addedLines: 1,
          removedLines: 0,
        },
      ]),
      primitives,
      deps(),
    );
    expect(outcome.status).toBe("conflict");
  });

  it("applies nothing when a checkpoint fails", async () => {
    const primitives = new InMemoryPrimitives({ "a.gd": A_BEFORE });
    const failingStore = {
      prepare: () =>
        Promise.reject(new Error("checkpoint capacity refused")),
    } as unknown as CheckpointStore;
    const outcome = await applyChangeSetProtocol(
      applyRequest([
        {
          path: "a.gd",
          operation: "update",
          expectedSha256: sha256Of(A_BEFORE),
          content: A_AFTER,
          beforeSha256: sha256Of(A_BEFORE),
          afterSha256: sha256Of(A_AFTER),
          addedLines: 1,
          removedLines: 0,
        },
      ]),
      primitives,
      { ...deps(), store: failingStore },
    );
    expect(outcome.status).toBe("failed");
    if (outcome.status === "failed") {
      expect(outcome.message).toContain("checkpoint capacity refused");
    }
    expect(primitives.files.get("a.gd")).toBe(A_BEFORE);
  });

  it("recovery restores every partially applied file and returns apply_failed_recovered", async () => {
    const primitives = new InMemoryPrimitives({ "a.gd": A_BEFORE, "b.gd": B_BEFORE });
    const originalWriteFile = primitives.writeFile.bind(primitives);
    primitives.writeFile = async (path: string, content: string): Promise<void> => {
      if (path === "b.gd") {
        throw new Error("injected write failure for b.gd");
      }
      await originalWriteFile(path, content);
    };
    const outcome = await applyChangeSetProtocol(
      applyRequest([
        {
          path: "a.gd",
          operation: "update",
          expectedSha256: sha256Of(A_BEFORE),
          content: A_AFTER,
          beforeSha256: sha256Of(A_BEFORE),
          afterSha256: sha256Of(A_AFTER),
          addedLines: 2,
          removedLines: 0,
        },
        {
          path: "b.gd",
          operation: "update",
          expectedSha256: sha256Of(B_BEFORE),
          content: "changed\n",
          beforeSha256: sha256Of(B_BEFORE),
          afterSha256: sha256Of("changed\n"),
          addedLines: 1,
          removedLines: 0,
        },
      ]),
      primitives,
      deps(),
    );
    expect(outcome.status).toBe("apply_failed_recovered");
    if (outcome.status === "apply_failed_recovered") {
      expect(outcome.checkpointIds).toHaveLength(2);
    }
    expect(primitives.files.get("a.gd")).toBe(A_BEFORE);
    expect(primitives.files.get("b.gd")).toBe(B_BEFORE);
  });

  it("reports apply_failed_uncertain when the partial result changed externally", async () => {
    const primitives = new InMemoryPrimitives({ "a.gd": A_BEFORE, "b.gd": B_BEFORE });
    const external = new InMemoryPrimitives();
    // Every "a.gd" read resolves through the external store, which stays
    // empty until the mid-application race lands (when "b.gd" fails).
    const originalReadFile = primitives.readFile.bind(primitives);
    primitives.readFile = async (path: string) => {
      if (path === "a.gd" && external.files.has("a.gd")) {
        return external.readFile("a.gd");
      }
      return originalReadFile(path);
    };
    const originalWriteFile = primitives.writeFile.bind(primitives);
    primitives.writeFile = async (path: string, content: string): Promise<void> => {
      if (path === "b.gd") {
        await external.writeFile("a.gd", "external-change");
        throw new Error(`injected write failure for ${path}`);
      }
      await originalWriteFile(path, content);
    };
    const outcome = await applyChangeSetProtocol(
      applyRequest([
        {
          path: "a.gd",
          operation: "update",
          expectedSha256: sha256Of(A_BEFORE),
          content: A_AFTER,
          beforeSha256: sha256Of(A_BEFORE),
          afterSha256: sha256Of(A_AFTER),
          addedLines: 2,
          removedLines: 0,
        },
        {
          path: "b.gd",
          operation: "update",
          expectedSha256: sha256Of(B_BEFORE),
          content: "changed\n",
          beforeSha256: sha256Of(B_BEFORE),
          afterSha256: sha256Of("changed\n"),
          addedLines: 1,
          removedLines: 0,
        },
      ]),
      primitives,
      deps(),
    );
    expect(outcome.status).toBe("apply_failed_uncertain");
    // The external change is preserved, never overwritten by recovery.
    expect(await primitives.readFile("a.gd")).toEqual({
      exists: true,
      sha256: sha256Of("external-change"),
    });
    if (outcome.status === "apply_failed_uncertain") {
      expect(outcome.checkpointIds).toHaveLength(2);
    }
  });

  it("holds the mutation lock across the whole apply", async () => {
    const primitives = new InMemoryPrimitives({ "a.gd": A_BEFORE });
    let held = false;
    const observed: string[] = [];
    const lock = {
      acquire(): Promise<() => void> {
        expect(held).toBe(false);
        held = true;
        observed.push("acquired");
        return Promise.resolve(() => {
          held = false;
          observed.push("released");
        });
      },
    };
    const outcome = await applyChangeSetProtocol(
      applyRequest([
        {
          path: "a.gd",
          operation: "update",
          expectedSha256: sha256Of(A_BEFORE),
          content: A_AFTER,
          beforeSha256: sha256Of(A_BEFORE),
          afterSha256: sha256Of(A_AFTER),
          addedLines: 1,
          removedLines: 0,
        },
      ]),
      primitives,
      { ...deps(), lock },
    );
    expect(outcome.status).toBe("applied");
    expect(observed).toEqual(["acquired", "released"]);
    expect(held).toBe(false);
  });
});

function createTestLock(): { acquire(signal?: AbortSignal): Promise<() => void> } {
  return {
    acquire(): Promise<() => void> {
      return Promise.resolve(() => undefined);
    },
  };
}
