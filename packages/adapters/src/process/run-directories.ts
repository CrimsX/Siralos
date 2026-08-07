import { createHash, randomUUID } from "node:crypto";
import { chmod, lstat, mkdir, realpath, rm, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import path from "node:path";
import type { CommandRunPaths } from "@solaris/core";

function join(...parts: readonly string[]): string {
  return path.join(...parts);
}

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
 * Every component of the path is verified with no-follow semantics before
 * anything is created beneath it; components are created exclusively; the
 * full chain is re-verified canonically before the paths are returned; and
 * cleanup re-verifies containment immediately before deletion and never
 * recursively deletes through a link. If a safe state cannot be proven, the
 * directory is preserved and the failure is reported.
 */
export function createRunDirectoryProvider(
  options: RunDirectoryProviderOptions,
): RunDirectoryProvider {
  const configuredRunsRoot = path.resolve(
    options.runsRoot ?? path.join(homedir(), ".solaris", "runs"),
  );
  const fingerprint = createHash("sha256")
    .update(options.workspaceRoot, "utf8")
    .digest("hex")
    .slice(0, 16);

  async function create(): Promise<CommandRunPaths> {
    const runId = randomUUID();
    const canonicalRunsRoot = await establishVerifiedRunsRoot();
    const fingerprintPath = join(canonicalRunsRoot, fingerprint);
    await ensureVerifiedComponent(fingerprintPath, canonicalRunsRoot, "fingerprint directory");
    const root = join(fingerprintPath, runId);
    await ensureVerifiedComponent(root, canonicalRunsRoot, "run directory");
    const home = join(root, "home");
    const temp = join(root, "tmp");
    const npmCache = join(root, "npm-cache");
    const scriptCache = join(root, "script-cache");
    await createVerifiedChildDirectory(home, root, "home");
    await createVerifiedChildDirectory(temp, root, "tmp");
    await createVerifiedChildDirectory(npmCache, root, "npm-cache");
    await createVerifiedChildDirectory(scriptCache, root, "script-cache");
    const npmUserConfig = join(root, "npmrc");
    await createVerifiedChildFile(npmUserConfig, root, "npmrc");
    const verified = await verifyCanonicalRoot(root, canonicalRunsRoot);
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
    let canonicalRunsRoot: string;
    try {
      canonicalRunsRoot = await establishVerifiedRunsRoot();
    } catch (error: unknown) {
      return { ok: false, message: describeError(error) };
    }
    const root = join(canonicalRunsRoot, fingerprint, runId);
    const verified = await verifyCanonicalRoot(root, canonicalRunsRoot);
    if (!verified.ok) {
      if (verified.missing) {
        return { ok: true };
      }
      return { ok: false, message: verified.message };
    }
    // Re-verify immediately before deletion: the path must still resolve
    // canonically to itself and the leaf must be a real directory, so the
    // recursive removal can never traverse a link planted in between.
    let leafStats;
    try {
      leafStats = await lstat(root);
    } catch (error: unknown) {
      if (isNotFoundError(error)) {
        return { ok: true };
      }
      return { ok: false, message: `Run directory cleanup refused: ${describeError(error)}` };
    }
    if (!leafStats.isDirectory() || leafStats.isSymbolicLink()) {
      return {
        ok: false,
        message: "Run directory cleanup refused: the run directory is not a real directory.",
      };
    }
    let canonicalNow: string;
    try {
      canonicalNow = await realpath(root);
    } catch {
      return {
        ok: false,
        message: "Run directory cleanup refused: the run directory cannot be resolved canonically.",
      };
    }
    if (canonicalNow !== root) {
      return {
        ok: false,
        message: "Run directory cleanup refused: the run directory resolves through a link.",
      };
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

  /**
   * Establishes the runs root as a verified real directory chain: every
   * existing component must be a real directory (never a symlink or
   * junction), missing components are created exclusively, and the full
   * chain must resolve canonically to its logical path. The runs root must
   * also be outside the workspace.
   */
  async function establishVerifiedRunsRoot(): Promise<string> {
    // Reject a runs root that contains or sits inside the workspace before
    // creating anything, so a rejected configuration leaves no directories
    // behind inside the workspace.
    const canonicalWorkspace = await realpath(options.workspaceRoot).catch(() => null);
    if (canonicalWorkspace !== null) {
      if (
        configuredRunsRoot === canonicalWorkspace ||
        isPathInside(canonicalWorkspace, configuredRunsRoot) ||
        isPathInside(configuredRunsRoot, canonicalWorkspace)
      ) {
        throw new Error("The runs root and the project workspace must not contain each other.");
      }
    }
    const parsed = path.parse(configuredRunsRoot);
    const relative = path.relative(parsed.root, configuredRunsRoot);
    const relativeComponents =
      relative.length === 0 ? [] : relative.split(path.sep).filter((part) => part.length > 0);
    let current = parsed.root;
    for (const component of relativeComponents) {
      current = join(current, component);
      await ensureVerifiedComponent(current, parsed.root, "runs root");
    }
    const canonical = await realpath(configuredRunsRoot).catch(() => null);
    if (canonical === null || canonical !== configuredRunsRoot) {
      throw new Error(
        "The runs root does not resolve canonically to its configured location; refusing to use it.",
      );
    }
    if (canonicalWorkspace !== null) {
      if (
        canonical === canonicalWorkspace ||
        isPathInside(canonicalWorkspace, canonical) ||
        isPathInside(canonical, canonicalWorkspace)
      ) {
        throw new Error("The runs root and the project workspace must not contain each other.");
      }
    }
    return canonical;
  }

  /**
   * Verifies or exclusively creates one path component. An existing
   * component must be a real directory, never a symbolic link or junction;
   * a missing component is created without recursion, then verified.
   */
  async function ensureVerifiedComponent(
    target: string,
    ancestor: string,
    label: string,
  ): Promise<void> {
    let stats;
    try {
      stats = await lstat(target);
    } catch (error: unknown) {
      if (!isNotFoundError(error)) {
        throw new Error(`${label} is not accessible: ${describeError(error)}`);
      }
      try {
        await mkdir(target);
      } catch (error: unknown) {
        throw new Error(`${label} could not be created: ${describeError(error)}`);
      }
      try {
        stats = await lstat(target);
      } catch (error: unknown) {
        throw new Error(`${label} could not be verified after creation: ${describeError(error)}`);
      }
    }
    if (stats.isSymbolicLink()) {
      throw new Error(`${label} is a symbolic link; refusing to use it.`);
    }
    if (!stats.isDirectory()) {
      throw new Error(`${label} is not a directory; refusing to use it.`);
    }
    const canonical = await realpath(target).catch(() => null);
    if (canonical === null || canonical !== target || !isPathInside(ancestor, canonical)) {
      throw new Error(`${label} does not resolve canonically inside its verified parent.`);
    }
  }

  async function createVerifiedChildDirectory(
    target: string,
    parent: string,
    label: string,
  ): Promise<void> {
    try {
      await mkdir(target);
    } catch (error: unknown) {
      throw new Error(`${label} directory could not be created: ${describeError(error)}`);
    }
    let stats;
    try {
      stats = await lstat(target);
    } catch (error: unknown) {
      throw new Error(`${label} directory could not be verified: ${describeError(error)}`);
    }
    if (stats.isSymbolicLink() || !stats.isDirectory()) {
      throw new Error(`${label} directory is not a real directory; refusing to use it.`);
    }
    const canonical = await realpath(target).catch(() => null);
    if (canonical === null || canonical !== target || !isPathInside(parent, canonical)) {
      throw new Error(`${label} directory does not resolve canonically inside its run.`);
    }
  }

  async function createVerifiedChildFile(
    target: string,
    parent: string,
    label: string,
  ): Promise<void> {
    try {
      await writeFile(target, "", { flag: "wx" });
    } catch (error: unknown) {
      throw new Error(`${label} could not be created: ${describeError(error)}`);
    }
    let stats;
    try {
      stats = await lstat(target);
    } catch (error: unknown) {
      throw new Error(`${label} could not be verified: ${describeError(error)}`);
    }
    if (stats.isSymbolicLink() || !stats.isFile()) {
      throw new Error(`${label} is not a real file; refusing to use it.`);
    }
    const canonical = await realpath(target).catch(() => null);
    if (canonical === null || canonical !== target || !isPathInside(parent, canonical)) {
      throw new Error(`${label} does not resolve canonically inside its run.`);
    }
  }

  async function verifyCanonicalRoot(
    root: string,
    runsRoot: string,
  ): Promise<{ ok: true } | { ok: false; message: string; missing?: boolean }> {
    let canonical: string;
    try {
      canonical = await realpath(root);
    } catch (error: unknown) {
      if (isNotFoundError(error)) {
        return { ok: false, message: "The run directory does not exist.", missing: true };
      }
      return { ok: false, message: `The run directory is not accessible: ${describeError(error)}` };
    }
    if (canonical !== root || !isPathInside(runsRoot, canonical)) {
      return {
        ok: false,
        message: "The run directory is not the verified Solaris-owned path.",
      };
    }
    return { ok: true };
  }

  return { create, remove };
}

function isPathInside(root: string, target: string): boolean {
  const rootPrefix = root.endsWith(path.sep) ? root : `${root}${path.sep}`;
  return target === root || target.startsWith(rootPrefix);
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
