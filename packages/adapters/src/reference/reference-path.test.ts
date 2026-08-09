import { afterEach, describe, expect, it } from "vitest";
import path from "node:path";
import {
  assertReferenceRoot,
  isReferenceRootWithin,
  resolveReferencePath,
} from "./reference-path.js";
import {
  createSymlink,
  createTempWorkspace,
  SYMLINKS_SUPPORTED,
  writeFixtureFiles,
  type TempWorkspace,
} from "../tools/workspace/workspace-fixtures.js";

const roots: TempWorkspace[] = [];

async function withRoot(): Promise<TempWorkspace> {
  const root = await createTempWorkspace();
  roots.push(root);
  return root;
}

afterEach(async () => {
  for (const root of roots.splice(0)) {
    await root.cleanup();
  }
});

describe("resolveReferencePath", () => {
  it("resolves a normal relative path to the canonical target", async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, { "packages/core/index.ts": "x" });
    const resolved = await resolveReferencePath(root.root, "packages/core");
    expect(resolved).toEqual({
      ok: true,
      resolved: path.join(root.root, "packages", "core"),
      relative: "packages/core",
    });
  });

  it("resolves dot to the reference root", async () => {
    const root = await withRoot();
    const resolved = await resolveReferencePath(root.root, ".");
    expect(resolved).toEqual({ ok: true, resolved: root.root, relative: "." });
  });

  it("treats backslashes as separators everywhere", async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, { "a/b.txt": "x" });
    const resolved = await resolveReferencePath(root.root, "a\\b.txt");
    expect(resolved).toMatchObject({ ok: true, relative: "a/b.txt" });
  });

  it("normalizes dot segments", async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, { "a/b.txt": "x" });
    const resolved = await resolveReferencePath(root.root, "a/./b.txt");
    expect(resolved).toMatchObject({ ok: true, relative: "a/b.txt" });
  });

  it("accepts nested normalization that stays inside the root", async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, { "b.txt": "x" });
    const resolved = await resolveReferencePath(root.root, "a/../b.txt");
    expect(resolved).toMatchObject({ ok: true, relative: "b.txt" });
  });

  it("rejects parent traversal", async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, { "a.txt": "x" });
    for (const requested of ["..", "../outside", "a/../../outside"]) {
      const resolved = await resolveReferencePath(root.root, requested);
      expect(resolved).toMatchObject({ ok: false });
    }
  });

  it("rejects absolute paths", async () => {
    const root = await withRoot();
    const absolute = path.join(root.root, "a.txt");
    const resolved = await resolveReferencePath(root.root, absolute);
    expect(resolved).toMatchObject({ ok: false });
  });

  it("rejects drive-letter paths", async () => {
    const root = await withRoot();
    const resolved = await resolveReferencePath(root.root, "C:\\Windows\\System32");
    expect(resolved).toMatchObject({ ok: false });
  });

  it("rejects null-byte paths", async () => {
    const root = await withRoot();
    const resolved = await resolveReferencePath(root.root, "a\0b.txt");
    expect(resolved).toMatchObject({ ok: false });
  });

  it("rejects empty paths", async () => {
    const root = await withRoot();
    const resolved = await resolveReferencePath(root.root, "");
    expect(resolved).toMatchObject({ ok: false });
  });

  it("rejects prefix-confusion paths", async () => {
    const root = await withRoot();
    const sibling = path.join(path.dirname(root.root), `${path.basename(root.root)}-sibling`);
    await writeFixtureFiles(sibling, { "secret.txt": "secret" });
    roots.push({ root: sibling, cleanup: async () => {} });
    const resolved = await resolveReferencePath(
      root.root,
      `../${path.basename(sibling)}/secret.txt`,
    );
    expect(resolved).toMatchObject({ ok: false });
  });

  it("accepts missing targets so callers can produce precise not_found results", async () => {
    const root = await withRoot();
    const resolved = await resolveReferencePath(root.root, "does-not-exist.txt");
    // The path is lexically inside the root and no parent is a symlink
    // escape, so the resolver returns the unresolved path; the caller's
    // stat produces the not_found result.
    expect(resolved).toMatchObject({ ok: true });
    expect(resolved.ok && resolved.relative).toBe("does-not-exist.txt");
  });

  it("returns forward-slash relative paths", async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, { "a/b/c.txt": "x" });
    const resolved = await resolveReferencePath(root.root, "a/b/c.txt");
    expect(resolved.ok).toBe(true);
    if (resolved.ok) {
      expect(resolved.relative).toBe("a/b/c.txt");
      expect(resolved.relative.startsWith("/")).toBe(false);
      expect(resolved.relative.startsWith("\\")).toBe(false);
      expect(resolved.relative.match(/^[A-Za-z]:/)).toBeNull();
    }
  });

  it("rejects symlinks that escape the reference root", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const root = await withRoot();
    const outside = await createTempWorkspace();
    roots.push(outside);
    await writeFixtureFiles(outside.root, { "secret.txt": "secret" });
    const linkPath = path.join(root.root, "escape-link.txt");
    await createSymlink(path.join(outside.root, "secret.txt"), linkPath);
    const resolved = await resolveReferencePath(root.root, "escape-link.txt");
    expect(resolved).toMatchObject({ ok: false });
  });

  it(
    "allows symlinks that stay inside the reference root",
    { skip: !SYMLINKS_SUPPORTED },
    async () => {
      const root = await withRoot();
      await writeFixtureFiles(root.root, { "target.txt": "x" });
      const linkPath = path.join(root.root, "inside-link.txt");
      await createSymlink(path.join(root.root, "target.txt"), linkPath);
      const resolved = await resolveReferencePath(root.root, "inside-link.txt");
      expect(resolved).toMatchObject({ ok: true, relative: "inside-link.txt" });
    },
  );

  it(
    "rejects symlink escape through an ancestor when the target is missing",
    { skip: !SYMLINKS_SUPPORTED },
    async () => {
      const root = await withRoot();
      const outside = await createTempWorkspace();
      roots.push(outside);
      const linkPath = path.join(root.root, "escape-dir");
      await createSymlink(outside.root, linkPath);
      const resolved = await resolveReferencePath(root.root, "escape-dir/missing.txt");
      expect(resolved).toMatchObject({ ok: false });
    },
  );
});

describe("isReferenceRootWithin / assertReferenceRoot", () => {
  it("reports roots inside the workspace namespace", () => {
    expect(isReferenceRootWithin("/workspace/refs/docs", "/workspace")).toBe(true);
    expect(isReferenceRootWithin("/workspace", "/workspace")).toBe(true);
    expect(isReferenceRootWithin("C:\\workspace\\refs", "C:\\workspace")).toBe(true);
  });

  it("reports roots outside the workspace namespace", () => {
    expect(isReferenceRootWithin("/other/docs", "/workspace")).toBe(false);
    expect(isReferenceRootWithin("/workspace-sibling", "/workspace")).toBe(false);
  });

  it("assertReferenceRoot throws only for in-workspace roots", () => {
    expect(() => assertReferenceRoot("/workspace/refs/docs", "/workspace")).toThrow(
      "Reference root must be outside the workspace namespace.",
    );
    expect(() => assertReferenceRoot("/other/docs", "/workspace")).not.toThrow();
  });
});
