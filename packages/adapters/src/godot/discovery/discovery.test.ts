import { mkdir, mkdtemp, rm, stat, symlink, utimes, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { discoverOnPath } from "./path-discovery.js";
import {
  hashFile,
  revalidateExecutableIdentity,
  validateExecutable,
} from "./executable-validation.js";
import { resolveMacOsBundle } from "./macos-bundle.js";

const tempDirectories: string[] = [];

async function withTemp(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "siralos-godot-discovery-"));
  tempDirectories.push(directory);
  return directory;
}

afterEach(async () => {
  for (const directory of tempDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
});

async function writeExecutable(path: string, content = "fake godot"): Promise<string> {
  await writeFile(path, content);
  return path;
}

describe("validateExecutable", () => {
  it("accepts a regular executable file and fingerprints it", async () => {
    const root = await withTemp();
    const path = await writeExecutable(join(root, "godot.exe"));
    const result = await validateExecutable({ path, workspaceRoot: join(root, "workspace") });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.identity.sizeBytes).toBe("fake godot".length);
      expect(result.identity.sha256).toMatch(/^[0-9a-f]{64}$/);
      expect(result.identity.canonicalPath.length).toBeGreaterThan(0);
    }
  });

  it("fails for a missing executable", async () => {
    const root = await withTemp();
    const result = await validateExecutable({
      path: join(root, "missing.exe"),
      workspaceRoot: join(root, "workspace"),
    });
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error).toMatch(/does not exist/);
    }
  });

  it("rejects directories", async () => {
    const root = await withTemp();
    const result = await validateExecutable({
      path: root,
      workspaceRoot: join(root, "workspace"),
    });
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error).toMatch(/not a regular file/);
    }
  });

  it("rejects executables inside the project workspace", async () => {
    const root = await withTemp();
    const workspace = join(root, "workspace");
    await mkdir(workspace);
    const path = await writeExecutable(join(workspace, "godot.exe"));
    const result = await validateExecutable({ path, workspaceRoot: workspace });
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error).toMatch(/inside the project workspace/);
    }
  });

  it("rejects oversized executables", async () => {
    const root = await withTemp();
    const path = await writeExecutable(join(root, "godot.exe"), "x".repeat(10));
    const result = await validateExecutable({ path, workspaceRoot: root, maxBytes: 5 });
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error).toMatch(/size limit/);
    }
  });

  it("canonicalizes through symlinks and records the target", async () => {
    const root = await withTemp();
    const target = await writeExecutable(join(root, "real-godot.exe"));
    const linked = join(root, "godot-link.exe");
    try {
      await symlink(target, linked);
    } catch {
      // symlinks unsupported (e.g. restricted Windows) - skip
      return;
    }
    const result = await validateExecutable({ path: linked, workspaceRoot: join(root, "w") });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.identity.canonicalPath.toLowerCase()).toBe(target.toLowerCase());
    }
  });

  it("records the enclosing .app bundle path for bundle executables", async () => {
    const root = await withTemp();
    const bundle = join(root, "Godot.app");
    const macos = join(bundle, "Contents", "MacOS");
    await mkdir(macos, { recursive: true });
    const executable = await writeExecutable(join(macos, "GodotBin"));
    const result = await validateExecutable({ path: executable, workspaceRoot: join(root, "w") });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.identity.bundlePath).toBe(bundle);
    }
  });

  it("records no bundle for standalone executables", async () => {
    const root = await withTemp();
    const path = await writeExecutable(join(root, "godot.exe"));
    const result = await validateExecutable({ path, workspaceRoot: join(root, "w") });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.identity.bundlePath).toBeNull();
    }
  });
});

