import { mkdir, mkdtemp, rm, writeFile, readFile } from "node:fs/promises";
import { spawn, spawnSync } from "node:child_process";
import { StringDecoder } from "node:string_decoder";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type {
  SandboxBackend,
  SandboxBackendStatus,
  SandboxedProcessRequest,
  SandboxedProcessResult,
} from "@solaris/core";
import { buildGitInvocation } from "./git-process.js";
import { createGitCliAdapter } from "./git-cli-adapter.js";

export interface TempRepo {
  readonly root: string;
  cleanup(): Promise<void>;
  git(...args: string[]): { status: number; stdout: string; stderr: string };
  write(relativePath: string, content: string): Promise<void>;
  commit(message: string): void;
}

const tempDirectories: string[] = [];

export function registerTempDir(directory: string): void {
  tempDirectories.push(directory);
}

export async function cleanupTempDirs(): Promise<void> {
  for (const directory of tempDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
}

export async function createTempRepo(initialBranch = "main"): Promise<TempRepo> {
  const root = await mkdtemp(join(tmpdir(), "solaris-git-test-"));
  registerTempDir(root);
  const git = (...args: string[]): { status: number; stdout: string; stderr: string } => {
    const result = spawnSync(
      "git",
      ["-c", "user.name=Solaris Test", "-c", "user.email=test@solaris.dev", ...args],
      {
        cwd: root,
        encoding: "utf8",
        env: {
          ...process.env,
          GIT_TERMINAL_PROMPT: "0",
          GIT_OPTIONAL_LOCKS: "0",
          LC_ALL: "C",
          LANG: "C",
        },
      },
    );
    return {
      status: result.status ?? 1,
      stdout: result.stdout ?? "",
      stderr: result.stderr ?? "",
    };
  };
  const init = git("init", `--initial-branch=${initialBranch}`);
  if (init.status !== 0) {
    throw new Error(`git init failed: ${init.stderr}`);
  }
  const repo: TempRepo = {
    root,
    async cleanup(): Promise<void> {
      await rm(root, { recursive: true, force: true });
    },
    git,
    async write(relativePath: string, content: string): Promise<void> {
      await writeFile(join(root, relativePath), content);
    },
    commit(message: string): void {
      const result = git("commit", "-m", message);
      if (result.status !== 0) {
        throw new Error(`git commit failed: ${result.stderr}`);
      }
    },
  };
  return repo;
}

export async function createNonGitDir(): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), "solaris-nongit-test-"));
  registerTempDir(root);
  return root;
}

export async function readFileText(absolutePath: string): Promise<string> {
  return readFile(absolutePath, "utf8");
}

/**
 * TEST-ONLY host-executing Git backend. It reports itself as an enforcing
 * backend and executes the requested Git invocation directly on the host
 * through `spawn`. It exists only so behavior and parser tests can observe
 * real Git output; the production adapter NEVER uses it (the architecture
 * check forbids child-process use in the git adapter, and production wiring
 * always injects the real sandboxed backend). It must never be used to
 * substantiate a security claim.
 */
export function createHostGitBackend(options: {
  readonly workspaceRoot: string;
  readonly gitExecutable?: string;
  readonly status?: Partial<SandboxBackendStatus>;
}): {
  backend: SandboxBackend;
  requests: () => readonly SandboxedProcessRequest[];
} {
  const requests: SandboxedProcessRequest[] = [];
  const status: SandboxBackendStatus = {
    backendId: "host-git-test-backend",
    state: "available",
    platform: "linux",
    version: "0.0.0-test",
    capabilities: {
      filesystemReadRestriction: true,
      filesystemWriteRestriction: true,
      networkRestriction: true,
      processTreeRestriction: true,
      violationReporting: true,
    },
    ...options.status,
  };
  const backend: SandboxBackend = {
    id: "host-git-test-backend",
    inspect(): Promise<SandboxBackendStatus> {
      return Promise.resolve(status);
    },
    execute(request: SandboxedProcessRequest): Promise<SandboxedProcessResult> {
      requests.push(request);
      return new Promise<SandboxedProcessResult>((resolve, reject) => {
        const child = spawn(request.executable, [...request.arguments], {
          cwd: request.workingDirectory,
          env: { ...request.environment },
          shell: false,
          stdio: ["ignore", "pipe", "pipe"],
          ...(request.signal === undefined ? {} : { signal: request.signal }),
        });
        const stdoutSink = createBoundedSink(request.stdoutLimitBytes ?? 2 * 1024 * 1024);
        const stderrSink = createBoundedSink(request.stderrLimitBytes ?? 2 * 1024 * 1024);
        child.stdout?.on("data", (chunk: Buffer) => {
          stdoutSink.push(chunk);
        });
        child.stderr?.on("data", (chunk: Buffer) => {
          stderrSink.push(chunk);
        });
        child.on("error", (error: NodeJS.ErrnoException) => {
          reject(
            new Error(`test git backend failed to start ${request.executable}: ${error.message}`),
          );
        });
        child.on("close", (exitCode: number | null) => {
          if (request.signal?.aborted) {
            reject(new Error("cancelled"));
            return;
          }
          resolve({
            status: "completed",
            exitCode: exitCode ?? 1,
            signal: null,
            stdout: stdoutSink.text(),
            stderr: stderrSink.text(),
            stdoutTruncated: stdoutSink.truncated(),
            stderrTruncated: stderrSink.truncated(),
            durationMs: 0,
            violations: [],
          });
        });
      });
    },
    close(): Promise<void> {
      return Promise.resolve();
    },
  };
  return { backend, requests: () => requests };
}

