import { readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createGitCliAdapter } from "./git-cli-adapter.js";
import {
  cleanupTempDirs,
  createTempRepo,
  createTestRunDirectories,
  type TempRepo,
} from "./git-test-support.js";
import { completedResult, createFakeSandboxBackend } from "../../sandbox/fake-sandbox-backend.js";
import { createAnthropicSandboxRuntimeBackend } from "../../sandbox/anthropic-runtime/anthropic-sandbox-runtime-backend.js";

afterEach(async () => {
  await cleanupTempDirs();
});

/**
 * P0 regression: repository- and worktree-configured helper code (for
 * example `filter.<name>.clean` selected through `.gitattributes`) must
 * never execute on the host during Git inspection. The adapter NEVER spawns
 * Git (structurally enforced by the architecture check); it requests
 * sandboxed execution whose confinement is asserted below, and it reports
 * Git inspection unavailable when no enforcing backend exists. Live
 * proof-of-non-execution runs only against a real enforcing backend.
 */
async function installCleanFilterMarker(
  repo: TempRepo,
): Promise<{ markerPath: string; filterCommand: string }> {
  const markerPath = join(repo.root, "host-marker-ran.txt");
  const scriptPath = join(repo.root, "filter-marker.cjs");
  const script = `"use strict";
const fs = require("node:fs");
fs.writeFileSync(${JSON.stringify(markerPath)}, "ran\\n");
process.stdin.pipe(process.stdout);
`;
  await writeFile(scriptPath, script);
  const filterCommand = `node ${JSON.stringify(scriptPath)}`;
  return { markerPath, filterCommand };
}

/** Racy-clean index: file content changed but the mtime is restored, so a
 * `git status` index refresh re-hashes the working tree through clean
 * filters. */
async function makeRacyClean(repo: TempRepo, relativePath: string, content: string): Promise<void> {
  const { stat } = await import("node:fs/promises");
  const absolute = join(repo.root, relativePath);
  const before = await stat(absolute);
  await writeFile(absolute, content);
  const { utimes } = await import("node:fs/promises");
  await utimes(absolute, before.atime, before.mtime);
}

describe("git inspection helper-execution confinement", () => {
  it("requests sandboxed confinement when the repository selects a clean filter", async () => {
    const repo = await createTempRepo();
    await repo.write("data.txt", "original\n");
    repo.git("add", "data.txt");
    repo.commit("initial");
    const { filterCommand, markerPath } = await installCleanFilterMarker(repo);
    repo.git("config", `filter.evil.clean`, filterCommand);
    await repo.write(".gitattributes", "data.txt filter=evil\n");
    repo.git("add", ".gitattributes");
    repo.commit("attrs");
    await makeRacyClean(repo, "data.txt", "changed-content\n");
    // The test harness's own `git add` above may have executed the filter
    // on the host; clear its marker so the adapter phase is what is probed.
    const { rm } = await import("node:fs/promises");
    await rm(markerPath, { force: true });

    const { backend, requests } = createFakeSandboxBackend({
      results: [
        completedResult({ stdout: "git version 2.40.0\n" }),
        completedResult({ stdout: `${repo.root}\ntrue\n` }),
        completedResult({ stdout: "# branch.head main\n" }),
      ],
    });
    const runs = createTestRunDirectories();
    const adapter = createGitCliAdapter({
      workspaceRoot: repo.root,
      backend,
      runDirectories: runs.provider,
    });
    await adapter.getStatus({});

    // The request must carry mechanical confinement: network denied,
    // writes limited to the private run directory, reads limited to the
    // repository and Git runtime roots, and a minimal environment.
    expect(requests().length).toBeGreaterThan(0);
    for (const request of requests()) {
      expect(request.profile.network.outbound).toBe("deny");
      expect(request.profile.filesystem.workspaceAccess).toBe("read-only");
      expect(request.runDirectory).toBeTruthy();
      const readRoots = request.explicitReadRoots ?? [];
      expect(readRoots).toContain(repo.root);
      expect(request.environment["GIT_OPTIONAL_LOCKS"]).toBe("0");
      expect(request.environment["GIT_CONFIG_NOSYSTEM"]).toBe("1");
    }
    // The adapter itself never executed the filter: only the backend saw
    // the request, and the marker was never created on the host.
    await expect(readFile(markerPath, "utf8")).rejects.toThrow();
  });

  it("reports git inspection unavailable and never requests execution when the backend cannot enforce", async () => {
    const repo = await createTempRepo();
    await repo.write("data.txt", "original\n");
    repo.git("add", "data.txt");
    repo.commit("initial");
    const { filterCommand } = await installCleanFilterMarker(repo);
    repo.git("config", `filter.evil.clean`, filterCommand);
    await repo.write(".gitattributes", "data.txt filter=evil\n");
    repo.git("add", ".gitattributes");
    repo.commit("attrs");
    await makeRacyClean(repo, "data.txt", "changed-content\n");

    const { backend, requests } = createFakeSandboxBackend({
      status: {
        backendId: "weak",
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
    expect(runs.roots()).toHaveLength(0);
    await expect(adapter.getStatus({})).rejects.toMatchObject({ code: "git_unavailable" });
    expect(requests()).toHaveLength(0);
  });
});

describe("git inspection live confinement (real enforcing sandbox only)", () => {
  const liveBackend = createAnthropicSandboxRuntimeBackend({
    workspaceRoot: join(tmpdir(), "solaris-git-live-probe"),
  });

  it("never creates the clean-filter marker and never writes the repository", async () => {
    const status = await liveBackend.inspect().catch(() => null);
    const enforcing =
      status !== null &&
      status.state === "available" &&
      status.capabilities.filesystemReadRestriction &&
      status.capabilities.filesystemWriteRestriction &&
      status.capabilities.networkRestriction &&
      status.capabilities.processTreeRestriction;
    if (!enforcing) {
      console.log(
        `GIT LIVE CONFINEMENT: SKIPPED - no enforcing sandbox backend (state: ${status?.state ?? "unknown"}); host execution is prevented by the availability gate instead.`,
      );
      return;
    }
    const repo = await createTempRepo();
    await repo.write("data.txt", "original\n");
    repo.git("add", "data.txt");
    repo.commit("initial");
    const { markerPath } = await installCleanFilterMarker(repo);
    repo.git(
      "config",
      "filter.evil.clean",
      `node ${JSON.stringify(join(repo.root, "filter-marker.cjs"))}`,
    );
    await repo.write(".gitattributes", "data.txt filter=evil\n");
    repo.git("add", ".gitattributes");
    repo.commit("attrs");
    await makeRacyClean(repo, "data.txt", "changed-content\n");
    const snapshot = repo.git("status", "--porcelain=v2", "-z");

    const runs = createTestRunDirectories();
    const adapter = createGitCliAdapter({
      workspaceRoot: repo.root,
      backend: liveBackend,
      runDirectories: runs.provider,
    });
    const result = await adapter.getStatus({});
    expect(result.repository).toBe(true);
    // The clean filter may execute inside the sandbox, but it can never
    // write its marker to the host repository.
    await expect(readFile(markerPath, "utf8")).rejects.toThrow();
    // The repository must be byte-identical: no index or object writes.
    const after = repo.git("status", "--porcelain=v2", "-z");
    expect(after.stdout).toBe(snapshot.stdout);
  });
});
