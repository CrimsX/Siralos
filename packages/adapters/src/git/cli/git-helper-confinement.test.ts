import { readFile, rm } from "node:fs/promises";
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
 *
 * HARD RULE: after the hostile filter is configured, NO host Git is ever
 * invoked again — every setup step that needs Git runs before the filter
 * exists, and all state comparison uses direct bounded filesystem reads
 * that cannot invoke Git helpers.
 */

/** The malicious clean-filter script attempts a host marker write and an
 * outbound network connection; every failure is swallowed so the filter
 * passes data through and `git status` can complete inside the sandbox. */
function filterScriptSource(markerPath: string): string {
  return `"use strict";
const fs = require("node:fs");
const net = require("node:net");
function mark(text) {
  try {
    fs.writeFileSync(${JSON.stringify(markerPath)}, text + "\\n");
  } catch (error) {
    // the marker write is denied by the sandbox; never let the filter fail
  }
}
mark("ran");
const socket = net.connect({ host: "1.1.1.1", port: 53 });
socket.setTimeout(1500);
socket.once("connect", function () {
  socket.destroy();
  mark("net-ok");
});
socket.once("error", function () {
  socket.destroy();
  mark("net-denied");
});
socket.once("timeout", function () {
  socket.destroy();
  mark("net-timeout");
});
process.stdin.pipe(process.stdout);
`;
}

function installCleanFilterMarker(repo: TempRepo): {
  markerPath: string;
  scriptPath: string;
  filterCommand: string;
} {
  const markerPath = join(repo.root, "host-marker-ran.txt");
  const scriptPath = join(repo.root, "filter-marker.cjs");
  return { markerPath, scriptPath, filterCommand: `node ${JSON.stringify(scriptPath)}` };
}

/** Racy-clean index: file content changed but the mtime is restored, so a
 * `git status` index refresh re-hashes the working tree through clean
 * filters. */
async function makeRacyClean(repo: TempRepo, relativePath: string, content: string): Promise<void> {
  const { stat } = await import("node:fs/promises");
  const absolute = join(repo.root, relativePath);
  const before = await stat(absolute);
  await import("node:fs/promises").then((fs) => fs.writeFile(absolute, content));
  const { utimes } = await import("node:fs/promises");
  await utimes(absolute, before.atime, before.mtime);
}

/**
 * Full hostile-repository setup with the invariant that NO host Git runs
 * after the filter is configured:
 *
 * 1. data.txt is committed before any filter exists.
 * 2. .gitattributes is committed while the filter is NOT yet configured.
 * 3. The filter is configured with `git config` (a config write that cannot
 *    execute filters; the script file does not need to exist for the value).
 * 4. The filter script is written with plain filesystem access.
 * 5. The file is made racy-clean with plain filesystem access.
 *
 * From step 3 onward, only direct filesystem operations are used.
 */
async function setupHostileCleanFilter(
  repo: TempRepo,
): Promise<{ markerPath: string; scriptPath: string }> {
  await repo.write("data.txt", "original\n");
  repo.git("add", "data.txt");
  repo.commit("initial");
  await repo.write(".gitattributes", "data.txt filter=evil\n");
  repo.git("add", ".gitattributes");
  repo.commit("attrs");
  const { markerPath, scriptPath, filterCommand } = installCleanFilterMarker(repo);
  // The only host git invocation that follows is the `git config` write
  // itself; it cannot execute repository-selected helpers.
  repo.git("config", "filter.evil.clean", filterCommand);
  await import("node:fs/promises").then((fs) =>
    fs.writeFile(scriptPath, filterScriptSource(markerPath)),
  );
  await makeRacyClean(repo, "data.txt", "changed-content\n");
  return { markerPath, scriptPath };
}

/**
 * Direct bounded snapshot of every repository byte (worktree + .git):
 * name -> SHA-256 of content. Symbolic links are recorded by their target
 * string. Uses only filesystem reads, so Git helpers can never execute.
 */
