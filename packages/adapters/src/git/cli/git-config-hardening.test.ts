import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createGitCliAdapter } from "./git-cli-adapter.js";
import { runGitProcess, sanitizeGitEnvironment } from "./git-process.js";
import { cleanupTempDirs, createTempRepo, type TempRepo } from "./git-test-support.js";

afterEach(async () => {
  await cleanupTempDirs();
});

/**
 * Installs a marker script inside the disposable repository. Any Git
 * configuration that executes external code (fsmonitor, aliases, pagers,
 * external diff, textconv, credential helpers) points at this script; the
 * script writes a marker file when it runs. The repo root is disposable and
 * malicious configuration never touches the Solaris repository.
 */
async function installMarker(repo: TempRepo): Promise<string> {
  const markerPath = join(repo.root, "marker-ran.txt");
  const scriptPath = join(repo.root, "marker.cjs");
  const script = `"use strict";
const fs = require("node:fs");
fs.writeFileSync(${JSON.stringify(markerPath)}, "ran\\n");
`;
  await writeFile(scriptPath, script);
  return markerPath;
}

async function assertMarkerAbsent(markerPath: string): Promise<void> {
  await expect(readFile(markerPath, "utf8")).rejects.toThrow();
}

function quotedNodeCommand(scriptName: string): string {
  const node = process.execPath;
  return `"${node}" "${scriptName}"`;
}

describe("git executable-helper hardening", () => {
  it("does not execute a malicious core.fsmonitor hook", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "hello\n");
    repo.git("add", "a.txt");
    repo.commit("initial");
    const markerPath = await installMarker(repo);
    repo.git("config", "core.fsmonitor", quotedNodeCommand(join(repo.root, "marker.cjs")));
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getStatus({});
    expect(result.repository).toBe(true);
    expect(result.changes).toEqual([]);
    await assertMarkerAbsent(markerPath);
  });

  it("does not execute shell aliases that attempt external execution", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "hello\n");
    repo.git("add", "a.txt");
    repo.commit("initial");
    const markerPath = await installMarker(repo);
    repo.git("config", "alias.status", `!${quotedNodeCommand(join(repo.root, "marker.cjs"))}`);
    repo.git("config", "alias.diff", `!${quotedNodeCommand(join(repo.root, "marker.cjs"))}`);
    repo.git(
      "config",
      "alias.check-ignore",
      `!${quotedNodeCommand(join(repo.root, "marker.cjs"))}`,
    );
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getStatus({});
    expect(result.repository).toBe(true);
    await adapter.getDiff({ scope: "working" });
    await expect(adapter.getStatus({}).then(() => true)).resolves.toBe(true);
    await assertMarkerAbsent(markerPath);
  });

  it("does not execute pager configuration", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "hello\n");
    repo.git("add", "a.txt");
    repo.commit("initial");
    const markerPath = await installMarker(repo);
    const command = quotedNodeCommand(join(repo.root, "marker.cjs"));
    repo.git("config", "core.pager", command);
    repo.git("config", "pager.diff", command);
    repo.git("config", "pager.status", command);
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    await adapter.getStatus({});
    await adapter.getDiff({ scope: "working" });
    await assertMarkerAbsent(markerPath);
  });

  it("does not execute an external diff configuration", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "hello\n");
    repo.git("add", "a.txt");
    repo.commit("initial");
    await repo.write("a.txt", "world\n");
    const markerPath = await installMarker(repo);
    repo.git("config", "diff.external", quotedNodeCommand(join(repo.root, "marker.cjs")));
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getDiff({ scope: "working" });
    expect(result.patch).toContain("diff --git");
    await assertMarkerAbsent(markerPath);
  });

  it("does not execute textconv configuration", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "hello\n");
    repo.git("add", "a.txt");
    repo.commit("initial");
    await repo.write("a.txt", "world\n");
    const markerPath = await installMarker(repo);
    repo.git("config", "diff.textexec.textconv", quotedNodeCommand(join(repo.root, "marker.cjs")));
    await repo.write(".gitattributes", "*.txt diff=textexec\n");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getDiff({ scope: "working" });
    expect(result.patch).toContain("diff --git");
    await assertMarkerAbsent(markerPath);
  });

  it("does not execute credential helpers or prompt for credentials", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "hello\n");
    repo.git("add", "a.txt");
    repo.commit("initial");
    const markerPath = await installMarker(repo);
    repo.git("config", "credential.helper", quotedNodeCommand(join(repo.root, "marker.cjs")));
    repo.git("config", "core.askPass", quotedNodeCommand(join(repo.root, "marker.cjs")));
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getStatus({});
    expect(result.repository).toBe(true);
    await assertMarkerAbsent(markerPath);
  });

  it("strips hostile environment-based Git configuration injection", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "hello\n");
    repo.git("add", "a.txt");
    repo.commit("initial");
    const markerPath = await installMarker(repo);
    const result = await runGitProcess({
      subcommand: "status",
      args: ["--porcelain=v2", "-z"],
      cwd: repo.root,
      environment: {
        GIT_CONFIG_COUNT: "1",
        GIT_CONFIG_KEY_0: "core.fsmonitor",
        GIT_CONFIG_VALUE_0: quotedNodeCommand(join(repo.root, "marker.cjs")),
        GIT_CONFIG_PARAMETERS: `'core.fsmonitor=${quotedNodeCommand(join(repo.root, "marker.cjs"))}'`,
        GIT_DIR: join(repo.root, ".git", "..", ".git"),
        GIT_WORK_TREE: tmpdir(),
        GIT_INDEX_FILE: join(repo.root, ".git", "index"),
        GIT_PAGER: quotedNodeCommand(join(repo.root, "marker.cjs")),
      },
      timeoutMs: 15_000,
      maxOutputBytes: 1024 * 1024,
    });
    expect(result.exitCode).toBe(0);
    await assertMarkerAbsent(markerPath);
  });

  it("rejects alias and fsmonitor overrides even when combined with later arguments", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "hello\n");
    repo.git("add", "a.txt");
    repo.commit("initial");
    const markerPath = await installMarker(repo);
    repo.git("config", "alias.status", `!${quotedNodeCommand(join(repo.root, "marker.cjs"))}`);
    repo.git("config", "core.fsmonitor", quotedNodeCommand(join(repo.root, "marker.cjs")));
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    await adapter.getStatus({});
    await adapter.getDiff({ scope: "working", paths: ["a.txt", "-c", "core.fsmonitor=node x"] });
    await assertMarkerAbsent(markerPath);
  });
});

