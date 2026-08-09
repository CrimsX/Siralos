import { mkdtemp, mkdir, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { createProjectMirror, PROJECT_MIRROR_UNAVAILABLE_MESSAGE } from "./project-mirror.js";

describe("createProjectMirror", () => {
  it("reports unavailable availability", async () => {
    expect(await createProjectMirror().isAvailable()).toBe(false);
  });

  it("refuses preparation before creating anything", async () => {
    const parent = await mkdtemp(join(tmpdir(), "solaris-mirror-parent-"));
    try {
      const outcome = await createProjectMirror().prepare({
        workspaceRoot: join(parent, "workspace"),
        parentDirectory: parent,
      });
      expect(outcome.status).toBe("unavailable");
      if (outcome.status !== "unavailable") {
        throw new Error("unreachable");
      }
      expect(outcome.message).toContain("unavailable");
      expect(await readdir(parent)).toEqual([]);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });

  it("refuses verification as unavailable", async () => {
    const outcome = await createProjectMirror().verify({
      projectPath: "C:\\placeholder\\mirror",
      sourceRoot: "C:\\placeholder\\workspace",
      parentDirectory: "C:\\placeholder\\runs",
      entries: [],
      copiedBytes: 0,
    });
    expect(outcome.ok).toBe(false);
    if (outcome.ok === false) {
      expect(outcome.reason).toBe("unavailable");
      expect(typeof outcome.message).toBe("string");
    }
  });

  it("refuses destruction and preserves any existing tree", async () => {
    const root = await mkdtemp(join(tmpdir(), "solaris-mirror-tree-"));
    try {
      await writeFile(join(root, "existing.txt"), "keep me");
      const outcome = await createProjectMirror().destroy({
        projectPath: join(root, "mirror"),
        sourceRoot: join(root, "workspace"),
        parentDirectory: root,
        entries: [],
        copiedBytes: 0,
      });
      expect(outcome.ok).toBe(false);
      if (outcome.ok === false) {
        expect(outcome.reason).toBe("unavailable");
        expect(typeof outcome.message).toBe("string");
      }
      expect(await readdir(root)).toEqual(["existing.txt"]);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("exposes a precise reason for the unavailability", () => {
    expect(PROJECT_MIRROR_UNAVAILABLE_MESSAGE).toContain("directory-relative");
    expect(PROJECT_MIRROR_UNAVAILABLE_MESSAGE).toContain("nothing was created");
  });

  it("never creates outside entries across a refused preparation", async () => {
    const root = await mkdtemp(join(tmpdir(), "solaris-mirror-outside-"));
    try {
      const workspace = join(root, "workspace");
      await mkdir(workspace);
      await writeFile(join(workspace, "project.godot"), "x");
      const outcome = await createProjectMirror().prepare({
        workspaceRoot: workspace,
        parentDirectory: root,
      });
      expect(outcome.status).toBe("unavailable");
      expect(await readdir(root)).toEqual(["workspace"]);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
});
