import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { GitError } from "@solaris/core";
import { createGitCliAdapter } from "./git-cli-adapter.js";
import { runGitProcess } from "./git-process.js";
import { parseNumstatDiff } from "./diff-parser.js";
import { parseBranchFromTruncatedOutput } from "./status-parser.js";
import { cleanupTempDirs, createTempRepo, type TempRepo } from "./git-test-support.js";

afterEach(async () => {
  await cleanupTempDirs();
});

function commitAll(repo: TempRepo, message: string): void {
  repo.git("add", "-A");
  repo.commit(message);
}

describe("git status inspection", () => {
  it("reports a clean repository", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "one\n");
    commitAll(repo, "initial");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const status = await adapter.getStatus({});
    expect(status.repository).toBe(true);
    expect(status.changes).toEqual([]);
    expect(status.untracked).toEqual([]);
    expect(status.conflicts).toEqual([]);
    expect(status.truncated).toBe(false);
    expect(status.branch).toMatchObject({ head: "main", detached: false, unborn: false });
  });

  it("reports a modified tracked file", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "one\n");
    commitAll(repo, "initial");
    await repo.write("a.txt", "two\n");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const status = await adapter.getStatus({});
    expect(status.changes).toEqual([
      {
        path: "a.txt",
        originalPath: null,
        indexStatus: "unmodified",
        worktreeStatus: "modified",
        kind: "ordinary",
      },
    ]);
  });

  it("reports a staged file", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "one\n");
    commitAll(repo, "initial");
    await repo.write("a.txt", "two\n");
    repo.git("add", "a.txt");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const status = await adapter.getStatus({});
    expect(status.changes[0]).toMatchObject({
      path: "a.txt",
      indexStatus: "modified",
      worktreeStatus: "unmodified",
    });
  });

  it("reports staged and unstaged changes separately", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "one\n");
    commitAll(repo, "initial");
    await repo.write("a.txt", "two\n");
    repo.git("add", "a.txt");
    await repo.write("a.txt", "three\n");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const status = await adapter.getStatus({});
    expect(status.changes[0]).toMatchObject({
      indexStatus: "modified",
      worktreeStatus: "modified",
    });
  });

  it("reports untracked files", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "one\n");
    commitAll(repo, "initial");
    await repo.write("new file.txt", "x\n");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const status = await adapter.getStatus({});
    expect(status.untracked).toEqual(["new file.txt"]);
  });

  it("reports a deleted file", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "one\n");
    commitAll(repo, "initial");
    const { rm } = await import("node:fs/promises");
    await rm(join(repo.root, "a.txt"));
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const status = await adapter.getStatus({});
    expect(status.changes[0]).toMatchObject({
      path: "a.txt",
      worktreeStatus: "deleted",
    });
  });

  it("reports a renamed file", async () => {
    const repo = await createTempRepo();
    await repo.write("old.txt", "one\n");
    commitAll(repo, "initial");
    repo.git("mv", "old.txt", "new.txt");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const status = await adapter.getStatus({});
    expect(status.changes[0]).toMatchObject({
      path: "new.txt",
      originalPath: "old.txt",
      indexStatus: "renamed",
      kind: "renamed",
    });
  });

  it("reports a merge conflict", async () => {
    const repo = await createTempRepo();
    await repo.write("conflict.txt", "base\n");
    commitAll(repo, "base");
    repo.git("checkout", "-b", "other");
    await repo.write("conflict.txt", "other\n");
    commitAll(repo, "other change");
    repo.git("checkout", "main");
    await repo.write("conflict.txt", "main\n");
    commitAll(repo, "main change");
    const merge = repo.git("merge", "other");
    expect(merge.status).not.toBe(0);
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const status = await adapter.getStatus({});
    expect(status.conflicts.length).toBeGreaterThan(0);
    expect(status.conflicts[0]?.path).toBe("conflict.txt");
  });

  it("handles spaces, tabs, and unicode in filenames", async () => {
    const repo = await createTempRepo();
    await repo.write("with space.txt", "one\n");
    await repo.write("unicodé.txt", "two\n");
    const tabSupported = process.platform !== "win32";
    if (tabSupported) {
      await repo.write("with\ttab.txt", "tab\n");
    }
    commitAll(repo, "initial");
    await repo.write("with space.txt", "changed\n");
    await repo.write("unicodé.txt", "changed\n");
    if (tabSupported) {
      await repo.write("with\ttab.txt", "changed\n");
    }
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const status = await adapter.getStatus({});
    const paths = status.changes.map((change) => change.path).sort();
    const expected = tabSupported
      ? ["unicodé.txt", "with space.txt", "with\ttab.txt"].sort()
      : ["unicodé.txt", "with space.txt"].sort();
    expect(paths).toEqual(expected);
  });

  it("reports ahead and behind against an upstream", async () => {
    const originDir = join(process.env.TEMP ?? ".", `solaris-git-origin-${Date.now()}`);
    const { mkdir } = await import("node:fs/promises");
    await mkdir(originDir, { recursive: true });
    const origin = await createTempRepo();
    await origin.write("a.txt", "one\n");
    commitAll(origin, "initial");
    const repo = await createTempRepo();
    const clone = repo.git("clone", origin.root, join(repo.root, "clone"));
    expect(clone.status).toBe(0);
    const cloneRoot = join(repo.root, "clone");
    const { writeFile } = await import("node:fs/promises");
    await writeFile(join(cloneRoot, "a.txt"), "two\n");
    const { spawnSync } = await import("node:child_process");
    spawnSync("git", ["-c", "user.name=t", "-c", "user.email=t@t", "add", "-A"], {
      cwd: cloneRoot,
    });
    spawnSync("git", ["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-m", "ahead"], {
      cwd: cloneRoot,
    });
    const adapter = createGitCliAdapter({ workspaceRoot: cloneRoot });
    const status = await adapter.getStatus({});
    expect(status.branch.upstream).toBeDefined();
    expect(status.branch.ahead).toBe(1);
  });

  it("reports no upstream when none is configured", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "one\n");
    commitAll(repo, "initial");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const status = await adapter.getStatus({});
    expect(status.branch.upstream).toBeNull();
    expect(status.branch.ahead).toBeNull();
    expect(status.branch.behind).toBeNull();
  });

  it("fails on malformed porcelain data", async () => {
    const { parsePorcelainV2 } = await import("./status-parser.js");
    expect(() => parsePorcelainV2("### nonsense\n")).toThrow(GitError);
  });

  it("returns truncated status without throwing when output is cut mid-record", async () => {
    const repo = await createTempRepo();
    for (let i = 0; i < 300; i += 1) {
      await repo.write(`untracked-file-${i}-with-a-reasonably-long-name.txt`, "x\n");
    }
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root, maxOutputBytes: 1024 });
    const status = await adapter.getStatus({});
    expect(status.repository).toBe(true);
    expect(status.truncated).toBe(true);
    expect(typeof status.branch.head).toBe("string");
  });
});

