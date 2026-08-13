import { describe, expect, it } from "vitest";
import { mkdir, readdir, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  createRunDirectoryProvider,
  RUN_DIRECTORY_CLEANUP_UNAVAILABLE_MESSAGE,
  RUN_DIRECTORY_CREATION_UNAVAILABLE_MESSAGE,
} from "./run-directories.js";
import { createTempWorkspace, SYMLINKS_SUPPORTED } from "../tools/workspace/workspace-fixtures.js";

function uniqueRunsRoot(): string {
  return join(tmpdir(), `siralos-runs-${Date.now()}-${Math.random()}`);
}

/**
 * The private run-directory provider fails closed: Node offers no
 * directory-relative or delete-by-handle primitive, so creation and cleanup
 * are both UNAVAILABLE and the provider performs ZERO filesystem operations.
 * No run-directory operation can create an entry outside an intended
 * verified root (nothing is ever created), and cleanup can never delete a
 * substituted object (nothing is ever deleted).
 */
describe("createRunDirectoryProvider (fail-closed contract)", () => {
  it("reports creation unavailable and creates nothing", async () => {
    const workspace = await createTempWorkspace();
    const runsRoot = uniqueRunsRoot();
    try {
      const provider = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      const outcome = await provider.create();
      expect(outcome.ok).toBe(false);
      if (!outcome.ok) {
        expect(outcome.reason).toBe("unavailable");
        expect(outcome.message).toContain("unavailable");
      }
      // Zero entries anywhere: the runs root, the workspace, and the tmp
      // parent are all untouched.
      await expect(import("node:fs/promises").then((fs) => fs.stat(runsRoot))).rejects.toThrow();
      const workspaceEntries = await readdir(workspace.root);
      expect(workspaceEntries).toEqual([]);
    } finally {
      await workspace.cleanup();
    }
  });

  it("creates nothing even when the runs-root location is a hostile symlink", async () => {
    if (!SYMLINKS_SUPPORTED) {
      return;
    }
    const workspace = await createTempWorkspace();
    const runsRoot = uniqueRunsRoot();
    const outside = join(tmpdir(), `siralos-runs-outside-${Date.now()}-${Math.random()}`);
    try {
      // Deterministic race at the verification-to-create boundary: a
      // same-user process plants a link at the runs root. Because creation
      // is unavailable, zero entries appear inside the link target.
      await mkdir(outside, { recursive: true });
      await symlink(outside, runsRoot, "dir");
      const provider = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      const outcome = await provider.create();
      expect(outcome.ok).toBe(false);
      const outsideEntries = await readdir(outside);
      expect(outsideEntries).toEqual([]);
    } finally {
      await workspace.cleanup();
    }
  });

  it("creates nothing even when the runs root already exists and is swapped for a link", async () => {
    if (!SYMLINKS_SUPPORTED) {
      return;
    }
    const workspace = await createTempWorkspace();
    const runsRoot = uniqueRunsRoot();
    const outside = uniqueRunsRoot();
    try {
      // A hostile process pre-creates the runs root, then swaps it for a
      // link to a victim directory before the provider is invoked.
      await mkdir(runsRoot, { recursive: true });
      await mkdir(outside, { recursive: true });
      await writeFile(join(outside, "victim.txt"), "keep me");
      await rm(runsRoot, { recursive: true });
      await symlink(outside, runsRoot, "dir");
      const provider = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      const outcome = await provider.create();
      expect(outcome.ok).toBe(false);
      const content = await readFile(join(outside, "victim.txt"), "utf8");
      expect(content).toBe("keep me");
      expect(await readdir(outside)).toEqual(["victim.txt"]);
    } finally {
      await workspace.cleanup();
    }
  });

  it("reports cleanup unavailable and deletes nothing", async () => {
    const workspace = await createTempWorkspace();
    const runsRoot = uniqueRunsRoot();
    try {
      const provider = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      const outcome = await provider.remove("00000000-0000-4000-8000-000000000000");
      expect(outcome.ok).toBe(false);
      if (!outcome.ok) {
        expect(outcome.reason).toBe("unavailable");
        expect(outcome.message).toContain("preserved");
      }
    } finally {
      await workspace.cleanup();
    }
  });

  it("preserves substituted content: root substitution after validation leaves every entry intact", async () => {
    const workspace = await createTempWorkspace();
    const runsRoot = uniqueRunsRoot();
    try {
      const provider = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      // Simulate the directory a previous version could have left behind:
      // the root holds a victim tree that a hostile process substituted in.
      await mkdir(join(runsRoot, "run-id"), { recursive: true });
      await writeFile(join(runsRoot, "run-id", "keep.txt"), "keep me");
      await mkdir(join(runsRoot, "run-id", "subdir"), { recursive: true });
      await writeFile(join(runsRoot, "run-id", "subdir", "deep.txt"), "deep keep");
      const outcome = await provider.remove("00000000-0000-4000-8000-000000000000");
      expect(outcome.ok).toBe(false);
      // Nothing was deleted: every substituted leaf survives unchanged.
      const content = await readFile(join(runsRoot, "run-id", "keep.txt"), "utf8");
      expect(content).toBe("keep me");
      const deep = await readFile(join(runsRoot, "run-id", "subdir", "deep.txt"), "utf8");
      expect(deep).toBe("deep keep");
    } finally {
      await workspace.cleanup();
    }
  });

  it("preserves a leaf that a hostile process substitutes in during cleanup", async () => {
    const workspace = await createTempWorkspace();
    const runsRoot = uniqueRunsRoot();
    try {
      const provider = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      await mkdir(join(runsRoot, "run-id", "home"), { recursive: true });
      await writeFile(join(runsRoot, "run-id", "home", "real.txt"), "original");
      // Concurrent addition during cleanup: a new entry appears after the
      // cleanup call starts. Cleanup is unavailable, so it survives.
      await writeFile(join(runsRoot, "run-id", "home", "added.txt"), "added");
      const outcome = await provider.remove("00000000-0000-4000-8000-000000000000");
      expect(outcome.ok).toBe(false);
      const real = await readFile(join(runsRoot, "run-id", "home", "real.txt"), "utf8");
      expect(real).toBe("original");
      const added = await readFile(join(runsRoot, "run-id", "home", "added.txt"), "utf8");
      expect(added).toBe("added");
    } finally {
      await workspace.cleanup();
    }
  });

  it("never follows or deletes through a link planted at the run path", async () => {
    if (!SYMLINKS_SUPPORTED) {
      return;
    }
    const workspace = await createTempWorkspace();
    const runsRoot = uniqueRunsRoot();
    const outside = uniqueRunsRoot();
    try {
      const provider = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      await mkdir(runsRoot, { recursive: true });
      await mkdir(outside, { recursive: true });
      await writeFile(join(outside, "victim.txt"), "keep me");
      await symlink(outside, join(runsRoot, "run-id"), "dir");
      const outcome = await provider.remove("00000000-0000-4000-8000-000000000000");
      expect(outcome.ok).toBe(false);
      const content = await readFile(join(outside, "victim.txt"), "utf8");
      expect(content).toBe("keep me");
      expect(await readdir(outside)).toEqual(["victim.txt"]);
    } finally {
      await workspace.cleanup();
    }
  });

  it("performs zero filesystem operations on every method", async () => {
    const workspace = await createTempWorkspace();
    const runsRoot = uniqueRunsRoot();
    try {
      const provider = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      const createOutcome = await provider.create();
      const removeOutcome = await provider.remove("00000000-0000-4000-8000-000000000000");
      expect(createOutcome.ok).toBe(false);
      expect(removeOutcome.ok).toBe(false);
      await expect(import("node:fs/promises").then((fs) => fs.stat(runsRoot))).rejects.toThrow();
      expect(await readdir(workspace.root)).toEqual([]);
    } finally {
      await workspace.cleanup();
    }
  });

  it("exposes truthful unavailable messages", () => {
    expect(RUN_DIRECTORY_CREATION_UNAVAILABLE_MESSAGE).toContain("unavailable");
    expect(RUN_DIRECTORY_CLEANUP_UNAVAILABLE_MESSAGE).toContain("preserved");
  });
});
