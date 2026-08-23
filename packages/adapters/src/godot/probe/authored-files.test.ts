import { mkdtemp, mkdir, rm, symlink, writeFile } from "node:fs/promises";
import { mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  computeAuthoredFileDigest,
  scanAuthoredFiles,
  type AuthoredFileEntry,
} from "./authored-files.js";

async function fixture(): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), "siralos-authored-"));
  await mkdir(join(root, "scripts"), { recursive: true });
  await writeFile(join(root, "project.godot"), '[application]\nconfig/name="Test"\n');
  await writeFile(join(root, "scripts", "tool.gd"), "extends Node\n");
  await writeFile(join(root, "scripts", "main.tscn"), "[gd_scene]\n");
  return root;
}

async function entriesOf(root: string): Promise<readonly AuthoredFileEntry[]> {
  return (await scanAuthoredFiles({ workspaceRoot: root })).entries;
}

function probeSymlinkSupport(): boolean {
  let supported: boolean;
  let probeDir: string | undefined;
  try {
    probeDir = mkdtempSync(join(tmpdir(), "siralos-authored-symlink-probe-"));
    const target = join(probeDir, "target.txt");
    writeFileSync(target, "x");
    symlinkSync(target, join(probeDir, "link.txt"));
    supported = true;
  } catch {
    supported = false;
  } finally {
    if (probeDir !== undefined) {
      rmSync(probeDir, { recursive: true, force: true });
    }
  }
  return supported;
}

const SYMLINKS_SUPPORTED = probeSymlinkSupport();

describe("scanAuthoredFiles", () => {
  it("walks the workspace deterministically and normalizes separators", async () => {
    const root = await fixture();
    try {
      const first = await entriesOf(root);
      const second = await entriesOf(root);
      expect(first).toEqual(second);
      const paths = first.map((entry) => entry.relativePath).sort();
      expect(paths).toEqual(["project.godot", "scripts/main.tscn", "scripts/tool.gd"]);
      for (const entry of first) {
        expect(entry.sha256).toMatch(/^[0-9a-f]{64}$/);
        expect(entry.bytes).toBeGreaterThan(0);
      }
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("produces a digest that changes when a file changes", async () => {
    const root = await fixture();
    try {
      const before = await scanAuthoredFiles({ workspaceRoot: root });
      await writeFile(join(root, "scripts", "tool.gd"), "extends Node\n# changed\n");
      const after = await scanAuthoredFiles({ workspaceRoot: root });
      expect(after.digest).not.toBe(before.digest);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("never enters excluded generated directories", async () => {
    const root = await fixture();
    try {
      await mkdir(join(root, ".godot"), { recursive: true });
      await writeFile(join(root, ".godot", "cache.bin"), "generated");
      await mkdir(join(root, "node_modules"), { recursive: true });
      await writeFile(join(root, "node_modules", "dep.js"), "untrusted");
      const entries = await entriesOf(root);
      const paths = entries.map((entry) => entry.relativePath);
      expect(paths).not.toContain(".godot/cache.bin");
      expect(paths).not.toContain("node_modules/dep.js");
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("skips symbolic links without following them", { skip: !SYMLINKS_SUPPORTED }, async () => {
    const root = await fixture();
    const outside = await mkdtemp(join(tmpdir(), "siralos-authored-outside-"));
    try {
      await writeFile(join(outside, "secret.txt"), "outside content");
      await symlink(join(outside, "secret.txt"), join(root, "link.txt"));
      await symlink(outside, join(root, "linked-dir"));
      const entries = await entriesOf(root);
      const paths = entries.map((entry) => entry.relativePath);
      expect(paths).not.toContain("link.txt");
      expect(paths).not.toContain("linked-dir/secret.txt");
    } finally {
      await rm(root, { recursive: true, force: true });
      await rm(outside, { recursive: true, force: true });
    }
  });

  it("truncates at the file-count bound", async () => {
    const root = await fixture();
    try {
      const result = await scanAuthoredFiles({ workspaceRoot: root, maxFiles: 2 });
      expect(result.truncated).toBe(true);
      expect(result.fileCount).toBeLessThanOrEqual(2);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("truncates at the aggregate-byte bound", async () => {
    const root = await fixture();
    try {
      const result = await scanAuthoredFiles({ workspaceRoot: root, maxBytes: 10 });
      expect(result.truncated).toBe(true);
      expect(result.totalBytes).toBeLessThanOrEqual(10);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("truncates at the directory-entry bound without materializing fanout", async () => {
    const root = await mkdtemp(join(tmpdir(), "siralos-authored-fanout-"));
    try {
      for (let index = 0; index < 500; index += 1) {
        await writeFile(join(root, `f${String(index).padStart(4, "0")}.txt`), "x");
      }
      const result = await scanAuthoredFiles({ workspaceRoot: root, maxEntries: 100 });
      expect(result.truncated).toBe(true);
      expect(result.fileCount).toBeLessThanOrEqual(100);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("truncates at the depth bound", async () => {
    const root = await fixture();
    try {
      const deep = join(root, ...Array.from({ length: 8 }, (_, index) => `d${index}`));
      await mkdir(deep, { recursive: true });
      await writeFile(join(deep, "deep.txt"), "deep");
      const result = await scanAuthoredFiles({ workspaceRoot: root, maxDepth: 3 });
      expect(result.truncated).toBe(true);
      const paths = result.entries.map((entry) => entry.relativePath);
      expect(paths.some((path) => path.endsWith("deep.txt"))).toBe(false);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("truncates when the deadline has passed", async () => {
    const root = await fixture();
    try {
      const result = await scanAuthoredFiles({ workspaceRoot: root, timeoutMs: -1000 });
      expect(result.truncated).toBe(true);
      expect(result.fileCount).toBe(0);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("throws when cancelled", async () => {
    const root = await fixture();
    try {
      const controller = new AbortController();
      controller.abort();
      await expect(
        scanAuthoredFiles({ workspaceRoot: root, signal: controller.signal }),
      ).rejects.toMatchObject({ name: "AbortError" });
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
});

describe("computeAuthoredFileDigest", () => {
  it("is deterministic for the same ordered entry list", () => {
    const entries: readonly AuthoredFileEntry[] = [
      { relativePath: "a.txt", bytes: 3, sha256: "a".repeat(64) },
      { relativePath: "b.txt", bytes: 5, sha256: "b".repeat(64) },
    ];
    expect(computeAuthoredFileDigest(entries)).toBe(computeAuthoredFileDigest(entries));
    expect(computeAuthoredFileDigest(entries)).toMatch(/^[0-9a-f]{64}$/);
    expect(computeAuthoredFileDigest([...entries].reverse())).not.toBe(
      computeAuthoredFileDigest(entries),
    );
  });
});