describe("git numstat parser", () => {
  it("keeps full paths that contain tabs", () => {
    const parsed = parseNumstatDiff("1\t1\ttab\tname.txt");
    expect(parsed).toEqual({
      files: [
        {
          path: "tab\tname.txt",
          originalPath: null,
          operation: "modify",
          addedLines: 1,
          removedLines: 1,
          binary: false,
        },
      ],
      truncated: false,
    });
  });

  it("keeps full rename paths that contain tabs", () => {
    const parsed = parseNumstatDiff("1\t0\t\0old\tname.txt\0new\tname.txt");
    expect(parsed.files).toEqual([
      {
        path: "new\tname.txt",
        originalPath: "old\tname.txt",
        operation: "rename",
        addedLines: 1,
        removedLines: 0,
        binary: false,
      },
    ]);
  });

  it("preserves basic rename records", () => {
    const parsed = parseNumstatDiff("1\t0\t\0old.txt\0new.txt");
    expect(parsed.files[0]).toMatchObject({
      path: "new.txt",
      originalPath: "old.txt",
      operation: "rename",
      addedLines: 1,
      removedLines: 0,
    });
  });

  it("fails with a structured error on malformed rename records", () => {
    let caught: unknown;
    try {
      parseNumstatDiff("1\t0\t");
    } catch (error: unknown) {
      caught = error;
    }
    expect(caught).toBeInstanceOf(GitError);
    expect((caught as GitError).code).toBe("git_parse_failed");
  });
});

