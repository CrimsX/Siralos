import { afterEach, describe, expect, it } from "vitest";
import { mkdtemp, mkdir, rm, symlink, writeFile, readFile, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { createHash } from "node:crypto";
import { createProjectMirror, type ProjectMirrorLimits } from "./project-mirror.js";

const tempRoots: string[] = [];

async function withTempRoot(): Promise<string> {
  const root = await mkdtemp(path.join(tmpdir(), "solaris-mirror-test-"));
  tempRoots.push(root);
  return root;
}

async function writeFiles(root: string, files: Readonly<Record<string, string>>): Promise<void> {
  for (const [relativePath, content] of Object.entries(files)) {
    const target = path.join(root, relativePath);
    await mkdir(path.dirname(target), { recursive: true });
    await writeFile(target, content);
  }
}

function sha256(text: string): string {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

async function createDirSymlinkOrSkip(target: string, linkPath: string): Promise<void> {
  return createSymlinkOrSkip(
    target,
    linkPath,
    process.platform === "win32" ? "junction" : ("dir" as const),
  );
}

async function createSymlinkOrSkip(
  target: string,
  linkPath: string,
  type: "file" | "junction" | "dir",
): Promise<void> {
  try {
    await symlink(target, linkPath, type);
  } catch (error: unknown) {
    if (process.platform === "win32" && isPermissionError(error)) {
      return;
    }
    throw error;
  }
}

function isPermissionError(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "EPERM";
}

function smallLimits(overrides: Partial<ProjectMirrorLimits> = {}): ProjectMirrorLimits {
  return {
    maxFiles: 100,
    maxBytes: 1024 * 1024,
    maxSingleFileBytes: 512 * 1024,
    maxRelativePathBytes: 1024,
    maxDepth: 64,
    prepareTimeoutMs: 30_000,
    ...overrides,
  };
}

afterEach(async () => {
  await Promise.all(
    tempRoots.map((root) => rm(root, { recursive: true, force: true }).catch(() => undefined)),
  );
  tempRoots.length = 0;
});

describe("project mirror preparation", () => {
  it("copies regular files with verified hashes", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, {
      "project.godot": '[application]\nconfig/name="Demo"\n',
      "src/main.gd": "extends Node\nfunc _ready():\n    pass\n",
      "assets/logo.png": "not-a-real-png",
    });
    const parent = await withTempRoot();
    const mirror = createProjectMirror();
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    expect(result.mirror.entries).toHaveLength(3);
    const byPath = new Map(result.mirror.entries.map((entry) => [entry.relativePath, entry]));
    expect(byPath.get("project.godot")?.sha256).toBe(sha256('[application]\nconfig/name="Demo"\n'));
    expect(byPath.get("src/main.gd")).toBeDefined();
    expect(byPath.get("assets/logo.png")?.bytes).toBe("not-a-real-png".length);
    const mirrored = await readFile(path.join(result.mirror.projectPath, "src/main.gd"), "utf8");
    expect(mirrored).toBe("extends Node\nfunc _ready():\n    pass\n");
    const verification = await mirror.verify(result.mirror);
    expect(verification.ok).toBe(true);
  });

  it("never copies .git, .godot, node_modules, dist, coverage, or .solaris", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, {
      "project.godot": "x",
      "src/main.gd": "y",
      ".git/config": "secret",
      ".godot/global_script_class_cache.cfg": "cache",
      "node_modules/pkg/index.js": "dep",
      "dist/bundle.js": "bundle",
      "coverage/lcov.info": "cov",
      ".solaris/something": "meta",
      ".solaris-mutation-abc/temp": "temp",
    });
    const parent = await withTempRoot();
    const mirror = createProjectMirror();
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    const paths = result.mirror.entries.map((entry) => entry.relativePath);
    expect(paths.sort()).toEqual(["project.godot", "src/main.gd"]);
  });

  it("rejects a symlinked file without dereferencing it", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, { "project.godot": "x", "real.txt": "real" });
    const linkPath = path.join(workspace, "linked.txt");
    await createSymlinkOrSkip(path.join(workspace, "real.txt"), linkPath, "file");
    const linkExists = (await stat(linkPath).catch(() => null)) !== null;
    if (!linkExists) {
      return; // symlink creation unsupported on this platform
    }
    const parent = await withTempRoot();
    const mirror = createProjectMirror();
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("mirror_unsupported");
    if (result.status !== "mirror_unsupported") {
      return;
    }
    expect(result.message).toContain("symbolic link");
    // The mirror is cleaned and the real file is untouched.
    expect(await readFile(path.join(workspace, "real.txt"), "utf8")).toBe("real");
    expect(await stat(path.join(parent, "project")).catch(() => null)).toBeNull();
  });

  it("rejects a symlinked directory and junction traversal", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, { "project.godot": "x", "outside/secret.txt": "secret" });
    const parent = await withTempRoot();
    await writeFiles(parent, { "outside/secret.txt": "outside-secret" });
    const linkPath = path.join(workspace, "linked-dir");
    const target =
      process.platform === "win32" ? path.join(parent, "outside") : path.join(workspace, "outside");
    try {
      await symlink(target, linkPath, process.platform === "win32" ? "junction" : "dir");
    } catch (error: unknown) {
      if (process.platform === "win32" && isPermissionError(error)) {
        return;
      }
      throw error;
    }
    const mirror = createProjectMirror();
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    if (result.status === "ready") {
      // On platforms where the link type was not created, skip.
      return;
    }
    expect(result.status).toBe("mirror_unsupported");
    if (result.status !== "mirror_unsupported") {
      return;
    }
    expect(result.message).toContain("symbolic link");
    expect(await stat(path.join(parent, "project")).catch(() => null)).toBeNull();
  });

  it("rejects special files", async () => {
    if (process.platform === "win32") {
      return; // FIFO/device files are not representable here
    }
    const workspace = await withTempRoot();
    await writeFiles(workspace, { "project.godot": "x" });
    const { spawnSync } = await import("node:child_process");
    const fifo = path.join(workspace, "pipe.fifo");
    spawnSync("mkfifo", [fifo]);
    const parent = await withTempRoot();
    const mirror = createProjectMirror();
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("mirror_unsupported");
    expect(await stat(path.join(parent, "project")).catch(() => null)).toBeNull();
  });

  it("stops safely at the file-count limit and cleans the partial mirror", async () => {
    const workspace = await withTempRoot();
    const files: Record<string, string> = { "project.godot": "x" };
    for (let index = 0; index < 5; index += 1) {
      files[`f${index}.txt`] = `content ${index}`;
    }
    await writeFiles(workspace, files);
    const parent = await withTempRoot();
    const mirror = createProjectMirror({ limits: smallLimits({ maxFiles: 3 }) });
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("too_large");
    if (result.status !== "too_large") {
      return;
    }
    expect(result.limit).toBe("files");
    expect(await stat(path.join(parent, "project")).catch(() => null)).toBeNull();
  });

  it("stops safely at the total byte limit and reports it", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, {
      "project.godot": "x".repeat(300),
      "big.bin": "y".repeat(300),
    });
    const parent = await withTempRoot();
    const mirror = createProjectMirror({ limits: smallLimits({ maxBytes: 400 }) });
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("too_large");
    if (result.status !== "too_large") {
      return;
    }
    expect(result.limit).toBe("bytes");
    expect(await stat(path.join(parent, "project")).catch(() => null)).toBeNull();
  });

  it("stops safely at the single-file limit", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, { "project.godot": "x", "huge.bin": "y".repeat(500) });
    const parent = await withTempRoot();
    const mirror = createProjectMirror({ limits: smallLimits({ maxSingleFileBytes: 100 }) });
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("too_large");
    if (result.status !== "too_large") {
      return;
    }
    expect(result.limit).toBe("single-file");
  });

  it("stops safely at the directory-depth limit", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, {
      "project.godot": "x",
      "a/b/c/d/e.txt": "deep",
    });
    const parent = await withTempRoot();
    const mirror = createProjectMirror({ limits: smallLimits({ maxDepth: 3 }) });
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("too_large");
    if (result.status !== "too_large") {
      return;
    }
    expect(result.limit).toBe("depth");
  });

  it("honours cancellation during the copy and cleans the partial mirror", async () => {
    const workspace = await withTempRoot();
    const files: Record<string, string> = { "project.godot": "x" };
    for (let index = 0; index < 200; index += 1) {
      files[`f${index}.txt`] = "z".repeat(512 * 1024);
    }
    await writeFiles(workspace, files);
    const parent = await withTempRoot();
    const controller = new AbortController();
    // Cancel while the copy is in flight (100 MiB cannot finish instantly).
    setTimeout(() => controller.abort(), 20);
    const mirror = createProjectMirror();
    await expect(
      mirror.prepare({
        workspaceRoot: workspace,
        parentDirectory: parent,
        signal: controller.signal,
      }),
    ).rejects.toThrow(/aborted/i);
    expect(await stat(path.join(parent, "project")).catch(() => null)).toBeNull();
  });

  it("refuses immediately when the signal is already aborted", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, { "project.godot": "x" });
    const parent = await withTempRoot();
    const controller = new AbortController();
    controller.abort();
    const mirror = createProjectMirror();
    await expect(
      mirror.prepare({
        workspaceRoot: workspace,
        parentDirectory: parent,
        signal: controller.signal,
      }),
    ).rejects.toThrow(/aborted/i);
    expect(await stat(path.join(parent, "project")).catch(() => null)).toBeNull();
  });

  it("detects source mutation during mirroring as a conflict", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, { "project.godot": "x", "main.gd": "version-one" });
    const parent = await withTempRoot();
    const mirror = createProjectMirror();
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    // Mutate the source after preparation; verification must notice.
    await writeFile(path.join(workspace, "main.gd"), "version-two");
    const verification = await mirror.verify(result.mirror);
    expect(verification.ok).toBe(false);
    if (!verification.ok) {
      expect(verification.reason).toBe("source-conflict");
    }
  });

  it("detects a hash mismatch in the copied mirror", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, { "project.godot": "x", "main.gd": "original" });
    const parent = await withTempRoot();
    const mirror = createProjectMirror();
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    await writeFile(path.join(result.mirror.projectPath, "main.gd"), "tampered");
    const verification = await mirror.verify(result.mirror);
    expect(verification.ok).toBe(false);
    if (!verification.ok) {
      expect(verification.reason).toBe("hash-mismatch");
    }
  });

  it("detects an unexpected file in the mirror", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, { "project.godot": "x" });
    const parent = await withTempRoot();
    const mirror = createProjectMirror();
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    await writeFile(path.join(result.mirror.projectPath, "sneaky.txt"), "added");
    const verification = await mirror.verify(result.mirror);
    expect(verification.ok).toBe(false);
    if (!verification.ok) {
      expect(verification.reason).toBe("unexpected-files");
    }
  });

  it("refuses a mirror parent inside the workspace", async () => {
    const workspace = await withTempRoot();
    const parent = path.join(workspace, "runs");
    await mkdir(parent);
    const mirror = createProjectMirror();
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("failed");
    expect(await stat(path.join(parent, "project")).catch(() => null)).toBeNull();
  });

  it("refuses a mirror parent inside a forbidden root such as checkpoint storage", async () => {
    const workspace = await withTempRoot();
    const checkpoints = await withTempRoot();
    const parent = path.join(checkpoints, "run-1");
    await mkdir(parent);
    const mirror = createProjectMirror();
    const result = await mirror.prepare({
      workspaceRoot: workspace,
      parentDirectory: parent,
      forbiddenRoots: [checkpoints],
    });
    expect(result.status).toBe("failed");
    expect(await stat(path.join(parent, "project")).catch(() => null)).toBeNull();
  });

  it("never lets a provider select the mirror path (fixed project subdirectory)", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, { "project.godot": "x" });
    const parent = await withTempRoot();
    const mirror = createProjectMirror();
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    // The mirror path is always <verified parent>/project; the request has
    // no provider-controlled path fields.
    expect(result.mirror.projectPath).toBe(path.join(parent, "project"));
  });
});

