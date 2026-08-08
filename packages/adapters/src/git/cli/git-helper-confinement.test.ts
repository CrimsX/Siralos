import { mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createGitCliAdapter } from "./git-cli-adapter.js";
import {
  cleanupTempDirs,
  createTempRepo,
  createTestRunDirectories,
  registerTempDir,
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

/** Distinct bounded outcomes the confined filter records into the
 * sandbox-private result channel (its Solaris-controlled HOME inside the
 * private run directory, which the sandbox profile permits it to write):
 *   - `filter-ran`:            the helper executed inside the sandbox
 *   - `repo-write-denied`:     a workspace write attempt was refused
 *   - `repo-write-succeeded`:  the workspace write attempt SUCCEEDED (fail)
 *   - `network-denied`:        the outbound connection attempt was refused
 *   - `network-connected`:     the outbound connection SUCCEEDED (fail)
 *   - `network-timeout`:       no immediate denial observed (fail on an
 *                              enforcing backend)
 * Exactly one network outcome is recorded: the helper settles on the first
 * of connect/error/timeout and destroys the socket, so a later event can
 * never overwrite or append a second network outcome.
 */
const RESULT_FILE_NAME = "clean-filter-result.json";
const REPO_WRITE_PROBE_NAME = "clean-filter-repo-write-probe.txt";

function filterScriptSource(): string {
  return `"use strict";
const fs = require("node:fs");
const net = require("node:net");
const path = require("node:path");
const home = process.env.HOME || process.env.USERPROFILE;
const resultPath = home ? path.join(home, ${JSON.stringify(RESULT_FILE_NAME)}) : null;
function record(outcome) {
  if (resultPath) {
    try {
      fs.appendFileSync(resultPath, outcome + "\\n");
    } catch (error) {
      // never let the result write fail the filter; a missing result file
      // is itself a test failure
    }
  }
}
record("filter-ran");
try {
  fs.writeFileSync(path.join(process.cwd(), ${JSON.stringify(REPO_WRITE_PROBE_NAME)}), "denied?\\n");
  record("repo-write-succeeded");
} catch (error) {
  record("repo-write-denied");
}
const socket = net.connect({ host: "1.1.1.1", port: 53 });
socket.setTimeout(1500);
let settled = false;
function settle(outcome) {
  if (settled) {
    return;
  }
  settled = true;
  socket.destroy();
  record(outcome);
}
socket.once("connect", function () {
  settle("network-connected");
});
socket.once("error", function () {
  settle("network-denied");
});
socket.once("timeout", function () {
  settle("network-timeout");
});
process.stdin.pipe(process.stdout);
`;
}

/**
 * Reads the archived sandbox-private result channel. Fails (rejects) when
 * the file is missing or malformed, so a helper that never executed, a
 * result that was never written, or a truncated file is a test failure.
 */
async function readResultOutcomes(archivePath: string): Promise<readonly string[]> {
  const raw = await readFile(archivePath, "utf8");
  const outcomes = raw
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
  if (outcomes.length === 0) {
    throw new Error(`The clean-filter result channel ${archivePath} is empty.`);
  }
  const known = new Set([
    "filter-ran",
    "repo-write-denied",
    "repo-write-succeeded",
    "network-denied",
    "network-connected",
    "network-timeout",
  ]);
  for (const outcome of outcomes) {
    if (!known.has(outcome)) {
      throw new Error(`The clean-filter result channel contains an unknown outcome: ${outcome}`);
    }
  }
  return outcomes;
}

/** Creates a host-side archive path for the sandbox-private result channel. */
async function createResultArchivePath(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "solaris-git-live-result-"));
  registerTempDir(directory);
  return join(directory, RESULT_FILE_NAME);
}

