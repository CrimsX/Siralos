import { afterEach, describe, expect, it } from "vitest";
import { mkdir } from "node:fs/promises";
import path from "node:path";
import {
  isProtectedWriteTarget,
  resolveCreateTarget,
  resolveMutationTarget,
} from "./mutation-paths.js";
import {
  createSymlink,
  createTempWorkspace,
  SYMLINKS_SUPPORTED,
  writeFixtureFiles,
  type TempWorkspace,
} from "../workspace-fixtures.js";

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

describe("resolveCreateTarget", () => {
  it("resolves a valid new target with an existing parent", async () => {
    const workspace = await withWorkspace();
    await mkdir(path.join(workspace.root, "docs"));
    const resolved = await resolveCreateTarget(workspace.root, "docs/example.md");
    expect(resolved).toMatchObject({
      status: "resolved",
      workspaceRelativePath: "docs/example.md",
    });
  });

  it("rejects a missing parent directory", async () => {
    const workspace = await withWorkspace();
    const resolved = await resolveCreateTarget(workspace.root, "missing/example.md");
    expect(resolved).toMatchObject({ status: "rejected" });
  });

  it("reports an existing target as exists", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "example.md": "x" });
    const resolved = await resolveCreateTarget(workspace.root, "example.md");
    expect(resolved).toMatchObject({ status: "exists" });
  });

  it("rejects protected targets", async () => {
    const workspace = await withWorkspace();
    expect((await resolveCreateTarget(workspace.root, ".env")).status).toBe("rejected");
    expect((await resolveCreateTarget(workspace.root, ".git/config")).status).toBe("rejected");
    expect((await resolveCreateTarget(workspace.root, "keys.pem")).status).toBe("rejected");
    expect((await resolveCreateTarget(workspace.root, ".solaris/state.json")).status).toBe(
      "rejected",
    );
  });

  it("rejects targets outside the workspace", async () => {
    const workspace = await withWorkspace();
    expect((await resolveCreateTarget(workspace.root, "../outside.txt")).status).toBe("rejected");
  });

  it("rejects a symlinked parent", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const workspace = await withWorkspace();
    const outside = await createTempWorkspace();
    workspaces.push(outside);
    await createSymlink(outside.root, path.join(workspace.root, "link-dir"));
    const resolved = await resolveCreateTarget(workspace.root, "link-dir/new.txt");
    expect(resolved).toMatchObject({ status: "rejected" });
    if (resolved.status === "rejected") {
      expect(resolved.message).toContain("symbolic link");
    }
  });
});

describe("resolveMutationTarget", () => {
  it("resolves an existing regular file", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "file.txt": "hello\n" });
    const resolved = await resolveMutationTarget(workspace.root, "file.txt");
    expect(resolved).toMatchObject({ status: "resolved", workspaceRelativePath: "file.txt" });
  });

  it("reports a missing target as missing", async () => {
    const workspace = await withWorkspace();
    const resolved = await resolveMutationTarget(workspace.root, "gone.txt");
    expect(resolved).toMatchObject({ status: "missing" });
  });

  it("rejects directories", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "dir/inner.txt": "x" });
    const resolved = await resolveMutationTarget(workspace.root, "dir");
    expect(resolved).toMatchObject({ status: "rejected" });
    if (resolved.status === "rejected") {
      expect(resolved.message).toContain("not a regular file");
    }
  });

  it("rejects protected targets", async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { ".env": "KEY=value\n" });
    expect((await resolveMutationTarget(workspace.root, ".env")).status).toBe("rejected");
    expect((await resolveMutationTarget(workspace.root, "secret.key")).status).toBe("rejected");
  });

  it("rejects targets outside the workspace", async () => {
    const workspace = await withWorkspace();
    expect((await resolveMutationTarget(workspace.root, "../secret.txt")).status).toBe("rejected");
  });

  it("rejects a symlink target", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const workspace = await withWorkspace();
    await writeFixtureFiles(workspace.root, { "real.txt": "x" });
    await createSymlink(
      path.join(workspace.root, "real.txt"),
      path.join(workspace.root, "link.txt"),
    );
    const resolved = await resolveMutationTarget(workspace.root, "link.txt");
    expect(resolved).toMatchObject({ status: "rejected" });
    if (resolved.status === "rejected") {
      expect(resolved.message).toContain("symbolic link");
    }
  });
});

describe("isProtectedWriteTarget", () => {
  it("matches protected paths component-aware", () => {
    expect(isProtectedWriteTarget(".env")).toBe(true);
    expect(isProtectedWriteTarget(".env.local")).toBe(true);
    expect(isProtectedWriteTarget("docs/.env")).toBe(true);
    expect(isProtectedWriteTarget("cert.pem")).toBe(true);
    expect(isProtectedWriteTarget("nested/private.key")).toBe(true);
    expect(isProtectedWriteTarget(".git/config")).toBe(true);
    expect(isProtectedWriteTarget(".solaris/config.json")).toBe(true);
    expect(isProtectedWriteTarget("packages/pkg/.git/HEAD")).toBe(true);
    expect(isProtectedWriteTarget("README.md")).toBe(false);
    expect(isProtectedWriteTarget("environment.txt")).toBe(false);
    expect(isProtectedWriteTarget("keyboard.md")).toBe(false);
    expect(isProtectedWriteTarget("package.json")).toBe(false);
  });
});
