import { describe, expect, it } from "vitest";
import { mkdir, readdir, symlink, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { createRunDirectoryProvider } from "./run-directories.js";
import { createTempWorkspace, SYMLINKS_SUPPORTED } from "../tools/workspace/workspace-fixtures.js";

function uniqueRunsRoot(): string {
  return join(tmpdir(), `solaris-runs-${Date.now()}-${Math.random()}`);
}

describe("createRunDirectoryProvider", () => {
  it("creates sandbox-private home, temp, cache, npmrc, and script cache beneath the runs root", async () => {
    const workspace = await createTempWorkspace();
    const runsRoot = uniqueRunsRoot();
    try {
      const provider = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      const paths = await provider.create();
      expect(paths.home).toContain("home");
      expect(paths.temp).toContain("tmp");
      expect(paths.npmCache).toContain("npm-cache");
      expect(paths.scriptCache).toContain("script-cache");
      expect(paths.npmUserConfig.endsWith("npmrc")).toBe(true);
      expect(paths.runId).toBeTruthy();
      const homeEntries = await readdir(paths.home);
      expect(homeEntries).toEqual([]);
      const npmrc = await import("node:fs/promises").then((fs) =>
        fs.readFile(paths.npmUserConfig, "utf8"),
      );
      expect(npmrc).toBe("");
      await provider.remove(paths.runId);
      await expect(import("node:fs/promises").then((fs) => fs.stat(paths.home))).rejects.toThrow();
    } finally {
      await workspace.cleanup();
    }
  });

  it("uses a deterministic workspace fingerprint", async () => {
    const workspace = await createTempWorkspace();
    const runsRoot = uniqueRunsRoot();
    try {
      const providerA = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      const providerB = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      const pathsA = await providerA.create();
      const pathsB = await providerB.create();
      expect(pathsA.home.split("\\").at(-3)).toBe(pathsB.home.split("\\").at(-3));
      expect(pathsA.root).not.toBe(pathsB.root);
      expect(pathsA.runId).not.toBe(pathsB.runId);
      await providerA.remove(pathsA.runId);
      await providerB.remove(pathsB.runId);
    } finally {
      await workspace.cleanup();
    }
  });

  it("refuses cleanup for invalid run ids", async () => {
    const workspace = await createTempWorkspace();
    const runsRoot = uniqueRunsRoot();
    try {
      const provider = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      const outcome = await provider.remove("../../evil");
      expect(outcome.ok).toBe(false);
    } finally {
      await workspace.cleanup();
    }
  });

  it("never deletes outside the verified run root", async () => {
    if (!SYMLINKS_SUPPORTED) {
      return;
    }
    const workspace = await createTempWorkspace();
    const runsRoot = uniqueRunsRoot();
    try {
      const provider = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      const paths = await provider.create();
      const victim = join(runsRoot, "victim.txt");
      await writeFile(victim, "keep me");
      await provider.remove(paths.runId);
      const content = await import("node:fs/promises").then((fs) => fs.readFile(victim, "utf8"));
      expect(content).toBe("keep me");
    } finally {
      await workspace.cleanup();
    }
  });

  it("rejects a run root that resolves through a symbolic link", async () => {
    if (!SYMLINKS_SUPPORTED) {
      return;
    }
    const workspace = await createTempWorkspace();
    const realRoot = uniqueRunsRoot();
    const linkedRoot = uniqueRunsRoot();
    try {
      await mkdir(realRoot, { recursive: true });
      await symlink(realRoot, linkedRoot, "dir");
      const provider = createRunDirectoryProvider({
        workspaceRoot: workspace.root,
        runsRoot: linkedRoot,
      });
      await expect(provider.create()).rejects.toThrow();
      // Nothing was created inside the link target.
      const entries = await readdir(realRoot);
      expect(entries).toEqual([]);
    } finally {
      await workspace.cleanup();
    }
  });

  it("rejects a fingerprint directory that is a symbolic link before creating anything", async () => {
    if (!SYMLINKS_SUPPORTED) {
      return;
    }
    const workspace = await createTempWorkspace();
    const runsRoot = uniqueRunsRoot();
    const outside = join(tmpdir(), `solaris-fingerprint-outside-${Date.now()}-${Math.random()}`);
    try {
      const provider = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      const first = await provider.create();
      const fingerprint = first.home.split("\\").at(-3) as string;
      await provider.remove(first.runId);
      // Plant a symlink at the predictable fingerprint path.
      const { rm } = await import("node:fs/promises");
      await rm(join(runsRoot, fingerprint), { recursive: true, force: true });
      await mkdir(outside, { recursive: true });
      await symlink(outside, join(runsRoot, fingerprint), "dir");
      await expect(provider.create()).rejects.toThrow();
      const outsideEntries = await readdir(outside);
      expect(outsideEntries).toEqual([]);
      const runsEntries = await readdir(runsRoot);
      expect(runsEntries).toEqual([fingerprint]);
    } finally {
      await workspace.cleanup();
    }
  });

  it("rejects a runs root inside the project workspace", async () => {
    const workspace = await createTempWorkspace();
    const runsRoot = join(workspace.root, ".solaris-runs");
    try {
      const provider = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      await expect(provider.create()).rejects.toThrow();
    } finally {
      await workspace.cleanup();
    }
  });

  it("refuses cleanup when the run directory resolves through a link", async () => {
    if (!SYMLINKS_SUPPORTED) {
      return;
    }
    const workspace = await createTempWorkspace();
    const runsRoot = uniqueRunsRoot();
    const outside = join(tmpdir(), `solaris-run-link-outside-${Date.now()}-${Math.random()}`);
    try {
      const provider = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      const paths = await provider.create();
      const { rm } = await import("node:fs/promises");
      await rm(paths.root, { recursive: true, force: true });
      await mkdir(outside, { recursive: true });
      await writeFile(join(outside, "victim.txt"), "keep me");
      await symlink(outside, paths.root, "dir");
      const outcome = await provider.remove(paths.runId);
      expect(outcome.ok).toBe(false);
      if (!outcome.ok) {
        expect(outcome.message).toContain("refused");
      }
      const content = await import("node:fs/promises").then((fs) =>
        fs.readFile(join(outside, "victim.txt"), "utf8"),
      );
      expect(content).toBe("keep me");
    } finally {
      await workspace.cleanup();
    }
  });

  it("removes the complete run directory", async () => {
    const workspace = await createTempWorkspace();
    const runsRoot = uniqueRunsRoot();
    try {
      const provider = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      const paths = await provider.create();
      await writeFile(join(paths.home, "output.txt"), "content");
      const outcome = await provider.remove(paths.runId);
      expect(outcome.ok).toBe(true);
      await expect(import("node:fs/promises").then((fs) => fs.stat(paths.home))).rejects.toThrow();
    } finally {
      await workspace.cleanup();
    }
  });
});
