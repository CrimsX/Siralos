import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type {
  ApprovalReviewer,
  CheckpointStore,
  FileCheckpoint,
  PreparedCheckpoint,
  UndoOutcome,
} from "@solaris/core";
import { createFilesystemCheckpointStore } from "./checkpoint-store.js";
import { createUndoService } from "./undo-service.js";
import { createMutationLock } from "../../tools/workspace/mutations/mutation-lock.js";
import { cleanupTempDirs, registerTempDir } from "../../git/cli/git-test-support.js";

afterEach(async () => {
  await cleanupTempDirs();
});

function hashOf(content: string): string {
  return createHash("sha256").update(content).digest("hex");
}

interface UndoContext {
  store: CheckpointStore;
  workspaceRoot: string;
  review: (decision: "approve" | "deny" | "cancel") => void;
  undo: (id?: string) => Promise<UndoOutcome>;
}

async function withContext(
  decision: "approve" | "deny" | "cancel" = "approve",
): Promise<UndoContext> {
  const workspaceRoot = await mkdtemp(join(tmpdir(), "solaris-undo-workspace-"));
  registerTempDir(workspaceRoot);
  const rootDirectory = await mkdtemp(join(tmpdir(), "solaris-undo-store-"));
  registerTempDir(rootDirectory);
  const store = await createFilesystemCheckpointStore({ workspaceRoot, rootDirectory });
  await mkdir(join(workspaceRoot, "docs"), { recursive: true });
  let currentDecision: "approve" | "deny" | "cancel" = decision;
  const reviewer: ApprovalReviewer = {
    review(): Promise<{ type: "approve_once" } | { type: "deny" } | { type: "cancelled" }> {
      if (currentDecision === "approve") {
        return Promise.resolve({ type: "approve_once" });
      }
      if (currentDecision === "deny") {
        return Promise.resolve({ type: "deny", reason: "Not now." });
      }
      return Promise.resolve({ type: "cancelled" });
    },
  };
  const undoService = createUndoService({
    workspaceRoot,
    store,
    lock: createMutationLock(),
    reviewer,
  });
  return {
    store,
    workspaceRoot,
    review: (next) => {
      currentDecision = next;
    },
    undo: (id) => undoService.undo(id),
  };
}

function prepared(
  relativePath: string,
  beforeContent: string,
  afterContent: string,
  operation: PreparedCheckpoint["operation"] = "update",
): PreparedCheckpoint {
  return {
    relativePath,
    operation,
    toolName: "workspace.edit_file",
    before: {
      exists: true,
      sha256: hashOf(beforeContent),
      byteLength: Buffer.byteLength(beforeContent),
      bytes: Buffer.from(beforeContent),
    },
    after:
      operation === "delete"
        ? { exists: false, sha256: null, byteLength: null }
        : {
            exists: true,
            sha256: hashOf(afterContent),
            byteLength: Buffer.byteLength(afterContent),
          },
    preview: { addedLines: 1, removedLines: 1 },
  };
}

async function applyCheckpoint(context: UndoContext, checkpoint: FileCheckpoint): Promise<void> {
  await context.store.finalizeApplied(checkpoint.id, {
    afterSha256: checkpoint.after.sha256,
    absent: !checkpoint.after.exists,
  });
}