describe("git environment sanitization", () => {
  it("removes every repository-redirecting and config-injecting variable", () => {
    const sanitized = sanitizeGitEnvironment({
      GIT_CONFIG_COUNT: "1",
      GIT_CONFIG_KEY_0: "core.fsmonitor",
      GIT_CONFIG_VALUE_0: "node marker.cjs",
      GIT_CONFIG_PARAMETERS: "'x=y'",
      GIT_CONFIG_NOSYSTEM: "0",
      GIT_DIR: "/evil",
      GIT_WORK_TREE: "/evil",
      GIT_INDEX_FILE: "/evil/index",
      GIT_OBJECT_DIRECTORY: "/evil/objects",
      GIT_ALTERNATE_OBJECT_DIRECTORIES: "/evil",
      GIT_COMMON_DIR: "/evil",
      GIT_NAMESPACE: "evil",
      GIT_ASKPASS: "/evil/askpass",
      SSH_ASKPASS: "/evil/askpass",
      GIT_SSH: "/evil/ssh",
      GIT_SSH_COMMAND: "evil",
      GIT_SSH_VARIANT: "evil",
      GIT_EXTERNAL_DIFF: "/evil/diff",
      PATH: "C:\\bin",
      KEEP_ME: "value",
    });
    expect(sanitized["KEEP_ME"]).toBe("value");
    for (const name of [
      "GIT_CONFIG_COUNT",
      "GIT_CONFIG_KEY_0",
      "GIT_CONFIG_VALUE_0",
      "GIT_CONFIG_PARAMETERS",
      "GIT_DIR",
      "GIT_WORK_TREE",
      "GIT_INDEX_FILE",
      "GIT_OBJECT_DIRECTORY",
      "GIT_ALTERNATE_OBJECT_DIRECTORIES",
      "GIT_COMMON_DIR",
      "GIT_NAMESPACE",
      "GIT_ASKPASS",
      "SSH_ASKPASS",
      "GIT_SSH",
      "GIT_SSH_COMMAND",
      "GIT_SSH_VARIANT",
    ]) {
      expect(sanitized[name]).toBeUndefined();
    }
    expect(sanitized).toMatchObject({
      GIT_TERMINAL_PROMPT: "0",
      GIT_OPTIONAL_LOCKS: "0",
      GIT_PAGER: "cat",
      PAGER: "cat",
      GIT_CONFIG_NOSYSTEM: "1",
      GIT_EXTERNAL_DIFF: "",
    });
  });

  it("sanitizes case-insensitively", () => {
    const sanitized = sanitizeGitEnvironment({ git_dir: "/evil", path: "C:\\bin" });
    expect(sanitized["git_dir"]).toBeUndefined();
    expect(sanitized["path"]).toBe("C:\\bin");
  });
});

