import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { GIT_SAFETY_ENVIRONMENT, buildGitInvocation } from "./git-process.js";
import { createGitCliAdapter } from "./git-cli-adapter.js";
import {
  cleanupTempDirs,
  createHostGitBackend,
  createNonGitDir,
  createTempRepo,
  createTestRunDirectories,
} from "./git-test-support.js";
import { completedResult, createFakeSandboxBackend } from "../../sandbox/fake-sandbox-backend.js";

afterEach(async () => {
  await cleanupTempDirs();
});

function createAdapter(workspaceRoot: string) {
  const git = createHostGitBackend({ workspaceRoot });
  const runs = createTestRunDirectories();
  const adapter = createGitCliAdapter({
    workspaceRoot,
    backend: git.backend,
    runDirectories: runs.provider,
  });
  return { adapter, requests: git.requests };
}

describe("git invocation construction", () => {
  it("rejects subcommands outside the allowlist", () => {
    expect(() => buildGitInvocation("reset", ["--hard"])).toThrowError(
      expect.objectContaining({ code: "git_status_failed" }),
    );
  });

  it("builds the fixed invocation with disabling configuration first", () => {
    const args = buildGitInvocation("status", ["--porcelain=v2", "-z"]);
    expect(args[0]).toBe("-c");
    expect(args).toContain("core.fsmonitor=false");
    expect(args).toContain("alias.status=status");
    expect(args).toContain("--no-pager");
    expect(args).toContain("--literal-pathspecs");
    expect(args).toContain("status");
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

describe("git sandboxed execution", () => {
  it("runs every invocation through the sandbox backend with confinement", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "hello\n");
    repo.git("add", "a.txt");
    repo.commit("initial");
    const { adapter, requests } = createAdapter(repo.root);
    await adapter.getDiff({ scope: "working", paths: ["a.txt"] });
    expect(requests().length).toBeGreaterThan(0);
    for (const request of requests()) {
      expect(request.profile.network.outbound).toBe("deny");
      expect(request.profile.filesystem.workspaceAccess).toBe("read-only");
      expect(request.runDirectory).toBeTruthy();
      const readRoots = request.explicitReadRoots ?? [];
      expect(readRoots).toContain(repo.root);
      expect(request.environment["GIT_TERMINAL_PROMPT"]).toBe("0");
      expect(request.environment["GIT_CONFIG_NOSYSTEM"]).toBe("1");
    }
  });

  it("does not execute shell metacharacters in arguments", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "hello\n");
    repo.git("add", "a.txt");
    repo.commit("initial");
    const { adapter } = createAdapter(repo.root);
    const result = await adapter.getDiff({
      scope: "working",
      paths: ["a.txt; touch pwned.txt"],
    });
    expect(result.files).toEqual([]);
    const { readFile } = await import("node:fs/promises");
    await expect(readFile(join(repo.root, "pwned.txt"))).rejects.toThrow();
  });

  it("maps a backend timeout to git_timeout", async () => {
    const repo = await createTempRepo();
    const { backend, requests } = createFakeSandboxBackend({
      results: [{ ...completedResult(), status: "timed-out", exitCode: null }],
    });
    const runs = createTestRunDirectories();
    const adapter = createGitCliAdapter({
      workspaceRoot: repo.root,
      backend,
      runDirectories: runs.provider,
    });
    await expect(adapter.getStatus({})).rejects.toMatchObject({ code: "git_timeout" });
    expect(requests().length).toBeGreaterThan(0);
  });

  it("maps a backend cancellation to git_cancelled", async () => {
    const repo = await createTempRepo();
    const { backend } = createFakeSandboxBackend({
      results: [{ ...completedResult(), status: "cancelled", exitCode: null }],
    });
    const runs = createTestRunDirectories();
    const adapter = createGitCliAdapter({
      workspaceRoot: repo.root,
      backend,
      runDirectories: runs.provider,
    });
    await expect(adapter.getStatus({})).rejects.toMatchObject({ code: "git_cancelled" });
  });

  it("reports git inspection unavailable when the backend cannot enforce the boundary", async () => {
    const repo = await createTempRepo();
    const { backend, requests } = createFakeSandboxBackend({
      status: {
        backendId: "weak",
        state: "available",
        platform: "linux",
        version: "0.0.0",
        capabilities: {
          filesystemReadRestriction: true,
          filesystemWriteRestriction: true,
          networkRestriction: true,
          processTreeRestriction: false,
          violationReporting: false,
        },
      },
    });
    const runs = createTestRunDirectories();
    const adapter = createGitCliAdapter({
      workspaceRoot: repo.root,
      backend,
      runDirectories: runs.provider,
    });
    const status = await adapter.inspectRepository();
    expect(status.gitAvailable).toBe(false);
    expect(status.repositoryState).toBe("unavailable");
    expect(status.message).toContain("never spawned outside the sandbox");
    // Git must never be executed: no backend request, no run directory.
    expect(requests()).toHaveLength(0);
    expect(runs.roots()).toHaveLength(0);
  });

  it("reports git inspection unavailable when the backend is not available", async () => {
    const repo = await createTempRepo();
    const { backend, requests } = createFakeSandboxBackend({
      status: {
        backendId: "unset",
        state: "setup-required",
        platform: "windows",
        version: "0.0.0",
        capabilities: {
          filesystemReadRestriction: false,
          filesystemWriteRestriction: false,
          networkRestriction: false,
          processTreeRestriction: false,
          violationReporting: false,
        },
      },
    });
    const runs = createTestRunDirectories();
    const adapter = createGitCliAdapter({
      workspaceRoot: repo.root,
      backend,
      runDirectories: runs.provider,
    });
    const status = await adapter.inspectRepository();
    expect(status.gitAvailable).toBe(false);
    expect(requests()).toHaveLength(0);
    await expect(adapter.getStatus({})).rejects.toMatchObject({ code: "git_unavailable" });
    expect(requests()).toHaveLength(0);
  });

  it("reports git inspection unavailable when the backend cannot be inspected", async () => {
    const repo = await createTempRepo();
    const { backend, requests } = createFakeSandboxBackend({ inspectError: new Error("boom") });
    const runs = createTestRunDirectories();
    const adapter = createGitCliAdapter({
      workspaceRoot: repo.root,
      backend,
      runDirectories: runs.provider,
    });
    const status = await adapter.inspectRepository();
    expect(status.gitAvailable).toBe(false);
    expect(requests()).toHaveLength(0);
  });
});

