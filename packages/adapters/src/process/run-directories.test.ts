import { describe, expect, it } from "vitest";
import { mkdir, readdir, symlink, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { createRunDirectoryProvider } from "./run-directories.js";
import { createTempWorkspace, SYMLINKS_SUPPORTED } from "../tools/workspace/workspace-fixtures.js";

describe("createRunDirectoryProvider", () => {
  it("creates sandbox-private home, temp, cache, and npmrc beneath the runs root", async () => {
    const workspace = await createTempWorkspace();
    const runsRoot = join(tmpdir(), `solaris-runs-${Date.now()}-${Math.random()}`);
    try {
      const provider = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      const paths = await provider.create();
      expect(paths.home).toContain("home");
      expect(paths.temp).toContain("tmp");
      expect(paths.npmCache).toContain("npm-cache");
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
    const runsRoot = join(tmpdir(), `solaris-runs-${Date.now()}-${Math.random()}`);
    try {
      const providerA = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      const providerB = createRunDirectoryProvider({ workspaceRoot: workspace.root, runsRoot });
      const pathsA = await providerA.create();
      const pathsB = await providerB.create();
      expect(pathsA.home.split("\\").at(-3)).toBe(pathsB.home.split("\\").at(-3));
      expect(pathsA.runId).not.toBe(pathsB.runId);
      await providerA.remove(pathsA.runId);
      await providerB.remove(pathsB.runId);
    } finally {
      await workspace.cleanup();
    }
  });

  it("refuses cleanup for invalid run ids", async () => {
    const workspace = await createTempWorkspace();
    const runsRoot = join(tmpdir(), `solaris-runs-${Date.now()}-${Math.random()}`);
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
    const runsRoot = join(tmpdir(), `solaris-runs-${Date.now()}-${Math.random()}`);
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
    const realRoot = join(tmpdir(), `solaris-runs-real-${Date.now()}-${Math.random()}`);
    const linkedRoot = join(tmpdir(), `solaris-runs-link-${Date.now()}-${Math.random()}`);
    try {
      await mkdir(realRoot, { recursive: true });
      await symlink(realRoot, linkedRoot, "dir");
      const provider = createRunDirectoryProvider({
        workspaceRoot: workspace.root,
        runsRoot: linkedRoot,
      });
      await expect(provider.create()).rejects.toThrow();
    } finally {
      await workspace.cleanup();
    }
  });

  it("removes the complete run directory", async () => {
    const workspace = await createTempWorkspace();
    const runsRoot = join(tmpdir(), `solaris-runs-${Date.now()}-${Math.random()}`);
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
