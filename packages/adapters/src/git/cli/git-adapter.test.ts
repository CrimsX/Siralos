import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { GIT_SAFETY_ENVIRONMENT, runGitProcess } from "./git-process.js";
import { createGitCliAdapter } from "./git-cli-adapter.js";
import { cleanupTempDirs, createNonGitDir, createTempRepo } from "./git-test-support.js";

afterEach(async () => {
  await cleanupTempDirs();
});

describe("git process runner", () => {
  it("rejects subcommands outside the allowlist", async () => {
    const directory = await createNonGitDir();
    await expect(
      runGitProcess({
        subcommand: "reset",
        args: ["--hard"],
        cwd: directory,
        environment: {},
        timeoutMs: 5000,
        maxOutputBytes: 1024,
      }),
    ).rejects.toMatchObject({ code: "git_status_failed" });
  });

  it("does not execute shell metacharacters in arguments", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "hello\n");
    repo.git("add", "a.txt");
    repo.commit("initial");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const result = await adapter.getDiff({
      scope: "working",
      paths: ["a.txt; touch pwned.txt"],
    });
    expect(result.files).toEqual([]);
    const { readFile } = await import("node:fs/promises");
    await expect(readFile(join(repo.root, "pwned.txt"))).rejects.toThrow();
  });

  it("times out long-running git commands", async () => {
    const directory = await createNonGitDir();
    await expect(
      runGitProcess({
        subcommand: "version",
        args: [],
        cwd: directory,
        environment: {},
        timeoutMs: 1,
        maxOutputBytes: 1024,
      }),
    ).rejects.toMatchObject({ code: "git_timeout" });
  });

  it("cancels running git commands", async () => {
    const directory = await createNonGitDir();
    const controller = new AbortController();
    const promise = runGitProcess({
      subcommand: "version",
      args: [],
      cwd: directory,
      environment: {},
      timeoutMs: 60_000,
      maxOutputBytes: 1024,
      signal: controller.signal,
    });
    controller.abort();
    await expect(promise).rejects.toMatchObject({ code: "git_cancelled" });
  });

  it("defines the required safety environment", () => {
    expect(GIT_SAFETY_ENVIRONMENT).toMatchObject({
      GIT_TERMINAL_PROMPT: "0",
      GIT_OPTIONAL_LOCKS: "0",
      GIT_PAGER: "cat",
      PAGER: "cat",
      LC_ALL: "C",
      LANG: "C",
    });
  });
});

describe("git repository detection", () => {
  it("detects an available git executable and a matching repository", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "hello\n");
    repo.git("add", "a.txt");
    repo.commit("initial");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const status = await adapter.inspectRepository();
    expect(status.gitAvailable).toBe(true);
    expect(status.gitVersion).toMatch(/^\d+\.\d+/);
    expect(status.repositoryState).toBe("repository");
  });

  it("reports a non-Git workspace", async () => {
    const directory = await createNonGitDir();
    const adapter = createGitCliAdapter({ workspaceRoot: directory });
    const status = await adapter.inspectRepository();
    expect(status).toMatchObject({
      gitAvailable: true,
      repositoryState: "not_repository",
      repositoryRoot: null,
    });
  });

  it("rejects a repository root that is a parent of the workspace", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "hello\n");
    repo.git("add", "a.txt");
    repo.commit("initial");
    const nested = join(repo.root, "nested");
    const { mkdir } = await import("node:fs/promises");
    await mkdir(nested);
    const adapter = createGitCliAdapter({ workspaceRoot: nested });
    const status = await adapter.inspectRepository();
    expect(status.repositoryState).toBe("root_mismatch");
  });

  it("reports a detached HEAD", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "one\n");
    repo.git("add", "a.txt");
    repo.commit("first");
    await repo.write("a.txt", "two\n");
    repo.git("add", "a.txt");
    repo.commit("second");
    repo.git("checkout", "HEAD~1");
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const status = await adapter.getStatus({});
    expect(status.branch.detached).toBe(true);
  });

  it("reports an unborn branch", async () => {
    const repo = await createTempRepo();
    const adapter = createGitCliAdapter({ workspaceRoot: repo.root });
    const status = await adapter.getStatus({});
    expect(status.branch.unborn).toBe(true);
    expect(status.branch.head).toBe("main");
  });

  it("reports git as unavailable when the executable is missing", async () => {
    const directory = await createNonGitDir();
    const adapter = createGitCliAdapter({
      workspaceRoot: directory,
    });
    const original = process.env.PATH;
    process.env.PATH = "C:\\definitely-missing-path-xyz";
    try {
      const status = await adapter.inspectRepository();
      expect(status.gitAvailable).toBe(false);
      expect(status.repositoryState).toBe("unavailable");
    } finally {
      process.env.PATH = original;
    }
  });

  it("reports git errors through the inspector", async () => {
    const directory = await createNonGitDir();
    const adapter = createGitCliAdapter({ workspaceRoot: directory });
    await expect(adapter.getStatus({})).rejects.toMatchObject({
      code: "git_not_repository",
    });
    await expect(adapter.getDiff({ scope: "working" })).rejects.toMatchObject({
      code: "git_not_repository",
    });
  });
});
