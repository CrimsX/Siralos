import { realpath } from "node:fs/promises";
import {
  GitError,
  describeGitError,
  type GitDiffRequest,
  type GitDiffResult,
  type GitInspector,
  type GitStatusRequest,
  type GitStatusResult,
  type GitWorkspaceStatus,
} from "@solaris/core";
import {
  buildChildEnvironment,
  readParentEnvironment,
} from "../../environment/child-environment.js";
import { getSandboxDirectories } from "../../sandbox/sandbox-directories.js";
import { validateRelativeWorkspacePath } from "../../tools/workspace/mutations/mutation-paths.js";
import { GIT_SAFETY_ENVIRONMENT, runGitProcess, type GitProcessResult } from "./git-process.js";
import { parsePorcelainV2 } from "./status-parser.js";
import { parseNumstatDiff } from "./diff-parser.js";

export interface GitCliAdapterOptions {
  readonly workspaceRoot: string;
  readonly timeoutMs?: number;
  readonly maxOutputBytes?: number;
}

const DEFAULT_TIMEOUT_MS = 15_000;
const DEFAULT_MAX_OUTPUT_BYTES = 2 * 1024 * 1024;

export function createGitCliAdapter(options: GitCliAdapterOptions): GitInspector {
  let inspection: GitWorkspaceStatus | null = null;

  async function run(
    subcommand: string,
    args: readonly string[],
    signal?: AbortSignal,
  ): Promise<GitProcessResult> {
    const sandboxDirectories = getSandboxDirectories();
    const environment: Readonly<Record<string, string>> = {
      ...buildChildEnvironment(readParentEnvironment(), {
        home: sandboxDirectories.home,
        temp: sandboxDirectories.temp,
      }),
      ...GIT_SAFETY_ENVIRONMENT,
    };
    return runGitProcess({
      subcommand,
      args,
      cwd: options.workspaceRoot,
      environment,
      ...(signal === undefined ? {} : { signal }),
      timeoutMs: options.timeoutMs ?? DEFAULT_TIMEOUT_MS,
      maxOutputBytes: options.maxOutputBytes ?? DEFAULT_MAX_OUTPUT_BYTES,
    });
  }

  async function inspectRepository(signal?: AbortSignal): Promise<GitWorkspaceStatus> {
    if (inspection !== null) {
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
          message: "Git is not installed or not on PATH.",
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
          message: "Git is not installed or not on PATH.",
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
        throw new GitError("git_unavailable", "Git is not available.");
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
