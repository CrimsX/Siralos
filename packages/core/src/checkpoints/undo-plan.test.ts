import { describe, expect, it } from "vitest";
import {
  createDefaultPolicy,
  DEVELOP_OFFLINE_PROFILE,
  evaluatePermission,
  INSPECT_PROFILE,
  planUndo,
  type FileCheckpoint,
  type WorkspaceFileState,
} from "../index.js";

function checkpoint(overrides: Partial<FileCheckpoint> = {}): FileCheckpoint {
  return {
    version: 1,
    id: "cp-test",
    workspaceFingerprint: "fingerprint",
    relativePath: "README.md",
    operation: "update",
    toolName: "workspace.edit_file",
    createdAt: "2026-08-06T18:00:00.000Z",
    state: "applied",
    before: { exists: true, sha256: "A", byteLength: 10 },
    after: { exists: true, sha256: "B", byteLength: 12 },
    preview: { addedLines: 1, removedLines: 1 },
    ...overrides,
  };
}

function state(exists: boolean, sha256: string | null): WorkspaceFileState {
  return { exists, sha256 };
}

describe("git.inspect capability", () => {
  it("allows git inspection under both profiles", () => {
    expect(
      evaluatePermission("git.inspect", createDefaultPolicy("inspect"), INSPECT_PROFILE),
    ).toEqual({
      decision: "allow",
    });
    expect(
      evaluatePermission(
        "git.inspect",
        createDefaultPolicy("develop-offline"),
        DEVELOP_OFFLINE_PROFILE,
      ),
    ).toEqual({ decision: "allow" });
  });

  it("fails closed when the git rule is missing", () => {
    const rules = { ...createDefaultPolicy("inspect").rules } as Record<string, unknown>;
    delete rules["git.inspect"];
    const result = evaluatePermission("git.inspect", { rules: rules as never }, INSPECT_PROFILE);
    expect(result).toMatchObject({ decision: "deny" });
  });
});

describe("planUndo", () => {
  it("plans a delete for a created file whose hash still matches", () => {
    const created = checkpoint({
      operation: "create",
      before: { exists: false, sha256: null, byteLength: null },
      after: { exists: true, sha256: "A", byteLength: 5 },
    });
    expect(planUndo(created, state(true, "A"))).toEqual({ decision: "ready", action: "delete" });
  });

  it("plans a restore for an updated file whose hash still matches", () => {
    expect(planUndo(checkpoint(), state(true, "B"))).toEqual({
      decision: "ready",
      action: "restore",
    });
  });

  it("plans a create for a deleted file that is still absent", () => {
    const deleted = checkpoint({
      operation: "delete",
      before: { exists: true, sha256: "A", byteLength: 5 },
      after: { exists: false, sha256: null, byteLength: null },
    });
    expect(planUndo(deleted, state(false, null))).toEqual({
      decision: "ready",
      action: "create",
    });
  });

  it("conflicts when an updated file changed after Solaris", () => {
    expect(planUndo(checkpoint(), state(true, "C"))).toMatchObject({ decision: "conflict" });
  });

  it("conflicts when a created file was modified", () => {
    const created = checkpoint({
      operation: "create",
      before: { exists: false, sha256: null, byteLength: null },
      after: { exists: true, sha256: "A", byteLength: 5 },
    });
    expect(planUndo(created, state(true, "D"))).toMatchObject({ decision: "conflict" });
  });

  it("conflicts when a deleted file reappeared", () => {
    const deleted = checkpoint({
      operation: "delete",
      before: { exists: true, sha256: "A", byteLength: 5 },
      after: { exists: false, sha256: null, byteLength: null },
    });
    expect(planUndo(deleted, state(true, "A"))).toMatchObject({ decision: "conflict" });
  });

  it("conflicts when an updated file is missing", () => {
    expect(planUndo(checkpoint(), state(false, null))).toMatchObject({ decision: "conflict" });
  });
});
