import { lstat, realpath } from "node:fs/promises";
import { delimiter, dirname, isAbsolute, join, posix, win32 } from "node:path";
import {
  GitError,
  VALIDATION_OFFLINE_PROFILE,
  type GitDiffRequest,
  type GitDiffResult,
  type GitInspector,
  type GitStatusRequest,
  type GitStatusResult,
  type GitWorkspaceStatus,
  type SandboxBackend,
  type SandboxBackendStatus,
} from "@solaris/core";
import {
  buildChildEnvironment,
  readParentEnvironment,
} from "../../environment/child-environment.js";
import { samePathIdentity } from "../../fs-path-identity.js";
import { validateRelativeWorkspacePath } from "../../tools/workspace/mutations/mutation-paths.js";
import type { RunDirectoryProvider } from "../../process/run-directories.js";
import {
  buildGitInvocation,
  GIT_SAFETY_ENVIRONMENT,
  mapSandboxedGitResult,
  type GitProcessResult,
} from "./git-process.js";
import { parsePorcelainV2 } from "./status-parser.js";
import { parseNumstatDiff } from "./diff-parser.js";

export interface GitCliAdapterOptions {
  readonly workspaceRoot: string;
  /**
   * The enforcing sandbox backend. Git ALWAYS executes inside the sandbox:
   * repository-, worktree-, attribute-, environment-, or configuration-
   * controlled helper code (for example clean filters) can therefore never
   * run on the host. When the backend cannot enforce the required boundary
   * (host-read allowlist, read-only workspace, network denial, process-tree
   * confinement), Git inspection reports itself unavailable and Git is
   * never spawned.
   */
  readonly backend: SandboxBackend;
  /** Solaris-owned private run directories for the sandboxed Git process. */
  readonly runDirectories: RunDirectoryProvider;
  /** Test-only override; defaults to a trusted Git resolved from PATH. */
  readonly gitExecutable?: string;
  readonly timeoutMs?: number;
  readonly maxOutputBytes?: number;
}

const DEFAULT_TIMEOUT_MS = 15_000;
const DEFAULT_MAX_OUTPUT_BYTES = 2 * 1024 * 1024;
const MAX_GIT_RUNTIME_READ_ROOTS = 8;

/**
 * Git inspection adapter. Every Git process runs inside the sandboxed
 * runtime with:
 *  - network denied,
 *  - writes limited to the exact private run directory (the repository is
 *    never writable),
 *  - host reads limited to the minimum repository root and the trusted Git
 *    runtime roots,
 *  - a confined process tree, so any helper Git executes (fsmonitor,
 *    filters, diff drivers, pagers, credential helpers, aliases, or future
 *    mechanisms) runs inside the same confinement and cannot touch the host.
 *
 * The adapter itself never spawns a process (enforced structurally by the
 * architecture check); raw Git process primitives are private to this
 * module and never exported. The executable resolved from PATH is
 * re-verified (canonical identity, regular non-link file) immediately
 * before every launch request; byte-for-byte binding to the spawned image
 * is impossible without an exec-by-handle primitive, so whatever bytes the
 * sandbox launches remain inside the enforcing boundary described above,
 * and Git never executes outside it.
 */
export interface GitExecutableCandidateInfo {
  /** True only for a regular non-link file (symlinks and junctions are rejected). */
  readonly isRegularNonLinkFile: boolean;
}

export interface ResolveGitExecutableOptions {
  readonly pathValue: string;
  /** Platform PATH delimiter (`path.delimiter`): `;` on Windows, `:` on POSIX. */
  readonly delimiter: string;
  readonly platform: NodeJS.Platform;
  /** Bounded inspection: at most this many PATH entries are visited. */
  readonly maxEntries: number;
  readonly stat: (candidate: string) => Promise<GitExecutableCandidateInfo | null>;
}

/**
 * Isolated Git executable resolution from a PATH value. The PATH string is
 * split with the caller-supplied platform delimiter (empty entries are
 * dropped — on POSIX they denote the current directory and must never be
 * resolved relative to the workspace), only a bounded prefix of entries is
 * inspected, candidates must be absolute after joining, and every candidate
 * must be a regular non-link file. Returns the first accepted candidate or
 * null. The stat function is injected so substitution behavior is testable
 * deterministically; the caller supplies the real filesystem stat.
 */