function installCleanFilterScript(repo: TempRepo): { scriptPath: string; filterCommand: string } {
  const scriptPath = join(repo.root, "filter-marker.cjs");
  return { scriptPath, filterCommand: `node ${JSON.stringify(scriptPath)}` };
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
async function setupHostileCleanFilter(repo: TempRepo): Promise<void> {
  await repo.write("data.txt", "original\n");
  repo.git("add", "data.txt");
  repo.commit("initial");
  await repo.write(".gitattributes", "data.txt filter=evil\n");
  repo.git("add", ".gitattributes");
  repo.commit("attrs");
  const { scriptPath, filterCommand } = installCleanFilterScript(repo);
  // The only host git invocation that follows is the `git config` write
  // itself; it cannot execute repository-selected helpers.
  repo.git("config", "filter.evil.clean", filterCommand);
  await import("node:fs/promises").then((fs) => fs.writeFile(scriptPath, filterScriptSource()));
  await makeRacyClean(repo, "data.txt", "changed-content\n");
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
    await setupHostileCleanFilter(repo);
    const archivePath = await createResultArchivePath();

    const { backend, requests } = createFakeSandboxBackend({
      results: [
        completedResult({ stdout: "git version 2.40.0\n" }),
        completedResult({ stdout: `${repo.root}\ntrue\n` }),
        completedResult({ stdout: "# branch.head main\n" }),
      ],
    });
    const runs = createTestRunDirectories({ resultArchivePath: archivePath });
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
    // The fake backend never executes, so the filter never ran: no result
    // channel file may exist anywhere.
    await expect(readFile(archivePath, "utf8")).rejects.toThrow();
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
    // repository byte-identical and never executes the filter on the host
    // (the fake backend does not execute; the result channel stays empty).
    const repo = await createTempRepo();
    await setupHostileCleanFilter(repo);
    const archivePath = await createResultArchivePath();
    const baseline = await snapshotRepoBytes(repo.root);

    const { backend } = createFakeSandboxBackend({
      results: [
        completedResult({ stdout: "git version 2.40.0\n" }),
        completedResult({ stdout: `${repo.root}\ntrue\n` }),
        completedResult({ stdout: "# branch.head main\n" }),
      ],
    });
    const runs = createTestRunDirectories({ resultArchivePath: archivePath });
    const adapter = createGitCliAdapter({
      workspaceRoot: repo.root,
      backend,
      runDirectories: runs.provider,
    });
    const status = await adapter.getStatus({});
    expect(status.repository).toBe(true);
    const after = await snapshotRepoBytes(repo.root);
    expect(after).toEqual(baseline);
    await expect(readFile(archivePath, "utf8")).rejects.toThrow();
  });
});

describe("git inspection live confinement (real enforcing sandbox only)", () => {
  const liveBackend = createAnthropicSandboxRuntimeBackend({
    workspaceRoot: join(tmpdir(), "solaris-git-live-probe"),
  });

  it("proves the clean filter ran inside the sandbox, wrote only to the permitted result channel, and had its network denied", async (context) => {
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
      await setupHostileCleanFilter(repo);
      // The repository state the adapter phase must preserve, captured with
      // direct filesystem reads that cannot invoke Git helpers.
      const baseline = await snapshotRepoBytes(repo.root);
      const archivePath = await createResultArchivePath();

      const runs = createTestRunDirectories({ resultArchivePath: archivePath });
      const adapter = createGitCliAdapter({
        workspaceRoot: repo.root,
        backend: liveBackend,
        runDirectories: runs.provider,
      });
      const result = await adapter.getStatus({});
      expect(result.repository).toBe(true);

      // 1. The hostile clean filter actually executed inside the sandbox:
      //    its outcome is observed through the archived sandbox-private
      //    result channel (missing or malformed fails the test).
      const outcomes = await readResultOutcomes(archivePath);
      expect(outcomes).toContain("filter-ran");

      // 2. It could write ONLY to the permitted sandbox-private result
      //    location: its workspace write probe was denied and never
      //    succeeded.
      expect(outcomes).toContain("repo-write-denied");
      expect(outcomes).not.toContain("repo-write-succeeded");

      // 3. Its outbound network connection was denied, not connected and
      //    not merely timed out: an enforcing backend denies immediately.
      expect(outcomes).toContain("network-denied");
      expect(outcomes).not.toContain("network-connected");
      expect(outcomes).not.toContain("network-timeout");

      // 4. It could not write into the repository, and 5. the repository
      //    bytes remained unchanged (any index, object, config, attributes,
      //    or worktree write would change this snapshot).
      const after = await snapshotRepoBytes(repo.root);
      expect(after).toEqual(baseline);
    } finally {
      await repo.cleanup();
    }
  });
});
