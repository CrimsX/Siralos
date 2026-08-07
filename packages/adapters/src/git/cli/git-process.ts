import { spawn } from "node:child_process";
import { GitError } from "@solaris/core";

export const GIT_ALLOWED_SUBCOMMANDS: readonly string[] = [
  "version",
  "rev-parse",
  "status",
  "diff",
  "check-ignore",
];

export const GIT_SAFETY_ENVIRONMENT: Readonly<Record<string, string>> = {
  GIT_TERMINAL_PROMPT: "0",
  GIT_OPTIONAL_LOCKS: "0",
  GIT_PAGER: "cat",
  PAGER: "cat",
  LC_ALL: "C",
  LANG: "C",
};

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

const ALIAS_SELF_OVERRIDES: readonly string[] = GIT_ALLOWED_SUBCOMMANDS.map(
  (command) => `alias.${command}=${command}`,
);

export async function runGitProcess(options: GitProcessOptions): Promise<GitProcessResult> {
  if (!GIT_ALLOWED_SUBCOMMANDS.includes(options.subcommand)) {
    throw new GitError(
      "git_status_failed",
      `Git subcommand "${options.subcommand}" is not allowed by Solaris.`,
    );
  }
  const configArgs = ALIAS_SELF_OVERRIDES.flatMap((override) => ["-c", override]);
  const fullArgs = [
    ...configArgs,
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
    env: options.environment,
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