describe("truncated output recovery", () => {
  it("recovers intact branch records from truncated status output", () => {
    const branch = parseBranchFromTruncatedOutput(
      "# branch.oid abc123def\0# branch.head main\0# branch.ab +2 -1\0? trun",
    );
    expect(branch).toEqual({
      head: "main",
      oid: "abc123def",
      upstream: null,
      ahead: 2,
      behind: -1,
      detached: false,
      unborn: false,
    });
  });

  it("defaults to a detached-HEAD-shaped branch when no records survive", () => {
    expect(parseBranchFromTruncatedOutput("? trun")).toEqual({
      head: "HEAD",
      oid: null,
      upstream: null,
      ahead: null,
      behind: null,
      detached: false,
      unborn: false,
    });
  });
});

describe("git diff inspection", () => {
  it("returns a working diff", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "one\ntwo\n");
    commitAll(repo, "initial");
    await repo.write("a.txt", "one\nchanged\n");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getDiff({ scope: "working" });
    expect(result.scope).toBe("working");
    expect(result.files).toEqual([
      {
        path: "a.txt",
        originalPath: null,
        operation: "modify",
        addedLines: 1,
        removedLines: 1,
        binary: false,
      },
    ]);
    expect(result.patch).toContain("-two");
    expect(result.patch).toContain("+changed");
  });

  it("returns a staged diff", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "one\n");
    commitAll(repo, "initial");
    await repo.write("a.txt", "two\n");
    repo.git("add", "a.txt");
    await repo.write("a.txt", "three\n");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const staged = await adapter.getDiff({ scope: "staged" });
    expect(staged.patch).toContain("+two");
    expect(staged.patch).not.toContain("+three");
    const working = await adapter.getDiff({ scope: "working" });
    expect(working.patch).toContain("+three");
  });

  it("returns an empty diff for a clean repository", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "one\n");
    commitAll(repo, "initial");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getDiff({ scope: "working" });
    expect(result.files).toEqual([]);
    expect(result.patch).toBe("");
  });

  it("handles repositories with no HEAD for the head scope", async () => {
    const repo = await createTempRepo();
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getDiff({ scope: "head" });
    expect(result.files).toEqual([]);
    expect(result.patch).toBe("");
  });

  it("filters by path", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "one\n");
    await repo.write("b.txt", "one\n");
    commitAll(repo, "initial");
    await repo.write("a.txt", "two\n");
    await repo.write("b.txt", "two\n");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getDiff({ scope: "working", paths: ["a.txt"] });
    expect(result.files.map((file) => file.path)).toEqual(["a.txt"]);
  });

  it("rejects invalid relative paths", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "one\n");
    commitAll(repo, "initial");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    await expect(
      adapter.getDiff({ scope: "working", paths: ["../outside"] }),
    ).rejects.toMatchObject({
      code: "git_diff_failed",
    });
  });

  it("treats pathspec-like input as a literal path", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "one\n");
    commitAll(repo, "initial");
    await repo.write("a.txt", "two\n");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getDiff({ scope: "working", paths: ["*.txt"] });
    expect(result.files).toEqual([]);
  });

  it("summarizes binary changes without patch content", async () => {
    const repo = await createTempRepo();
    await repo.write("bin.dat", "abc");
    commitAll(repo, "initial");
    const { writeFile } = await import("node:fs/promises");
    await writeFile(join(repo.root, "bin.dat"), Buffer.from([0x00, 0x01, 0x02, 0x03]));
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getDiff({ scope: "working" });
    expect(result.files[0]).toMatchObject({ path: "bin.dat", binary: true });
    expect(result.patch).not.toContain("\\x00");
  });

  it("summarizes renames", async () => {
    const repo = await createTempRepo();
    await repo.write("old.txt", "one\n");
    commitAll(repo, "initial");
    repo.git("mv", "old.txt", "new.txt");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getDiff({ scope: "staged" });
    expect(result.files[0]).toMatchObject({
      path: "new.txt",
      originalPath: "old.txt",
      operation: "rename",
    });
  });

  it("marks truncated output", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "x\n".repeat(2000));
    commitAll(repo, "initial");
    await repo.write("a.txt", "y\n".repeat(2000));
    const adapter = createGitCliAdapter({
      workspaceRoot: repo.root,
      maxOutputBytes: 1024,
    });
    const result = await adapter.getDiff({ scope: "working" });
    expect(result.truncated).toBe(true);
  });

  it("returns a truncated diff without throwing when numstat is cut mid-record", async () => {
    const repo = await createTempRepo();
    for (let i = 0; i < 300; i += 1) {
      await repo.write(`file-${i}.txt`, "one\n");
    }
    commitAll(repo, "initial");
    for (let i = 0; i < 300; i += 1) {
      await repo.write(`file-${i}.txt`, "two\n");
    }
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root, maxOutputBytes: 1024 });
    const result = await adapter.getDiff({ scope: "working" });
    expect(result.truncated).toBe(true);
    expect(typeof result.patch).toBe("string");
  });

  it("ignores external diff helpers and textconv configuration", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "one\n");
    commitAll(repo, "initial");
    repo.git("config", "diff.external", "echo EXTERNAL-DIFF-RAN");
    repo.git("config", "diff.a.textconv", "echo TEXTCONV-RAN");
    await repo.write("a.txt", "two\n");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getDiff({ scope: "working" });
    expect(result.patch).toContain("diff --git");
    expect(result.patch).not.toContain("EXTERNAL-DIFF-RAN");
    expect(result.patch).not.toContain("TEXTCONV-RAN");
  });

  it("does not include untracked file contents", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "one\n");
    commitAll(repo, "initial");
    await repo.write("untracked.txt", "secret untracked content\n");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getDiff({ scope: "working" });
    expect(result.files).toEqual([]);
    expect(result.patch).toBe("");
    expect(result.untrackedExcluded).toBe(true);
  });

  it("reports exact paths for special filenames in diffs", async () => {
    const repo = await createTempRepo();
    const names: string[] = ["sp ace.txt", "uni-\u00e9\u4e2d.txt"];
    if (process.platform !== "win32") {
      names.push('quote"name.txt', "back\\slash.txt", "tab\tname.txt", "new\nline.txt");
    }
    for (const name of names) {
      await repo.write(name, "one\n");
    }
    commitAll(repo, "initial");
    for (const name of names) {
      await repo.write(name, "two\n");
    }
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getDiff({ scope: "working" });
    const paths = result.files.map((file) => file.path);
    for (const name of names) {
      expect(paths).toContain(name);
    }
  });

  it(
    "classifies tracked type changes as modified",
    { skip: process.platform === "win32" },
    async () => {
      const repo = await createTempRepo();
      await repo.write("t.txt", "x\n");
      commitAll(repo, "initial");
      const { rm, symlink } = await import("node:fs/promises");
      await rm(join(repo.root, "t.txt"));
      await symlink("somewhere", join(repo.root, "t.txt"));
      const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
      const result = await adapter.getStatus({});
      const change = result.changes.find((entry) => entry.path === "t.txt");
      expect(change).toBeDefined();
      expect(change?.worktreeStatus).toBe("modified");
    },
  );

  it("decodes truncated output as valid UTF-8 without splitting sequences", async () => {
    const repo = await createTempRepo();
    await repo.write("uni-\u00e9\u4e2d.txt", "one\n");
    commitAll(repo, "initial");
    const { writeFile } = await import("node:fs/promises");
    await writeFile(join(repo.root, "uni-\u00e9\u4e2d.txt"), "\u00e9".repeat(4000));
    const result = await runGitProcess({
      subcommand: "diff",
      args: ["--no-ext-diff", "--no-textconv", "--no-color", "--", "uni-\u00e9\u4e2d.txt"],
      cwd: repo.root,
      environment: {},
      timeoutMs: 15_000,
      maxOutputBytes: 997,
    });
    expect(result.exitCode).toBe(0);
    expect(result.stdoutTruncated).toBe(true);
    expect(Buffer.byteLength(result.stdout, "utf8")).toBeLessThanOrEqual(997);
    const fatal = new TextDecoder("utf-8", { fatal: true });
    expect(() => fatal.decode(Buffer.from(result.stdout, "utf8"))).not.toThrow();
  });

  it("preserves rename counts from numstat records", async () => {
    const repo = await createTempRepo();
    await repo.write("old.txt", "one\ntwo\n");
    commitAll(repo, "initial");
    await repo.write("new.txt", "one\ntwo\nthree\n");
    repo.git("rm", "-q", "old.txt");
    repo.git("add", "new.txt");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getDiff({ scope: "staged" });
    const renamed = result.files.find((file) => file.operation === "rename");
    expect(renamed).toMatchObject({
      path: "new.txt",
      originalPath: "old.txt",
      addedLines: 1,
      removedLines: 0,
    });
  });
});