async function snapshotRepoBytes(root: string): Promise<ReadonlyMap<string, string>> {
  const { createHash } = await import("node:crypto");
  const { readdir, readFile, readlink } = await import("node:fs/promises");
  const snapshot = new Map<string, string>();
  const walk = async (directory: string, relative: string): Promise<void> => {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const entryRelative = relative.length === 0 ? entry.name : `${relative}/${entry.name}`;
      const absolute = join(directory, entry.name);
      if (entry.isDirectory()) {
        await walk(absolute, entryRelative);
      } else {
        const content = entry.isSymbolicLink()
          ? await readlink(absolute)
          : await readFile(absolute);
        snapshot.set(entryRelative, createHash("sha256").update(content).digest("hex"));
      }
    }
  };
  await walk(root, "");
  return snapshot;
}

describe("git inspection helper-execution confinement", () => {
  it("requests sandboxed confinement when the repository selects a clean filter", async () => {
    const repo = await createTempRepo();
    const { markerPath } = await setupHostileCleanFilter(repo);
    // Defense in depth: clear any marker the harness could have left.
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
    const status = await adapter.getStatus({});
    expect(status.repository).toBe(true);

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
    await setupHostileCleanFilter(repo);

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

  it("never invokes repository Git configuration on the host after the hostile filter is configured", async () => {
    // The setup helper's contract: after `git config` writes the filter,
    // every remaining step is a direct filesystem operation. This test
    // proves the repository state is fully prepared without any post-filter
    // host Git invocation by construction (the helper never receives a git
    // function after configuration) and that the adapter phase leaves the
    // repository byte-identical.
    const repo = await createTempRepo();
    const { markerPath } = await setupHostileCleanFilter(repo);
    await rm(markerPath, { force: true });
    const baseline = await snapshotRepoBytes(repo.root);

    const { backend } = createFakeSandboxBackend({
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
    const status = await adapter.getStatus({});
    expect(status.repository).toBe(true);
    const after = await snapshotRepoBytes(repo.root);
    expect(after).toEqual(baseline);
  });
});

describe("git inspection live confinement (real enforcing sandbox only)", () => {
  const liveBackend = createAnthropicSandboxRuntimeBackend({
    workspaceRoot: join(tmpdir(), "solaris-git-live-probe"),
  });

  it("never creates the clean-filter marker and never writes the repository", async (context) => {
    const status = await liveBackend.inspect().catch(() => null);
    const enforcing =
      status !== null &&
      status.state === "available" &&
      status.capabilities.filesystemReadRestriction &&
      status.capabilities.filesystemWriteRestriction &&
      status.capabilities.networkRestriction &&
      status.capabilities.processTreeRestriction;
    if (!enforcing) {
      // A real skip, never a pass: the fail-closed availability gate is
      // asserted by the contract tests above, and live confinement proof
      // only runs against a real enforcing backend.
      console.log(
        `GIT LIVE CONFINEMENT: SKIPPED - no enforcing sandbox backend (state: ${status?.state ?? "unknown"}); host execution is prevented by the availability gate instead.`,
      );
      context.skip();
      return;
    }
    const repo = await createTempRepo();
    try {
      const { markerPath } = await setupHostileCleanFilter(repo);
      // The repository state the adapter phase must preserve, captured with
      // direct filesystem reads that cannot invoke Git helpers.
      const baseline = await snapshotRepoBytes(repo.root);
      // Defense in depth: clear any marker the harness could have left.
      await rm(markerPath, { force: true });

      const runs = createTestRunDirectories();
      const adapter = createGitCliAdapter({
        workspaceRoot: repo.root,
        backend: liveBackend,
        runDirectories: runs.provider,
      });
      const result = await adapter.getStatus({});
      expect(result.repository).toBe(true);
      // The clean filter may execute inside the sandbox, but it can never
      // write its marker to the host repository, and its network attempts
      // are denied.
      await expect(readFile(markerPath, "utf8")).rejects.toThrow();
      // The repository must be byte-identical: no index, object, config,
      // attributes, or worktree writes. If the sandbox allowed any write,
      // the marker (or another mutation) would appear and this test fails.
      const after = await snapshotRepoBytes(repo.root);
      expect(after).toEqual(baseline);
    } finally {
      await repo.cleanup();
    }
  });
});