describe("revalidateExecutableIdentity", () => {
  it("accepts an unchanged executable (full re-hash)", async () => {
    const root = await withTemp();
    const path = await writeExecutable(join(root, "godot.exe"));
    const result = await validateExecutable({ path, workspaceRoot: join(root, "w") });
    expect(result.ok).toBe(true);
    if (result.ok) {
      const revalidated = await revalidateExecutableIdentity(result.identity);
      expect(revalidated.unchanged).toBe(true);
    }
  });

  it("rejects a same-size replacement with a restored modification time", async () => {
    const root = await withTemp();
    const path = await writeExecutable(join(root, "godot.exe"), "fake godot binary");
    const result = await validateExecutable({ path, workspaceRoot: join(root, "w") });
    expect(result.ok).toBe(true);
    if (!result.ok) {
      return;
    }
    const identity = result.identity;
    const metadata = await stat(identity.canonicalPath);
    // Same byte length, different content, exact mtime restored: the old
    // size+mtime check would pass; the complete re-hash must not.
    await writeFile(identity.canonicalPath, "FAKE GODOT BINARY");
    await utimes(identity.canonicalPath, metadata.atime, metadata.mtime);
    const revalidated = await revalidateExecutableIdentity(identity);
    expect(revalidated.unchanged).toBe(false);
    if (!revalidated.unchanged) {
      expect(revalidated.error).toMatch(/rediscovery/);
    }
  });

  it("rejects a symlink swapped onto the canonical target", async () => {
    const root = await withTemp();
    const target = await writeExecutable(join(root, "real.exe"), "original bytes");
    const linked = join(root, "link.exe");
    try {
      await symlink(target, linked);
    } catch {
      // symlinks unsupported (e.g. restricted Windows) - skip
      return;
    }
    const result = await validateExecutable({ path: linked, workspaceRoot: join(root, "w") });
    expect(result.ok).toBe(true);
    if (!result.ok) {
      return;
    }
    const other = await writeExecutable(join(root, "other.exe"), "other bytes");
    await rm(target);
    await symlink(other, target);
    const revalidated = await revalidateExecutableIdentity(result.identity);
    expect(revalidated.unchanged).toBe(false);
    if (!revalidated.unchanged) {
      expect(revalidated.error).toMatch(/rediscovery/);
    }
  });

  it("rejects a canonical target that no longer resolves to itself", async () => {
    const root = await withTemp();
    const path = await writeExecutable(join(root, "godot.exe"));
    const result = await validateExecutable({ path, workspaceRoot: join(root, "w") });
    expect(result.ok).toBe(true);
    if (!result.ok) {
      return;
    }
    await rm(result.identity.canonicalPath);
    const revalidated = await revalidateExecutableIdentity(result.identity);
    expect(revalidated.unchanged).toBe(false);
  });
});

describe("hashFile", () => {
  it("computes a bounded SHA-256 of the file bytes", async () => {
    const root = await withTemp();
    const path = await writeExecutable(join(root, "godot.exe"), "bounded bytes");
    const digest = await hashFile(path, undefined, 1024);
    expect(digest).toMatch(/^[0-9a-f]{64}$/);
  });

  it("fails closed when the file exceeds the byte bound", async () => {
    const root = await withTemp();
    const path = await writeExecutable(join(root, "godot.exe"), "123456789");
    expect(await hashFile(path, undefined, 5)).toBeNull();
  });

  it("returns null for a missing file", async () => {
    const root = await withTemp();
    expect(await hashFile(join(root, "missing.exe"))).toBeNull();
  });
});

