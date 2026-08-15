import { afterEach, describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readWorkspaceFileState } from "./checkpoint-file-state.js";

describe("readWorkspaceFileState", () => {
  const dirs: string[] = [];

  afterEach(async () => {
    for (const dir of dirs.splice(0)) {
      await rm(dir, { recursive: true, force: true });
    }
  });

  async function withDir(prefix: string): Promise<string> {
    const dir = await mkdtemp(join(tmpdir(), prefix));
    dirs.push(dir);
    return dir;
  }

  it("hashes the exact complete bytes of an in-workspace file", async () => {
    const workspace = await withDir("siralos-cpfs-ws-");
    await writeFile(join(workspace, "a.txt"), "hello", "utf8");
    const expected = createHash("sha256").update("hello", "utf8").digest("hex");
    expect(await readWorkspaceFileState(workspace, "a.txt")).toEqual({
      exists: true,
      sha256: expected,
    });
    await mkdir(join(workspace, "dir"), { recursive: true });
    await writeFile(join(workspace, "dir", "b.txt"), "world", "utf8");
    const nested = await readWorkspaceFileState(workspace, "dir/b.txt");
    expect(nested.sha256).toBe(createHash("sha256").update("world", "utf8").digest("hex"));
  });

  it("reports missing files as not existing", async () => {
    const workspace = await withDir("siralos-cpfs-ws-");
    expect(await readWorkspaceFileState(workspace, "missing.txt")).toEqual({
      exists: false,
      sha256: null,
    });
  });

  it("fails closed on a parent symlink escape", async () => {
    const workspace = await withDir("siralos-cpfs-ws-");
    const outside = await withDir("siralos-cpfs-outside-");
    await writeFile(join(outside, "secret.txt"), "outside secret", "utf8");
    let linked = false;
    try {
      await symlink(outside, join(workspace, "link"), "dir");
      linked = true;
    } catch {
      // Symlink creation is a host privilege; skip the escape assertion.
    }
    const escape = await readWorkspaceFileState(workspace, "link/secret.txt");
    if (linked) {
      // A malicious checkpoint record must never cause inspection
      // outside the workspace: the disposition is the same fail-closed
      // "linked" state, never the outside file's hash.
      expect(escape).toEqual({ exists: true, sha256: null });
    } else {
      expect(escape).toEqual({ exists: false, sha256: null });
    }
  });
});