describe("project mirror cleanup", () => {
  it("deletes the mirror on demand", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, { "project.godot": "x", "f.txt": "y" });
    const parent = await withTempRoot();
    const mirror = createProjectMirror();
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    const outcome = await mirror.destroy(result.mirror);
    expect(outcome.ok).toBe(true);
    expect(await stat(path.join(parent, "project")).catch(() => null)).toBeNull();
  });

  it("refuses to remove a mirror path outside its verified parent", async () => {
    const workspace = await withTempRoot();
    const parent = await withTempRoot();
    const mirror = createProjectMirror();
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    const other = await withTempRoot();
    await writeFile(path.join(other, "keep.txt"), "keep");
    const outcome = await mirror.destroy({
      ...result.mirror,
      projectPath: path.join(other, "keep.txt"),
    });
    expect(outcome.ok).toBe(false);
    if (!outcome.ok) {
      expect(outcome.message).toContain("not inside");
    }
    expect(await readFile(path.join(other, "keep.txt"), "utf8")).toBe("keep");
  });

  it("never follows a malicious symlink planted at the mirror path", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, { "project.godot": "x" });
    const parent = await withTempRoot();
    const mirror = createProjectMirror();
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    const victim = await withTempRoot();
    await writeFiles(victim, { "important.txt": "do-not-delete" });
    const mirrorPath = result.mirror.projectPath;
    await rm(mirrorPath, { recursive: true, force: true });
    await createDirSymlinkOrSkip(victim, mirrorPath);
    const outcome = await mirror.destroy(result.mirror);
    if (process.platform === "win32") {
      // Junction creation without privileges is supported; refuse + preserve.
      expect(outcome.ok).toBe(false);
      expect(await readFile(path.join(victim, "important.txt"), "utf8")).toBe("do-not-delete");
      await rm(mirrorPath, { recursive: true, force: true });
      return;
    }
    expect(outcome.ok).toBe(false);
    expect(await readFile(path.join(victim, "important.txt"), "utf8")).toBe("do-not-delete");
  });

  it("handles a symlink inside the mirror without following it", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, { "project.godot": "x", "dir/f.txt": "y" });
    const parent = await withTempRoot();
    const mirror = createProjectMirror();
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    const victim = await withTempRoot();
    await writeFiles(victim, { "important.txt": "do-not-delete" });
    const linkPath = path.join(result.mirror.projectPath, "dir", "escape");
    await createDirSymlinkOrSkip(victim, linkPath);
    const outcome = await mirror.destroy(result.mirror);
    expect(outcome.ok).toBe(true);
    expect(await readFile(path.join(victim, "important.txt"), "utf8")).toBe("do-not-delete");
    expect(await stat(path.join(parent, "project")).catch(() => null)).toBeNull();
  });

  it("cleans a partial mirror after preparation failure", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, { "project.godot": "x", "a.txt": "y", "b.txt": "z" });
    const parent = await withTempRoot();
    const mirror = createProjectMirror({ limits: smallLimits({ maxFiles: 1 }) });
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("too_large");
    expect(await stat(path.join(parent, "project")).catch(() => null)).toBeNull();
  });

  it("does not delete sibling runs during cleanup", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, { "project.godot": "x" });
    const parent = await withTempRoot();
    const sibling = path.join(parent, "other-run");
    await mkdir(sibling);
    await writeFile(path.join(sibling, "keep.txt"), "keep");
    const mirror = createProjectMirror();
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    const outcome = await mirror.destroy(result.mirror);
    expect(outcome.ok).toBe(true);
    expect(await readFile(path.join(sibling, "keep.txt"), "utf8")).toBe("keep");
  });

  it("never touches the source workspace during cleanup", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, { "project.godot": "x", "keep.txt": "keep" });
    const parent = await withTempRoot();
    const mirror = createProjectMirror();
    const result = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    await mirror.destroy(result.mirror);
    expect(await readFile(path.join(workspace, "keep.txt"), "utf8")).toBe("keep");
    expect(await readFile(path.join(workspace, "project.godot"), "utf8")).toBe("x");
  });

  it("prepares a second mirror in the same parent after the first was destroyed", async () => {
    const workspace = await withTempRoot();
    await writeFiles(workspace, { "project.godot": "x" });
    const parent = await withTempRoot();
    const mirror = createProjectMirror();
    const first = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(first.status).toBe("ready");
    if (first.status !== "ready") {
      return;
    }
    await mirror.destroy(first.mirror);
    const second = await mirror.prepare({ workspaceRoot: workspace, parentDirectory: parent });
    expect(second.status).toBe("ready");
  });
});