export { buildGitInvocation, join, tmpdir };

/** Byte-accurate bounded UTF-8 sink mirroring the production backend. */
function createBoundedSink(maxBytes: number): {
  push(chunk: Buffer): void;
  text(): string;
  truncated(): boolean;
} {
  const decoder = new StringDecoder("utf8");
  let text = "";
  let remaining = maxBytes;
  let cut = false;
  return {
    push(chunk: Buffer): void {
      if (cut) {
        return;
      }
      if (chunk.length >= remaining) {
        text += decoder.write(chunk.subarray(0, Math.max(remaining, 0)));
        cut = true;
        return;
      }
      remaining -= chunk.length;
      text += decoder.write(chunk);
    },
    text(): string {
      return text + decoder.end();
    },
    truncated(): boolean {
      return cut;
    },
  };
}

/**
 * TEST-ONLY adapter harness: wires the adapter to the host-executing test
 * backend and a temporary run-directory provider. Behavior and parser tests
 * use it to observe real Git output; it must never be used to substantiate
 * a security claim (production always routes through the real sandbox).
 */
export function createTestGitAdapter(
  workspaceRoot: string,
  options: {
    readonly gitExecutable?: string;
    readonly maxOutputBytes?: number;
    readonly timeoutMs?: number;
  } = {},
) {
  const git = createHostGitBackend({ workspaceRoot });
  const runs = createTestRunDirectories();
  const adapter = createGitCliAdapter({
    workspaceRoot,
    backend: git.backend,
    runDirectories: runs.provider,
    ...(options.gitExecutable === undefined ? {} : { gitExecutable: options.gitExecutable }),
    ...(options.maxOutputBytes === undefined ? {} : { maxOutputBytes: options.maxOutputBytes }),
    ...(options.timeoutMs === undefined ? {} : { timeoutMs: options.timeoutMs }),
  });
  return { adapter, requests: git.requests, runs };
}

/**
 * TEST-ONLY run-directory provider: each create() makes a unique temporary
 * directory that remove() deletes. Mirrors the shape of the production
 * provider (typed outcomes) for adapter tests that exercise sandboxed
 * execution.
 */
export function createTestRunDirectories(): {
  provider: {
    create(): Promise<
      | {
          readonly ok: true;
          readonly paths: {
            runId: string;
            root: string;
            home: string;
            temp: string;
            npmCache: string;
            npmUserConfig: string;
            scriptCache: string;
          };
        }
      | { readonly ok: false; readonly reason: "unavailable"; readonly message: string }
    >;
    remove(
      runId: string,
    ): Promise<
      | { readonly ok: true }
      | { readonly ok: false; readonly reason: "failed"; readonly message: string }
    >;
  };
  roots: () => readonly string[];
} {
  const created: string[] = [];
  return {
    provider: {
      async create() {
        const root = await mkdtemp(join(tmpdir(), "solaris-git-run-"));
        created.push(root);
        const home = join(root, "home");
        const temp = join(root, "tmp");
        const npmCache = join(root, "npm-cache");
        const scriptCache = join(root, "script-cache");
        const npmUserConfig = join(root, "npmrc");
        await mkdir(home, { recursive: true });
        await mkdir(temp, { recursive: true });
        return {
          ok: true,
          paths: {
            runId: `run-${created.length}`,
            root,
            home,
            temp,
            npmCache,
            npmUserConfig,
            scriptCache,
          },
        };
      },
      async remove(
        runId: string,
      ): Promise<
        | { readonly ok: true }
        | { readonly ok: false; readonly reason: "failed"; readonly message: string }
      > {
        const index = Number(runId.replace("run-", "")) - 1;
        const root = created[index];
        if (root === undefined) {
          return { ok: false, reason: "failed", message: "unknown run" };
        }
        await rm(root, { recursive: true, force: true }).catch(() => undefined);
        return { ok: true };
      },
    },
    roots: () => created,
  };
}