describe("git special filenames", () => {
  it("reports unusual filenames exactly", async () => {
    const repo = await createTempRepo();
    const names: string[] = ["sp ace.txt", "uni-\u00e9\u4e2d.txt"];
    if (process.platform !== "win32") {
      names.push('quote"name.txt');
    }
    for (const name of names) {
      await repo.write(name, "hello\n");
    }
    repo.git("add", ".");
    repo.commit("initial");
    await repo.write("sp ace.txt", "world\n");
    await repo.write("uni-\u00e9\u4e2d.txt", "world\n");
    if (process.platform !== "win32") {
      await repo.write('quote"name.txt', "world\n");
    }
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getStatus({});
    const paths = result.changes.map((change) => change.path);
    expect(paths).toContain("sp ace.txt");
    expect(paths).toContain("uni-\u00e9\u4e2d.txt");
    if (process.platform !== "win32") {
      expect(paths).toContain('quote"name.txt');
    }
  });

  it("handles backslash and tab filenames on supported platforms", async () => {
    const repo = await createTempRepo();
    const names: string[] = [];
    if (process.platform !== "win32") {
      names.push("back\\slash.txt");
      names.push("tab\tname.txt");
      names.push("new\nline.txt");
    }
    for (const name of names) {
      await writeFile(`${repo.root}/${name}`, "hello\n");
    }
    if (names.length === 0) {
      await repo.write("plain.txt", "hello\n");
    }
    repo.git("add", ".");
    repo.commit("initial");
    if (names.length > 0) {
      await writeFile(`${repo.root}/${names[0] as string}`, "world\n");
    } else {
      await repo.write("plain.txt", "world\n");
    }
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getStatus({});
    const paths = result.changes.map((change) => change.path);
    if (process.platform !== "win32") {
      expect(paths).toContain("back\\slash.txt");
      expect(paths).toContain("tab\tname.txt");
      expect(paths).toContain("new\nline.txt");
    } else {
      expect(paths).toContain("plain.txt");
    }
  });
});

describe("git configuration isolation", () => {
  it("never reads configuration from the Solaris repository", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "hello\n");
    repo.git("add", "a.txt");
    repo.commit("initial");
    const markerPath = await installMarker(repo);
    const outside = await mkdtemp(join(tmpdir(), "solaris-git-config-"));
    try {
      const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
      await adapter.getStatus({});
      await adapter.getDiff({ scope: "working" });
      await assertMarkerAbsent(markerPath);
      expect(outside).toBeTruthy();
    } finally {
      const { rm } = await import("node:fs/promises");
      await rm(outside, { recursive: true, force: true });
    }
  });
});
