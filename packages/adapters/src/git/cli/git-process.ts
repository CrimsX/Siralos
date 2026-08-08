import { GitError, type SandboxedProcessResult } from "@solaris/core";

export const GIT_ALLOWED_SUBCOMMANDS: readonly string[] = [
  "version",
  "rev-parse",
  "status",
  "diff",
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
  GIT_ATTR_NOSYSTEM: "1",
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
  /^GIT_CONFIG_GLOBAL$/i,
  /^GIT_CONFIG_SYSTEM$/i,
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
  /^GIT_TEST_FSMONITOR$/i,
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
 *
 * This override set is NOT the security boundary by itself: repository- and
 * worktree-configured mechanisms Git may gain in the future (for example
 * `filter.<name>.*` clean/smudge/process filters selected through
 * `.gitattributes`) cannot all be enumerated or disabled from the command
 * line. The mechanical boundary is that Git always executes inside the
 * sandboxed runtime with network denied, writes limited to the exact private
 * run directory, and host reads limited to the minimum repository and
 * trusted Git runtime roots; the overrides here are defense in depth.
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

/**
 * Builds the full Git argument array for one allowed subcommand: the
 * disabling configuration overrides, no pager, literal pathspecs, the
 * subcommand, and the fixed argument list. Provider input can only select
 * validated high-level options that arrive as `args`; the construction is
 * fixed and never includes a shell.
 */
export function buildGitInvocation(
  subcommand: string,
  args: readonly string[],
): readonly string[] {
  if (!GIT_ALLOWED_SUBCOMMANDS.includes(subcommand)) {
    throw new GitError(
      "git_status_failed",
      `Git subcommand "${subcommand}" is not allowed by Solaris.`,
    );
  }
  return [
    ...GIT_DISABLING_CONFIG,
    "--no-pager",
    "--literal-pathspecs",
    subcommand,
    ...args,
  ];
}

export interface GitProcessResult {
  readonly exitCode: number;
  readonly stdout: string;
  readonly stderr: string;
  readonly stdoutTruncated: boolean;
  readonly stderrTruncated: boolean;
}

/**
 * Maps one sandboxed process result to the Git inspection result model.
 * Sandbox failures never surface as Git output: timeouts, cancellation,
 * denials, and sandbox unavailability become typed Git errors so callers
 * can never mistake a confined failure for repository content.
 */
export function mapSandboxedGitResult(
  result: SandboxedProcessResult,
  subcommand: string,
): GitProcessResult {
  switch (result.status) {
    case "completed":
      return {
        exitCode: result.exitCode ?? 1,
        stdout: result.stdout,
        stderr: result.stderr,
        stdoutTruncated: result.stdoutTruncated,
        stderrTruncated: result.stderrTruncated,
      };
    case "timed-out":
      throw new GitError("git_timeout", "The git command timed out.");
    case "cancelled":
      throw new GitError("git_cancelled", "The git command was cancelled.");
    case "sandbox-denied":
      throw new GitError(
        "git_status_failed",
        `Git inspection was denied by the sandbox while running ${subcommand}; repository-configured helpers cannot execute on the host.`,
      );
    case "sandbox-unavailable":
      throw new GitError(
        "git_unavailable",
        "Git inspection is unavailable: the sandbox backend cannot enforce the required boundary.",
      );
    case "output-limit":
      throw new GitError(
        "git_status_failed",
        `Git inspection exceeded its output limit while running ${subcommand}.`,
      );
    case "failed":
      throw new GitError(
        "git_status_failed",
        `Git inspection failed while running ${subcommand} inside the sandbox.`,
      );
  }
}