describe("discoverOnPath", () => {
  it("finds fixed candidate names only", async () => {
    const root = await withTemp();
    const directory = join(root, "bin");
    await mkdir(directory);
    await writeExecutable(join(directory, "godot.exe"));
    await writeExecutable(join(directory, "godot4.exe"));
    await writeExecutable(join(directory, "other-tool.exe"));
    const result = await discoverOnPath({
      hostPath: directory,
      hostPathExt: null,
      platform: "win32",
      workspaceRoot: join(root, "workspace"),
    });
    const ids = result.candidates.map((candidate) => candidate.id);
    expect(ids).toEqual(["path-1", "path-2"]);
  });

  it("searches multiple PATH entries deterministically", async () => {
    const root = await withTemp();
    const first = join(root, "first");
    const second = join(root, "second");
    await mkdir(first);
    await mkdir(second);
    await writeExecutable(join(first, "godot.exe"), "first");
    await writeExecutable(join(second, "godot.exe"), "second");
    const result = await discoverOnPath({
      hostPath: `${first};${second}`,
      hostPathExt: ".EXE",
      platform: "win32",
      workspaceRoot: join(root, "workspace"),
    });
    expect(result.candidates.map((candidate) => candidate.sourceLabel)).toEqual(["PATH", "PATH"]);
  });

  it("applies PATHEXT safely on Windows", async () => {
    const root = await withTemp();
    const directory = join(root, "bin");
    await mkdir(directory);
    await writeExecutable(join(directory, "godot.exe"));
    await writeExecutable(join(directory, "godot.bat"));
    const result = await discoverOnPath({
      hostPath: directory,
      hostPathExt: ".EXE;.BAT;.CMD",
      platform: "win32",
      workspaceRoot: join(root, "workspace"),
    });
    expect(result.candidates.length).toBeGreaterThan(0);
    for (const candidate of result.candidates) {
      expect(candidate.canonicalPath.toLowerCase()).not.toMatch(/\.(bat|cmd)$/);
    }
  });

  it("deduplicates canonical paths", async () => {
    const root = await withTemp();
    const directory = join(root, "bin");
    await mkdir(directory);
    await writeExecutable(join(directory, "godot.exe"));
    const result = await discoverOnPath({
      hostPath: `${directory};${directory}`,
      hostPathExt: ".EXE",
      platform: "win32",
      workspaceRoot: join(root, "workspace"),
    });
    expect(result.candidates.length).toBe(1);
  });

  it("bounds candidate count", async () => {
    const root = await withTemp();
    const directories: string[] = [];
    for (let index = 0; index < 10; index += 1) {
      const directory = join(root, `bin-${index}`);
      await mkdir(directory);
      directories.push(directory);
      await writeExecutable(join(directory, "godot.exe"));
      await writeExecutable(join(directory, "godot4.exe"));
    }
    const result = await discoverOnPath({
      hostPath: directories.join(";"),
      hostPathExt: ".EXE",
      platform: "win32",
      workspaceRoot: join(root, "workspace"),
    });
    expect(result.candidates.length).toBeLessThanOrEqual(16);
  });

  it("handles an empty PATH", async () => {
    const root = await withTemp();
    const result = await discoverOnPath({
      hostPath: null,
      hostPathExt: null,
      platform: "win32",
      workspaceRoot: join(root, "workspace"),
    });
    expect(result.candidates).toEqual([]);
  });
});

describe("resolveMacOsBundle", () => {
  it("resolves the exact executable from an Info.plist", async () => {
    const root = await withTemp();
    const bundle = join(root, "Godot.app");
    const macos = join(bundle, "Contents", "MacOS");
    await mkdir(macos, { recursive: true });
    await writeFile(
      join(bundle, "Contents", "Info.plist"),
      '<?xml version="1.0"?><plist><dict><key>CFBundleExecutable</key><string>GodotBin</string></dict></plist>',
    );
    await writeExecutable(join(macos, "GodotBin"));
    const result = await resolveMacOsBundle(bundle);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.executablePath.toLowerCase()).toBe(join(macos, "GodotBin").toLowerCase());
    }
  });

  it("falls back to the conventional Godot executable name", async () => {
    const root = await withTemp();
    const bundle = join(root, "Godot.app");
    const macos = join(bundle, "Contents", "MacOS");
    await mkdir(macos, { recursive: true });
    await writeExecutable(join(macos, "Godot"));
    const result = await resolveMacOsBundle(bundle);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.executablePath.toLowerCase()).toBe(join(macos, "Godot").toLowerCase());
    }
  });

  it("rejects bundles without Contents/MacOS", async () => {
    const root = await withTemp();
    const bundle = join(root, "Godot.app");
    await mkdir(bundle);
    const result = await resolveMacOsBundle(bundle);
    expect(result.ok).toBe(false);
  });

  it("rejects non-bundle paths", async () => {
    const root = await withTemp();
    const result = await resolveMacOsBundle(join(root, "Godot"));
    expect(result.ok).toBe(false);
  });
});
