import { spawn } from "node:child_process";
import { GitError } from "@solaris/core";

export const GIT_ALLOWED_SUBCOMMANDS: readonly string[] = [
  "version",
  "rev-parse",
  "status",
  "diff",
  "check-ignore",
];

/**
 * Environment variables Solaris pins for every Git invocation. Caller
 * values for these names are always discarded; only the pinned values reach
 * the child. `GIT_CONFIG_NOSYSTEM=1` keeps a compromised or malicious
 * machine-wide system Git config from being read at all.
 */
export const GIT_SAFETY_ENVIRONMENT: Readonly<Record<string, string>> = {
  GIT_TERMINAL_PROMPT: "0",
  GIT_OPTIONAL_LOCKS: "0",
  GIT_PAGER: "cat",
  PAGER: "cat",
  GIT_CONFIG_NOSYSTEM: "1",
  GIT_EXTERNAL_DIFF: "",
  LC_ALL: "C",
  LANG: "C",
};

/**
 * Environment variables stripped from the caller-provided environment before
 * any Git process starts. Git reads several `GIT_*` variables that redirect
 * the repository, index, object store, or configuration source; an attacker
 * who can influence the process environment (or a stale hostile environment
 * left behind on the host) must not be able to repoint Git at a different
 * repository or inject configuration through `GIT_CONFIG_COUNT` /
 * `GIT_CONFIG_KEY_*` / `GIT_CONFIG_VALUE_*` / `GIT_CONFIG_PARAMETERS`.
 */
const GIT_STRIPPED_ENVIRONMENT_PATTERNS: readonly RegExp[] = [
  /^GIT_CONFIG_COUNT$/i,
  /^GIT_CONFIG_KEY_\d+$/i,
  /^GIT_CONFIG_VALUE_\d+$/i,
  /^GIT_CONFIG_PARAMETERS$/i,
  /^GIT_CONFIG_NOSYSTEM$/i,
  /^GIT_DIR$/i,
  /^GIT_WORK_TREE$/i,
  /^GIT_INDEX_FILE$/i,
  /^GIT_OBJECT_DIRECTORY$/i,
  /^GIT_ALTERNATE_OBJECT_DIRECTORIES$/i,
  /^GIT_COMMON_DIR$/i,
  /^GIT_NAMESPACE$/i,
  /^GIT_ASKPASS$/i,
  /^SSH_ASKPASS$/i,
  /^GIT_SSH$/i,
  /^GIT_SSH_COMMAND$/i,
  /^GIT_SSH_VARIANT$/i,
  /^GIT_PAGER$/i,
  /^PAGER$/i,
  /^GIT_TERMINAL_PROMPT$/i,
  /^GIT_OPTIONAL_LOCKS$/i,
  /^GIT_EXTERNAL_DIFF$/i,
];

/**
 * Command-line configuration overrides applied before every Git subcommand.
 * Command-line `-c` settings take precedence over repository, user, and
 * system configuration, so repository-local configuration cannot re-enable a
 * disabled behavior, and later arguments cannot override them (Solaris
 * builds the argument array itself; providers only select high-level options).
 *
 * The override set neutralizes every configuration mechanism that can
 * execute or delegate to external code during the supported read-only
 * inspection operations:
 *
 * - `core.fsmonitor` / `core.useBuiltinFSMonitor`: background watch daemons
 *   executed by `git status`.
 * - `core.askPass` / `credential.helper` / `credential.interactive`:
 *   external credential helpers and interactive prompts.
 * - `core.pager` and per-command pagers: external pager programs.
 * - `diff.external`: external diff programs (also pinned empty in the
 *   environment and disabled with `--no-ext-diff` by the adapter).
 * - `alias.<command>`: shell aliases; each allowed subcommand is mapped to
 *   itself so a repository alias like `alias.status = !evil` cannot run.
 */
const GIT_DISABLING_CONFIG: readonly string[] = [
  "-c",
  "core.fsmonitor=false",
  "-c",
  "core.useBuiltinFSMonitor=false",
  "-c",
  "core.askPass=",
  "-c",
  "core.pager=cat",
  "-c",
  "credential.helper=",
  "-c",
  "credential.interactive=false",
  "-c",
  "diff.external=",
  ...GIT_ALLOWED_SUBCOMMANDS.flatMap((command) => ["-c", `alias.${command}=${command}`]),
];