export async function resolveGitExecutableFromPath(
  options: ResolveGitExecutableOptions,
): Promise<string | null> {
  const names = options.platform === "win32" ? ["git.exe", "git"] : ["git"];
  const pathFor = options.platform === "win32" ? win32 : posix;
  const absolute = (candidate: string): boolean => pathFor.isAbsolute(candidate);
  const entries = options.pathValue
    .split(options.delimiter)
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0)
    .slice(0, Math.max(0, options.maxEntries));
  for (const entry of entries) {
    for (const name of names) {
      const candidate = pathFor.join(entry, name);
      if (!absolute(candidate)) {
        continue;
      }
      const info = await options.stat(candidate);
      if (info !== null && info.isRegularNonLinkFile) {
        return candidate;
      }
    }
  }
  return null;
}

export function createGitCliAdapter(options: GitCliAdapterOptions): GitInspector {
  let inspection: GitWorkspaceStatus | null = null;
  let cachedExecutable: string | null | undefined;

  async function requireSandbox(): Promise<void> {
    const status = await options.backend.inspect().catch(() => null);
    if (status === null || !sandboxEnforcesBoundary(status)) {
      const reason =
        status === null
          ? "the sandbox backend could not be inspected"
          : status.state !== "available"
            ? `the sandbox backend state is ${status.state}`
            : "the sandbox backend cannot enforce one of the required restrictions (host-read allowlist, read-only workspace, network denial, process-tree confinement)";
      throw new GitError(
        "git_unavailable",
        `Git inspection is unavailable because ${reason}; Git is never spawned outside the sandbox.`,
      );
    }
  }

  async function resolveGitExecutable(): Promise<string> {
    if (cachedExecutable !== undefined) {
      return cachedExecutable as string;
    }
    if (options.gitExecutable !== undefined) {
      if (!isAbsolute(options.gitExecutable)) {
        throw new GitError(
          "git_unavailable",
          "The git executable override must be an absolute path.",
        );
      }
      const metadata = await lstat(options.gitExecutable).catch(() => null);
      if (metadata === null || metadata.isSymbolicLink() || !metadata.isFile()) {
        throw new GitError("git_unavailable", "The git executable override is not a regular file.");
      }
      cachedExecutable = options.gitExecutable;
      return options.gitExecutable;
    }
    // PATH is split with the platform delimiter (`;` on Windows, `:` on
    // Linux/macOS): a hard-coded delimiter would break discovery on every
    // platform but the one it was written for. Empty entries (which on
    // POSIX denote the current directory) are dropped — they must never be
    // resolved relative to the workspace — and only a bounded prefix of
    // entries is inspected. Candidates must be absolute and must resolve to
    // a regular non-link file; symlinked executables are rejected.
    const found = await resolveGitExecutableFromPath({
      pathValue: readParentEnvironment()["PATH"] ?? "",
      delimiter,
      platform: process.platform,
      maxEntries: 64,
      stat: async (candidate) => {
        const metadata = await lstat(candidate).catch(() => null);
        if (metadata === null) {
          return null;
        }
        return { isRegularNonLinkFile: !metadata.isSymbolicLink() && metadata.isFile() };
      },
    });
    if (found === null) {
      throw new GitError("git_unavailable", "The git executable was not found on PATH.");
    }
    cachedExecutable = found;
    return found;
  }

  /**
   * Re-verifies the resolved executable's identity immediately before every
   * launch request: the path must still resolve canonically to itself (a
   * link or junction planted after discovery is refused) and the leaf must
   * still be a regular non-link file. This narrows the substitution window
   * to the moment of the sandbox's own spawn; the enforcing sandbox boundary
   * is the security line (whatever bytes launch are confined to the read-only
   * workspace, denied network, and private run directory), and byte-binding
   * to the spawned image is not claimed — Node offers no exec-by-handle
   * primitive.
   */
  async function verifyExecutableAtLaunch(gitPath: string): Promise<void> {
    let canonical: string;
    try {
      canonical = await realpath(gitPath);
    } catch {
      throw new GitError(
        "git_unavailable",
        "The git executable could not be resolved immediately before launch; refusing to execute.",
      );
    }
    if (!samePathIdentity(canonical, gitPath)) {
      throw new GitError(
        "git_unavailable",
        "The git executable resolves through a link immediately before launch; refusing to execute.",
      );
    }
    const metadata = await lstat(canonical).catch(() => null);
    if (metadata === null || metadata.isSymbolicLink() || !metadata.isFile()) {
      throw new GitError(
        "git_unavailable",
        "The git executable is no longer a regular file immediately before launch; refusing to execute.",
      );
    }
  }

  async function gitRuntimeReadRoots(gitPath: string): Promise<readonly string[]> {
    const roots: string[] = [dirname(gitPath)];
    const candidates = [
      join(dirname(gitPath), "..", "lib", "git-core"),
      join(dirname(gitPath), "..", "libexec", "git-core"),
      join(dirname(gitPath), "..", "usr", "lib", "git-core"),
      "/usr/lib/git-core",
      "/usr/libexec/git-core",
    ];
    for (const candidate of candidates) {
      if (roots.length >= MAX_GIT_RUNTIME_READ_ROOTS) {
        break;
      }
      const metadata = await lstat(candidate).catch(() => null);
      if (metadata !== null && !metadata.isSymbolicLink() && metadata.isDirectory()) {
        roots.push(candidate);
      }
    }
    return roots;
  }

  async function run(
    subcommand: string,
    args: readonly string[],
    signal?: AbortSignal,
  ): Promise<GitProcessResult> {
    await requireSandbox();
    const gitPath = await resolveGitExecutable();
    await verifyExecutableAtLaunch(gitPath);
    const created = await options.runDirectories.create();
    if (!created.ok) {
      // Run-directory creation fails closed: Git is never launched without
      // a verified Solaris-owned private run directory.
      throw new GitError(
        "git_unavailable",
        `Git inspection is unavailable because a private run directory cannot be prepared safely: ${created.message}`,
      );
    }
    const runPaths = created.paths;
    let failure: unknown = null;
    let outcome: GitProcessResult | null = null;
    try {
      const environment: Readonly<Record<string, string>> = {
        ...buildChildEnvironment(readParentEnvironment(), {
          home: runPaths.home,
          temp: runPaths.temp,
        }),
        ...GIT_SAFETY_ENVIRONMENT,
      };
      const result = await options.backend.execute({
        executable: gitPath,
        arguments: buildGitInvocation(subcommand, args),
        workingDirectory: options.workspaceRoot,
        profile: VALIDATION_OFFLINE_PROFILE,
        environment,
        runDirectory: runPaths.root,
        timeoutMs: options.timeoutMs ?? DEFAULT_TIMEOUT_MS,
        stdoutLimitBytes: options.maxOutputBytes ?? DEFAULT_MAX_OUTPUT_BYTES,
        stderrLimitBytes: options.maxOutputBytes ?? DEFAULT_MAX_OUTPUT_BYTES,
        // The repository root is the minimum read surface Git needs, plus
        // the trusted Git runtime roots; nothing else on the host is granted.
        explicitReadRoots: [options.workspaceRoot, ...(await gitRuntimeReadRoots(gitPath))],
        ...(signal === undefined ? {} : { signal }),
      });
      outcome = mapSandboxedGitResult(result, subcommand);
    } catch (error: unknown) {
      failure = error;
    }
    // The cleanup outcome is OBSERVED on every path, never ignored: a
    // refused or failed cleanup surfaces as an explicit cleanup failure
    // (the original failure, when one occurred, is preserved as the detail),
    // and the preserved run directory is never silently left behind.
    const cleanup = await options.runDirectories.remove(runPaths.runId);
    if (!cleanup.ok) {
      throw new GitError(
        "git_status_failed",
        `Git inspection ran, but its private run directory could not be cleaned up (${cleanup.message}); the run directory was preserved and must be inspected manually.`,
        failure,
      );
    }
    if (failure !== null) {
      throw failure instanceof Error ? failure : new Error(describeGitError(failure));
    }
    return outcome as GitProcessResult;
  }

  async function inspectRepository(signal?: AbortSignal): Promise<GitWorkspaceStatus> {
    if (inspection !== null) {
      return inspection;
    }
    try {
      // Early availability check: a missing executable reports unavailable
      // without any sandbox execution.
      await resolveGitExecutable();
    } catch (error: unknown) {
      inspection = {
        gitAvailable: false,
        gitVersion: null,
        repositoryState: "unavailable",
        repositoryRoot: null,
        message: error instanceof GitError ? error.message : "Git is not available.",
      };
      return inspection;
    }
    let versionResult: GitProcessResult;
    try {
      versionResult = await run("version", [], signal);
    } catch (error: unknown) {
      if (error instanceof GitError && error.code === "git_unavailable") {
        inspection = {
          gitAvailable: false,
          gitVersion: null,
          repositoryState: "unavailable",
          repositoryRoot: null,
          message: error.message,
        };
        return inspection;
      }
      throw error;
    }
    if (versionResult.exitCode !== 0) {
      inspection = {
        gitAvailable: true,
        gitVersion: null,
        repositoryState: "failed",
        repositoryRoot: null,
        message: versionResult.stderr.trim() || "git --version failed.",
      };
      return inspection;
    }
    const gitVersion = parseGitVersion(versionResult.stdout);
    let revResult: GitProcessResult;
    try {
      revResult = await run("rev-parse", ["--show-toplevel", "--is-inside-work-tree"], signal);
    } catch (error: unknown) {
      if (error instanceof GitError && error.code === "git_unavailable") {
        inspection = {
          gitAvailable: false,
          gitVersion: null,
          repositoryState: "unavailable",
          repositoryRoot: null,
          message: error.message,
        };
        return inspection;
      }
      throw error;
    }
    if (revResult.exitCode !== 0) {
      const stderr = revResult.stderr.trim();
      inspection = {
        gitAvailable: true,
        gitVersion,
        repositoryState: "not_repository",
        repositoryRoot: null,
        message: stderr.length > 0 ? stderr : "Not a Git repository.",
      };
      return inspection;
    }
    const lines = revResult.stdout
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
    const topLevel = lines[0] ?? null;
    const insideWorkTree = lines[1] === "true";
    if (!insideWorkTree || topLevel === null) {
      inspection = {
        gitAvailable: true,
        gitVersion,
        repositoryState: "not_repository",
        repositoryRoot: null,
      };
      return inspection;
    }
    let canonicalRoot: string;
    let canonicalWorkspace: string;
    try {
      canonicalRoot = await realpath(topLevel);
      canonicalWorkspace = await realpath(options.workspaceRoot);
    } catch (error: unknown) {
      inspection = {
        gitAvailable: true,
        gitVersion,
        repositoryState: "failed",
        repositoryRoot: null,
        message: `Cannot resolve repository paths: ${describeGitError(error)}`,
      };
      return inspection;
    }
    if (canonicalRoot === canonicalWorkspace) {
      inspection = {
        gitAvailable: true,
        gitVersion,
        repositoryState: "repository",
        repositoryRoot: canonicalRoot,
      };
    } else {
      inspection = {
        gitAvailable: true,
        gitVersion,
        repositoryState: "root_mismatch",
        repositoryRoot: canonicalRoot,
        message: "The Git repository root differs from the Solaris workspace root.",
      };
    }
    return inspection;
  }

  async function ensureRepository(): Promise<GitWorkspaceStatus> {
    const status = await inspectRepository();
    switch (status.repositoryState) {
      case "repository":
        return status;
      case "not_repository":
        throw new GitError("git_not_repository", "The workspace is not a Git repository.");
      case "root_mismatch":
        throw new GitError(
          "git_root_mismatch",
          "The Git repository root differs from the Solaris workspace root.",
        );
      case "unavailable":
        throw new GitError("git_unavailable", status.message ?? "Git is not available.");
      case "failed":
        throw new GitError("git_status_failed", status.message ?? "Git inspection failed.");
    }
  }

  async function getStatus(request: GitStatusRequest): Promise<GitStatusResult> {
    await ensureRepository();
    let result: GitProcessResult;
    try {
      result = await run(
        "status",
        ["--porcelain=v2", "-z", "--branch", "--untracked-files=all", "--ignore-submodules=all"],
        request.signal,
      );
    } catch (error: unknown) {
      if (error instanceof GitError) {
        throw error;
      }
      throw new GitError("git_status_failed", `git status failed: ${describeGitError(error)}`);
    }
    if (result.exitCode !== 0) {
      throw new GitError("git_status_failed", result.stderr.trim() || "git status failed.");
    }
    const parsed = parsePorcelainV2(result.stdout);
    if (result.stdoutTruncated) {
      return { ...parsed, truncated: true };
    }
    return parsed;
  }

  async function getDiff(request: GitDiffRequest): Promise<GitDiffResult> {
    await ensureRepository();
    const scopeArgs = buildScopeArgs(request.scope);
    const paths = request.paths ?? [];
    if (paths.length > 100) {
      throw new GitError("git_diff_failed", "Too many paths requested for git.diff.");
    }
    for (const path of paths) {
      const validation = validateRelativeWorkspacePath(path);
      if (validation !== null) {
        throw new GitError("git_diff_failed", `Invalid diff path: ${validation}`);
      }
    }
    const args = [
      "--no-ext-diff",
      "--no-textconv",
      "--no-color",
      "--ignore-submodules=all",
      ...scopeArgs,
      ...(paths.length > 0 ? ["--", ...paths] : []),
    ];
    let result: GitProcessResult;
    try {
      result = await run("diff", args, request.signal);
    } catch (error: unknown) {
      if (error instanceof GitError) {
        throw error;
      }
      throw new GitError("git_diff_failed", `git diff failed: ${describeGitError(error)}`);
    }
    if (result.exitCode !== 0) {
      if (request.scope === "head" && result.stderr.includes("HEAD")) {
        return emptyDiff(request.scope);
      }
      throw new GitError("git_diff_failed", result.stderr.trim() || "git diff failed.");
    }
    const summaryArgs = [
      "--no-ext-diff",
      "--no-textconv",
      "--no-color",
      "--ignore-submodules=all",
      "--numstat",
      "-z",
      ...scopeArgs,
      ...(paths.length > 0 ? ["--", ...paths] : []),
    ];
    let summaryResult: GitProcessResult;
    try {
      summaryResult = await run("diff", summaryArgs, request.signal);
    } catch (error: unknown) {
      if (error instanceof GitError) {
        throw error;
      }
      throw new GitError("git_diff_failed", `git diff failed: ${describeGitError(error)}`);
    }
    if (summaryResult.exitCode !== 0) {
      throw new GitError("git_diff_failed", summaryResult.stderr.trim() || "git diff failed.");
    }
    const parsed = parseNumstatDiff(summaryResult.stdout);
    return {
      scope: request.scope,
      files: parsed.files,
      patch: result.stdout,
      truncated: result.stdoutTruncated || summaryResult.stdoutTruncated || parsed.truncated,
      untrackedExcluded: true,
    };
  }

  return {
    inspectRepository,
    getStatus,
    getDiff,
  };
}

function sandboxEnforcesBoundary(status: SandboxBackendStatus): boolean {
  return (
    status.state === "available" &&
    status.capabilities.filesystemReadRestriction &&
    status.capabilities.filesystemWriteRestriction &&
    status.capabilities.networkRestriction &&
    status.capabilities.processTreeRestriction
  );
}

function parseGitVersion(stdout: string): string | null {
  const match = /^git version ([\d.]+)/.exec(stdout.trim());
  return match === null ? null : (match[1] ?? null);
}

function buildScopeArgs(scope: GitDiffRequest["scope"]): readonly string[] {
  switch (scope) {
    case "working":
      return [];
    case "staged":
      return ["--cached"];
    case "head":
      return ["HEAD"];
  }
}

function emptyDiff(scope: GitDiffRequest["scope"]): GitDiffResult {
  return {
    scope,
    files: [],
    patch: "",
    truncated: false,
    untrackedExcluded: true,
  };
}

function describeGitError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "an unknown Git failure occurred";
}
