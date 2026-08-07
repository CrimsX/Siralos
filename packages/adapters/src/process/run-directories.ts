import { createHash, randomUUID } from "node:crypto";
import { chmod, mkdir, realpath, rm, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join, resolve, sep } from "node:path";
import type { CommandRunPaths } from "@solaris/core";

export type { CommandRunPaths };

export interface RunDirectoryProviderOptions {
  readonly workspaceRoot: string;
  /** Solaris-owned runs root; defaults to `~/.solaris/runs`. */
  readonly runsRoot?: string;
}

export type RunCleanupOutcome =
  | {
      readonly ok: true;
    }
  | {
      readonly ok: false;
      readonly message: string;
    };

export interface RunDirectoryProvider {
  create(): Promise<CommandRunPaths>;
  remove(runId: string): Promise<RunCleanupOutcome>;
}

const RUN_ID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

/**
 * Each command run gets its own directory beneath a verified Solaris-owned
 * location, outside the project workspace and never provider-selectable.
 * The directory is removed after completion; cleanup never follows links and
 * never deletes outside the verified run root.
 */
export function createRunDirectoryProvider(
  options: RunDirectoryProviderOptions,
): RunDirectoryProvider {
  const runsRoot = resolve(options.runsRoot ?? join(homedir(), ".solaris", "runs"));
  const fingerprint = createHash("sha256")
    .update(options.workspaceRoot, "utf8")
    .digest("hex")
    .slice(0, 16);

  async function create(): Promise<CommandRunPaths> {
    const runId = randomUUID();
    const root = join(runsRoot, fingerprint, runId);
    const home = join(root, "home");
    const temp = join(root, "tmp");
    const npmCache = join(root, "npm-cache");
    const scriptCache = join(root, "script-cache");
    await mkdir(home, { recursive: true });
    await mkdir(temp, { recursive: true });
    await mkdir(npmCache, { recursive: true });
    await mkdir(scriptCache, { recursive: true });
    const npmUserConfig = join(root, "npmrc");
    await writeFile(npmUserConfig, "", { flag: "wx" });
    const verified = await verifyRunRoot(root, runsRoot);
    if (!verified.ok) {
      throw new Error(verified.message);
    }
    try {
      await chmod(root, 0o700);
    } catch {
      // restrictive permissions are best-effort where the platform supports them
    }
    return { runId, root, home, temp, npmCache, npmUserConfig, scriptCache };
  }
  async function remove(runId: string): Promise<RunCleanupOutcome> {
    if (!RUN_ID_PATTERN.test(runId)) {
      return { ok: false, message: "Cleanup refused: the run id is invalid." };
    }
    const root = join(runsRoot, fingerprint, runId);
    const verified = await verifyRunRoot(root, runsRoot);
    if (!verified.ok) {
      if (verified.missing) {
        return { ok: true };
      }
      return { ok: false, message: verified.message };
    }
    try {
      await rm(root, { recursive: true, force: true });
      return { ok: true };
    } catch (error: unknown) {
      return {
        ok: false,
        message: `Run directory cleanup failed: ${describeError(error)}`,
      };
    }
  }

  return { create, remove };
}

async function verifyRunRoot(
  root: string,
  runsRoot: string,
): Promise<{ ok: true } | { ok: false; message: string; missing?: boolean }> {
  let canonicalRunsRoot: string;
  try {
    canonicalRunsRoot = await realpath(runsRoot);
  } catch (error: unknown) {
    return { ok: false, message: `The runs root is not accessible: ${describeError(error)}` };
  }
  let canonical: string;
  try {
    canonical = await realpath(root);
  } catch (error: unknown) {
    if (isNotFoundError(error)) {
      return { ok: false, message: "The run directory does not exist.", missing: true };
    }
    return { ok: false, message: `The run directory is not accessible: ${describeError(error)}` };
  }
  const prefix = canonicalRunsRoot.endsWith(sep) ? canonicalRunsRoot : `${canonicalRunsRoot}${sep}`;
  if (canonical !== root || !canonical.startsWith(prefix)) {
    return {
      ok: false,
      message: "The run directory is not the verified Solaris-owned path.",
    };
  }
  return { ok: true };
}

function isNotFoundError(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "an unknown filesystem error occurred";
}
