import { afterEach, describe, expect, it } from "vitest";
import path from "node:path";
import {
  findExcludedComponent,
  resolveWorkspacePath,
  resolveWorkspaceRoot,
} from "./workspace-path.js";
import {
  createSymlink,
  createTempWorkspace,
  SYMLINKS_SUPPORTED,
  writeFixtureFiles,
  type TempWorkspace,
} from "./workspace-fixtures.js";

const workspaces: TempWorkspace[] = [];

async function withWorkspace(): Promise<TempWorkspace> {
  const workspace = await createTempWorkspace();
  workspaces.push(workspace);
  return workspace;
}

afterEach(async () => {
  for (const workspace of workspaces.splice(0)) {
    await workspace.cleanup();
  }
});

describe("resolveWorkspaceRoot", () => {
  it("canonicalizes an existing directory", async () => {
    const workspace = await withWorkspace();
    const resolved = await resolveWorkspaceRoot(workspace.root);
    expect(resolved).toBe(workspace.root);
  });

  it("rejects a missing root", async () => {
    const workspace = await withWorkspace();
    await expect(resolveWorkspaceRoot(path.join(workspace.root, "missing"))).rejects.toThrow(
      "Workspace root is not accessible",
    );
  });
});

describe("resolveWorkspacePath", () => {
  it("resolves a normal relative path to the canonical target", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "packages/core/index.ts": "x" });
    const resolved = await resolveWorkspacePath(workspace.root, "packages/core");
    expect(resolved).toEqual({
      status: "resolved",
      workspaceRelativePath: "packages/core",
      absolutePath: path.join(workspace.root, "packages", "core"),
    });
  });

  it("resolves dot to the workspace root", async () => {
    const workspace = await withWorkspace();
    const resolved = await resolveWorkspacePath(workspace.root, ".");
    expect(resolved).toEqual({
      status: "resolved",
      workspaceRelativePath: ".",
      absolutePath: workspace.root,
    });
  });

  it("accepts OS-native separators", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "packages/core/index.ts": "x" });
    const resolved = await resolveWorkspacePath(workspace.root, `packages${path.sep}core`);
    expect(resolved).toMatchObject({ status: "resolved", workspaceRelativePath: "packages/core" });
  });

  it("rejects parent traversal", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "x" });
    const resolved = await resolveWorkspacePath(workspace.root, "..");
    expect(resolved).toMatchObject({ status: "rejected" });
  });

  it("rejects absolute paths", async () => {
    const workspace = await withWorkspace();
    const absolute = path.join(workspace.root, "a.txt");
    const resolved = await resolveWorkspacePath(workspace.root, absolute);
    expect(resolved).toMatchObject({ status: "rejected" });
  });

  it("rejects drive-letter paths on Windows", async () => {
    const workspace = await withWorkspace();
    const resolved = await resolveWorkspacePath(workspace.root, "C:\\Windows\\System32");
    expect(resolved).toMatchObject({ status: "rejected" });
  });

  it("rejects null-byte paths", async () => {
    const workspace = await withWorkspace();
    const resolved = await resolveWorkspacePath(workspace.root, "a\0b.txt");
    expect(resolved).toMatchObject({ status: "rejected" });
  });

  it("rejects empty paths", async () => {
    const workspace = await withWorkspace();
    const resolved = await resolveWorkspacePath(workspace.root, "");
    expect(resolved).toMatchObject({ status: "rejected" });
  });

  it("rejects prefix-confusion paths", async () => {
    const workspace = await withWorkspace();
    const sibling = path.join(
      path.dirname(workspace.root),
      `${path.basename(workspace.root)}-sibling`,
    );
    await writeFixtureFiles(sibling, { "secret.txt": "secret" });
    workspaces.push({ root: sibling, cleanup: async () => {} });
    const resolved = await resolveWorkspacePath(
      workspace.root,
      `../${path.basename(sibling)}/secret.txt`,
    );
    expect(resolved).toMatchObject({ status: "rejected" });
  });

  it("rejects missing targets", async () => {
    const workspace = await withWorkspace();
    const resolved = await resolveWorkspacePath(workspace.root, "does-not-exist.txt");
    expect(resolved).toMatchObject({ status: "rejected" });
  });

  it("returns workspace-relative paths with forward slashes", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a/b/c.txt": "x" });
    const resolved = await resolveWorkspacePath(workspace.root, "a/b/c.txt");
    expect(resolved.status).toBe("resolved");
    if (resolved.status === "resolved") {
      expect(resolved.workspaceRelativePath).toBe("a/b/c.txt");
      expect(resolved.workspaceRelativePath.startsWith("/")).toBe(false);
      expect(resolved.workspaceRelativePath.startsWith("\\")).toBe(false);
      expect(resolved.workspaceRelativePath.match(/^[A-Za-z]:/)).toBeNull();
    }
  });

  it("normalizes dot segments", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "a.txt": "x" });
    const resolved = await resolveWorkspacePath(workspace.root, "./a.txt");
    expect(resolved).toMatchObject({ status: "resolved", workspaceRelativePath: "a.txt" });
  });

  it("rejects symlinks that escape the workspace", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const workspace = await withWorkspace();
    const outside = await createTempWorkspace();
    workspaces.push(outside);
    await writeFixtureFiles(outside.root, { "secret.txt": "secret" });
    const linkPath = path.join(workspace.root, "escape-link.txt");
    await createSymlink(path.join(outside.root, "secret.txt"), linkPath);
    const resolved = await resolveWorkspacePath(workspace.root, "escape-link.txt");
    expect(resolved).toMatchObject({ status: "rejected" });
  });

  it("allows symlinks that stay inside the workspace", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "target.txt": "x" });
    const linkPath = path.join(workspace.root, "inside-link.txt");
    await createSymlink(path.join(workspace.root, "target.txt"), linkPath);
    const resolved = await resolveWorkspacePath(workspace.root, "inside-link.txt");
    expect(resolved).toMatchObject({ status: "resolved" });
  });
});

describe("findExcludedComponent", () => {
  it("matches excluded directory components only", () => {
    const excluded = ["node_modules", "dist"];
    expect(findExcludedComponent(".", excluded)).toBeNull();
    expect(findExcludedComponent("README.md", excluded)).toBeNull();
    expect(findExcludedComponent("packages/core", excluded)).toBeNull();
    expect(findExcludedComponent("node_modules/pkg", excluded)).toBe("node_modules");
    expect(findExcludedComponent("packages/dist/out", excluded)).toBe("dist");
    expect(findExcludedComponent("dist", excluded)).toBe("dist");
    expect(findExcludedComponent("distro", excluded)).toBeNull();
  });

  it("matches mixed-case excluded components on case-insensitive filesystems", () => {
    const excluded = [".git", "node_modules"];
    expect(findExcludedComponent(".GIT/config", excluded, "darwin")).toBe(".GIT");
    expect(findExcludedComponent("packages/pkg/.Git/HEAD", excluded, "darwin")).toBe(".Git");
    expect(findExcludedComponent("NODE_MODULES/pkg", excluded, "win32")).toBe("NODE_MODULES");
  });

  it("does not match case variants on case-sensitive filesystems", () => {
    const excluded = [".git"];
    expect(findExcludedComponent(".GIT/config", excluded, "linux")).toBeNull();
  });

  it("returns the original component casing so variants are visible", () => {
    const excluded = [".git"];
    expect(findExcludedComponent("packages/.GIT/HEAD", excluded, "darwin")).toBe(".GIT");
  });
});