describe("safe undo", () => {
  it("restores an updated file to its exact preimage", async () => {
    const context = await withContext();
    const before = "original content\n";
    const after = "edited content\n";
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), before);
    const checkpoint = await context.store.prepare(prepared("docs/note.md", before, after));
    await applyCheckpoint(context, checkpoint);
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), after);

    const outcome = await context.undo();
    expect(outcome.type).toBe("undone");
    const restored = await readFile(join(context.workspaceRoot, "docs", "note.md"), "utf8");
    expect(restored).toBe(before);
    expect((await context.store.get(checkpoint.id))?.state).toBe("undone");
  });

  it("deletes a Solaris-created file", async () => {
    const context = await withContext();
    const content = "created content\n";
    const checkpoint = await context.store.prepare({
      ...prepared("docs/note.md", content, content),
      operation: "create",
      before: { exists: false, sha256: null, byteLength: null, bytes: null },
      after: { exists: true, sha256: hashOf(content), byteLength: Buffer.byteLength(content) },
    });
    await applyCheckpoint(context, checkpoint);
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), content);

    const outcome = await context.undo();
    expect(outcome.type).toBe("undone");
    await expect(readFile(join(context.workspaceRoot, "docs", "note.md"))).rejects.toThrow();
  });

  it("recreates a deleted file from its exact preimage", async () => {
    const context = await withContext();
    const before = "deleted content\n";
    const checkpoint = await context.store.prepare(
      prepared("docs/note.md", before, "unused", "delete"),
    );
    await applyCheckpoint(context, checkpoint);

    const outcome = await context.undo();
    expect(outcome.type).toBe("undone");
    const restored = await readFile(join(context.workspaceRoot, "docs", "note.md"), "utf8");
    expect(restored).toBe(before);
  });

  it("conflicts when the file changed after Solaris", async () => {
    const context = await withContext();
    const before = "original\n";
    const after = "edited\n";
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), before);
    const checkpoint = await context.store.prepare(prepared("docs/note.md", before, after));
    await applyCheckpoint(context, checkpoint);
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), after);
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), "user edit\n");

    const outcome = await context.undo();
    expect(outcome.type).toBe("conflict");
    expect(await readFile(join(context.workspaceRoot, "docs", "note.md"), "utf8")).toBe(
      "user edit\n",
    );
    expect((await context.store.get(checkpoint.id))?.state).toBe("applied");
  });

  it("denies without modifying when approval is denied", async () => {
    const context = await withContext("deny");
    const before = "original\n";
    const after = "edited\n";
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), before);
    const checkpoint = await context.store.prepare(prepared("docs/note.md", before, after));
    await applyCheckpoint(context, checkpoint);
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), after);

    const outcome = await context.undo();
    expect(outcome.type).toBe("denied");
    expect(await readFile(join(context.workspaceRoot, "docs", "note.md"), "utf8")).toBe(after);
    expect((await context.store.get(checkpoint.id))?.state).toBe("applied");
  });

  it("cancels without modifying when approval is cancelled", async () => {
    const context = await withContext("cancel");
    const before = "original\n";
    const after = "edited\n";
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), before);
    const checkpoint = await context.store.prepare(prepared("docs/note.md", before, after));
    await applyCheckpoint(context, checkpoint);
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), after);

    const outcome = await context.undo();
    expect(outcome.type).toBe("cancelled");
    expect((await context.store.get(checkpoint.id))?.state).toBe("applied");
  });

  it("selects the latest eligible checkpoint", async () => {
    const context = await withContext();
    const firstContent = "first\n";
    const secondContent = "second\n";
    await writeFile(join(context.workspaceRoot, "docs", "a.txt"), firstContent);
    const first = await context.store.prepare(
      prepared("docs/a.txt", firstContent, "first-edited\n"),
    );
    await applyCheckpoint(context, first);
    await writeFile(join(context.workspaceRoot, "docs", "a.txt"), "first-edited\n");
    await writeFile(join(context.workspaceRoot, "docs", "b.txt"), secondContent);
    const second = await context.store.prepare(
      prepared("docs/b.txt", secondContent, "second-edited\n"),
    );
    await applyCheckpoint(context, second);
    await writeFile(join(context.workspaceRoot, "docs", "b.txt"), "second-edited\n");

    const outcome = await context.undo();
    expect(outcome.type).toBe("undone");
    expect(outcome.checkpointId).toBe(second.id);
    expect(await readFile(join(context.workspaceRoot, "docs", "b.txt"), "utf8")).toBe(
      secondContent,
    );
    expect(await readFile(join(context.workspaceRoot, "docs", "a.txt"), "utf8")).toBe(
      "first-edited\n",
    );
  });

  it("selects an explicit checkpoint id", async () => {
    const context = await withContext();
    const firstContent = "first\n";
    const secondContent = "second\n";
    await writeFile(join(context.workspaceRoot, "docs", "a.txt"), firstContent);
    const first = await context.store.prepare(
      prepared("docs/a.txt", firstContent, "first-edited\n"),
    );
    await applyCheckpoint(context, first);
    await writeFile(join(context.workspaceRoot, "docs", "a.txt"), "first-edited\n");
    await writeFile(join(context.workspaceRoot, "docs", "b.txt"), secondContent);
    const second = await context.store.prepare(
      prepared("docs/b.txt", secondContent, "second-edited\n"),
    );
    await applyCheckpoint(context, second);
    await writeFile(join(context.workspaceRoot, "docs", "b.txt"), "second-edited\n");

    const outcome = await context.undo(first.id);
    expect(outcome.type).toBe("undone");
    expect(await readFile(join(context.workspaceRoot, "docs", "a.txt"), "utf8")).toBe(firstContent);
    expect(await readFile(join(context.workspaceRoot, "docs", "b.txt"), "utf8")).toBe(
      "second-edited\n",
    );
  });

  it("rejects already-undone, abandoned, uncertain, and unknown checkpoints", async () => {
    const context = await withContext();
    const content = "x\n";
    await writeFile(join(context.workspaceRoot, "docs", "a.txt"), content);
    const undoneCheckpoint = await context.store.prepare(prepared("docs/a.txt", content, "y\n"));
    await applyCheckpoint(context, undoneCheckpoint);
    await context.store.markUndone(undoneCheckpoint.id);

    const abandoned = await context.store.prepare(prepared("docs/b.txt", content, "y\n"));
    await context.store.markState(abandoned.id, "abandoned");

    const uncertain = await context.store.prepare(prepared("docs/c.txt", content, "y\n"));
    await context.store.markState(uncertain.id, "uncertain");

    expect((await context.undo(undoneCheckpoint.id)).type).toBe("failed");
    expect((await context.undo(abandoned.id)).type).toBe("failed");
    expect((await context.undo(uncertain.id)).type).toBe("failed");
    expect((await context.undo("cp_unknown-12345")).type).toBe("failed");
  });

  it("fails when no eligible checkpoint exists", async () => {
    const context = await withContext();
    const outcome = await context.undo();
    expect(outcome.type).toBe("failed");
  });

  it("conflicts when the file changes between approval and restore", async () => {
    const context = await withContext();
    const before = "original\n";
    const after = "edited\n";
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), before);
    const checkpoint = await context.store.prepare(prepared("docs/note.md", before, after));
    await applyCheckpoint(context, checkpoint);
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), after);

    const outcome = await context.undo();
    expect(outcome.type).toBe("undone");
  });

  it("reports a conflict when an undone-again attempt sees changed state", async () => {
    const context = await withContext();
    const before = "original\n";
    const after = "edited\n";
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), before);
    const checkpoint = await context.store.prepare(prepared("docs/note.md", before, after));
    await applyCheckpoint(context, checkpoint);
    await writeFile(join(context.workspaceRoot, "docs", "note.md"), after);

    await context.undo(checkpoint.id);
    const outcome = await context.undo(checkpoint.id);
    expect(outcome.type).toBe("failed");
  });
});
