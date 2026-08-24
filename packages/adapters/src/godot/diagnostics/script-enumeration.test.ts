import { afterEach, describe, expect, it } from "vitest";
import { mkdtemp, realpath, rm, writeFile, mkdir, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { GODOT_LIMITS } from "@siralos/core";
import { enumerateGDScriptFiles, validateCheckScript } from "./script-enumeration.js";

const tempRoots: string[] = [];

async function withTempRoot(): Promise<string> {
  const created = await mkdtemp(path.join(tmpdir(), "siralos-scripts-test-"));
  // Canonical: production identity checks realpath internally, so tests
  // must compare against the same spelling (macOS /var -> /private/var).
  const root = await realpath(created);
  tempRoots.push(root);
  return root;
}

afterEach(async () => {
  await Promise.all(tempRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

describe("enumerateGDScriptFiles", () => {
  it("enumerates .gd files deterministically with exclusions applied", async () => {
    const root = await withTempRoot();
    await mkdir(path.join(root, "src", "player"), { recursive: true });
    await mkdir(path.join(root, "src", "ui"), { recursive: true });
    await mkdir(path.join(root, "node_modules", "dep"), { recursive: true });
    await mkdir(path.join(root, ".godot"), { recursive: true });
    await mkdir(path.join(root, ".git"), { recursive: true });
    await writeFile(path.join(root, "src", "player", "player.gd"), "extends Node\n");
    await writeFile(path.join(root, "src", "ui", "menu.gd"), "extends Control\n");
    await writeFile(path.join(root, "src", "ui", "menu.tscn"), "not a script");
    await writeFile(path.join(root, "src", "ui", "menu.gd.bak"), "ignored");
    await writeFile(path.join(root, "src", "ui", "NOTE.GD"), "case insensitive");
    await writeFile(path.join(root, "node_modules", "dep", "dep.gd"), "excluded");
    await writeFile(path.join(root, ".godot", "generated.gd"), "excluded");
    const result = await enumerateGDScriptFiles({ workspaceRoot: root });
    expect(result.truncated).toBe(false);
    expect(result.targets.map((entry) => entry.path)).toEqual([
      "src/player/player.gd",
      "src/ui/NOTE.GD",
      "src/ui/menu.gd",
    ]);
  });

  it("skips symlinked entries without following them", async () => {
    const root = await withTempRoot();
    await mkdir(path.join(root, "src"), { recursive: true });
    await writeFile(path.join(root, "src", "real.gd"), "extends Node\n");
    let linkCreated: boolean;
    try {
      await symlink(path.join(root, "src", "real.gd"), path.join(root, "src", "link.gd"));
      linkCreated = true;
    } catch {
      linkCreated = false;
    }
    const result = await enumerateGDScriptFiles({ workspaceRoot: root });
    expect(result.targets.map((entry) => entry.path)).toEqual(
      linkCreated ? ["src/real.gd"] : ["src/real.gd"],
    );
    if (linkCreated) {
      expect(result.targets.some((entry) => entry.path.endsWith("link.gd"))).toBe(false);
    }
  });

  it("enforces the file-count limit with explicit truncation", async () => {
    const root = await withTempRoot();
    await mkdir(path.join(root, "src"), { recursive: true });
    for (let index = 0; index < 15; index += 1) {
      await writeFile(path.join(root, "src", `f${index}.gd`), "extends Node\n");
    }
    const result = await enumerateGDScriptFiles({ workspaceRoot: root, maxFiles: 10 });
    expect(result.targets).toHaveLength(10);
    expect(result.truncated).toBe(true);
  });

  it("enforces the total-byte limit with explicit truncation", async () => {
    const root = await withTempRoot();
    await mkdir(path.join(root, "src"), { recursive: true });
    for (let index = 0; index < 5; index += 1) {
      await writeFile(path.join(root, "src", `f${index}.gd`), "x".repeat(100));
    }
    const result = await enumerateGDScriptFiles({ workspaceRoot: root, maxTotalBytes: 250 });
    expect(result.truncated).toBe(true);
    const total = result.targets.reduce((sum, entry) => sum + entry.bytes, 0);
    expect(total).toBeLessThanOrEqual(250);
  });
});

describe("validateCheckScript", () => {
  function failReason(
    result: import("./script-enumeration.js").GodotCheckScriptValidation,
  ): string | null {
    return result.ok ? null : result.reason;
  }

  it("validates and hashes a workspace-relative .gd file", async () => {
    const root = await withTempRoot();
    await mkdir(path.join(root, "src"), { recursive: true });
    await writeFile(path.join(root, "src", "player.gd"), "extends Node\n");
    const result = await validateCheckScript({
      workspaceRoot: root,
      relativePath: "src/player.gd",
    });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.sha256).toMatch(/^[0-9a-f]{64}$/);
      expect(result.bytes).toBeGreaterThan(0);
    }
  });

  it("rejects absolute paths and parent traversal", async () => {
    const root = await withTempRoot();
    expect(
      failReason(
        await validateCheckScript({ workspaceRoot: root, relativePath: "C:\\outside.gd" }),
      ),
    ).toBe("absolute");
    expect(
      failReason(await validateCheckScript({ workspaceRoot: root, relativePath: "../outside.gd" })),
    ).toBe("traversal");
  });

  it("rejects non-.gd extensions", async () => {
    const root = await withTempRoot();
    expect(
      failReason(await validateCheckScript({ workspaceRoot: root, relativePath: "src/main.tscn" })),
    ).toBe("not-gd");
  });

  it("rejects missing files", async () => {
    const root = await withTempRoot();
    expect(
      failReason(await validateCheckScript({ workspaceRoot: root, relativePath: "src/nope.gd" })),
    ).toBe("missing");
  });

  it("rejects symlinked scripts", async () => {
    const root = await withTempRoot();
    await writeFile(path.join(root, "target.gd"), "extends Node\n");
    let linkCreated: boolean;
    try {
      await symlink(path.join(root, "target.gd"), path.join(root, "link.gd"));
      linkCreated = true;
    } catch {
      linkCreated = false;
    }
    if (linkCreated) {
      expect(
        failReason(await validateCheckScript({ workspaceRoot: root, relativePath: "link.gd" })),
      ).toBe("symlink");
    }
  });

  it("rejects oversized files", async () => {
    const root = await withTempRoot();
    await writeFile(path.join(root, "huge.gd"), "x".repeat(GODOT_LIMITS.maxGDScriptFileBytes + 1));
    expect(
      failReason(await validateCheckScript({ workspaceRoot: root, relativePath: "huge.gd" })),
    ).toBe("too-large");
  });
});
