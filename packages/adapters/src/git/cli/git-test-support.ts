import { mkdtemp, rm, writeFile, readFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";

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

export { join, tmpdir };