export function sanitizeGitEnvironment(
  environment: Readonly<Record<string, string>>,
): Readonly<Record<string, string>> {
  const sanitized: Record<string, string> = {};
  for (const [name, value] of Object.entries(environment)) {
    if (GIT_STRIPPED_ENVIRONMENT_PATTERNS.some((pattern) => pattern.test(name))) {
      continue;
    }
    sanitized[name] = value;
  }
  return { ...sanitized, ...GIT_SAFETY_ENVIRONMENT };
}

export interface GitProcessResult {
  readonly exitCode: number;
  readonly stdout: string;
  readonly stderr: string;
  readonly stdoutTruncated: boolean;
  readonly stderrTruncated: boolean;
}

export interface GitProcessOptions {
  readonly subcommand: string;
  readonly args: readonly string[];
  readonly cwd: string;
  readonly environment: Readonly<Record<string, string>>;
  readonly signal?: AbortSignal;
  readonly timeoutMs: number;
  readonly maxOutputBytes: number;
  readonly gitExecutable?: string;
}

export async function runGitProcess(options: GitProcessOptions): Promise<GitProcessResult> {
  if (!GIT_ALLOWED_SUBCOMMANDS.includes(options.subcommand)) {
    throw new GitError(
      "git_status_failed",
      `Git subcommand "${options.subcommand}" is not allowed by Solaris.`,
    );
  }
  const fullArgs = [
    ...GIT_DISABLING_CONFIG,
    "--no-pager",
    "--literal-pathspecs",
    options.subcommand,
    ...options.args,
  ];
  const timeoutController = new AbortController();
  let timedOut = false;
  const timeoutTimer = setTimeout(() => {
    timedOut = true;
    timeoutController.abort();
  }, options.timeoutMs);
  const signals: AbortSignal[] = [timeoutController.signal];
  if (options.signal !== undefined) {
    signals.push(options.signal);
  }
  const child = spawn(options.gitExecutable ?? "git", fullArgs, {
    cwd: options.cwd,
    env: sanitizeGitEnvironment(options.environment),
    shell: false,
    stdio: ["ignore", "pipe", "pipe"],
    signal: AbortSignal.any(signals),
  });
  const stdoutSink = createOutputSink(options.maxOutputBytes);
  const stderrSink = createOutputSink(options.maxOutputBytes);
  child.stdout?.on("data", (chunk: Buffer) => {
    stdoutSink.push(chunk);
  });
  child.stderr?.on("data", (chunk: Buffer) => {
    stderrSink.push(chunk);
  });
  try {
    const result = await new Promise<GitProcessResult>((resolve, reject) => {
      child.on("error", (error: NodeJS.ErrnoException) => {
        if (timedOut) {
          reject(new GitError("git_timeout", "The git command timed out.", error));
          return;
        }
        if (options.signal?.aborted) {
          reject(new GitError("git_cancelled", "The git command was cancelled.", error));
          return;
        }
        if (error.code === "ENOENT") {
          reject(new GitError("git_unavailable", "The git executable was not found.", error));
          return;
        }
        reject(new GitError("git_status_failed", `Cannot start git: ${error.message}`, error));
      });
      child.on("close", (exitCode: number | null) => {
        if (timedOut) {
          reject(new GitError("git_timeout", "The git command timed out."));
          return;
        }
        if (options.signal?.aborted) {
          reject(new GitError("git_cancelled", "The git command was cancelled."));
          return;
        }
        resolve({
          exitCode: exitCode ?? 1,
          stdout: stdoutSink.text,
          stderr: stderrSink.text,
          stdoutTruncated: stdoutSink.truncated,
          stderrTruncated: stderrSink.truncated,
        });
      });
    });
    return result;
  } finally {
    clearTimeout(timeoutTimer);
  }
}

function createOutputSink(maxBytes: number): {
  text: string;
  truncated: boolean;
  push(chunk: Buffer): void;
} {
  let text = "";
  let truncated = false;
  return {
    get text(): string {
      return text;
    },
    get truncated(): boolean {
      return truncated;
    },
    push(chunk: Buffer): void {
      if (truncated) {
        return;
      }
      const remaining = maxBytes - text.length;
      if (chunk.length > remaining) {
        text += chunk.toString("utf8", 0, Math.max(remaining, 0));
        truncated = true;
      } else {
        text += chunk.toString("utf8");
      }
    },
  };
}