describe("git repository detection", () => {
  it("detects an available git executable and a matching repository", async () => {
    const repo = await createTempRepo();
    await repo.write("a.txt", "hello\n");
    repo.git("add", "a.txt");
    repo.commit("initial");
    const { adapter } = createAdapter(repo.root);
    const status = await adapter.inspectRepository();
    expect(status.gitAvailable).toBe(true);
    expect(status.gitVersion).toMatch(/^\d+\.\d+/);
    expect(status.repositoryState).toBe("repository");
  });

  it("reports a non-Git workspace", async () => {
    const directory = await createNonGitDir();
    const { adapter } = createAdapter(directory);
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
    const { adapter } = createAdapter(nested);
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
    const { adapter } = createAdapter(repo.root);
    const status = await adapter.getStatus({});
    expect(status.branch.detached).toBe(true);
  });

  it("reports an unborn branch", async () => {
    const repo = await createTempRepo();
    const { adapter } = createAdapter(repo.root);
    const status = await adapter.getStatus({});
    expect(status.branch.unborn).toBe(true);
    expect(status.branch.head).toBe("main");
  });

  it("reports git as unavailable when the executable is missing", async () => {
    const directory = await createNonGitDir();
    const git = createHostGitBackend({ workspaceRoot: directory });
    const runs = createTestRunDirectories();
    const adapter = createGitCliAdapter({
      workspaceRoot: directory,
      backend: git.backend,
      runDirectories: runs.provider,
      gitExecutable: join(directory, "definitely-missing-git"),
    });
    const status = await adapter.inspectRepository();
    expect(status.gitAvailable).toBe(false);
    expect(status.repositoryState).toBe("unavailable");
  });

  it("reports git errors through the inspector", async () => {
    const directory = await createNonGitDir();
    const { adapter } = createAdapter(directory);
    await expect(adapter.getStatus({})).rejects.toMatchObject({
      code: "git_not_repository",
    });
    await expect(adapter.getDiff({ scope: "working" })).rejects.toMatchObject({
      code: "git_not_repository",
    });
  });
});
