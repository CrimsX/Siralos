import { createHash, randomUUID } from "node:crypto";
import { chmod, lstat, mkdir, readdir, realpath, rmdir, unlink, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import path from "node:path";
import type { CommandRunPaths } from "@solaris/core";
import { isWithinPathIdentity, samePathIdentity } from "../fs-path-identity.js";
import { removeDirectoryTreeBounded } from "../fs/directory-enumeration.js";

/** Entry budget for one bounded run-directory removal. */
const RUN_DIRECTORY_REMOVAL_ENTRY_BUDGET = 50_000;

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
 * anything is created beneath it; components are created exclusively with
 * the verified parent identity re-checked immediately before the create;
 * the full chain is re-verified canonically before the paths are returned;
 * and cleanup re-verifies containment immediately before deletion and never
 * recursively deletes through a link. Identity comparisons are platform
 * aware (case/separator/prefix normalized on Windows). If a safe state
 * cannot be proven, the directory is preserved and the failure is
 * reported — nothing uncertain is ever deleted.
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
  // Exact filesystem identity of each run root as Solaris created it; a
  // substituted directory (even a non-link replacement) is refused cleanup.
  const runRootIdentities = new Map<
    string,
    { readonly dev: number | bigint; readonly ino: number | bigint }
  >();

  async function create(): Promise<CommandRunPaths> {
    const runId = randomUUID();
    const canonicalRunsRoot = await establishVerifiedRunsRoot();
    const fingerprintPath = join(canonicalRunsRoot, fingerprint);
    await ensureVerifiedDirectoryComponent(
      fingerprintPath,
      canonicalRunsRoot,
      "fingerprint directory",
    );
    const root = join(fingerprintPath, runId);
    await ensureVerifiedDirectoryComponent(root, fingerprintPath, "run directory");
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
    // Record the exact created object's identity so cleanup can refuse a
    // substituted directory even when the replacement is not a link.
    const rootMetadata = await lstat(root);
    runRootIdentities.set(runId, { dev: rootMetadata.dev, ino: rootMetadata.ino });
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
    // recursive removal can never traverse a link planted in between. The
    // root's exact filesystem identity must still match the object Solaris
    // created, so a substituted directory (even a non-link replacement) is
    // refused cleanup. Removal is two-phase: the full plan is validated and
    // budgeted without mutation, and a refused plan performs zero deletions.
    const preDelete = await verifyDeletableRoot(root);
    if (!preDelete.ok) {
      return { ok: false, message: preDelete.message };
    }
    const expectedIdentity = runRootIdentities.get(runId);
    if (expectedIdentity !== undefined) {
      const currentMetadata = await lstat(root).catch(() => null);
      if (
        currentMetadata === null ||
        currentMetadata.dev !== expectedIdentity.dev ||
        currentMetadata.ino !== expectedIdentity.ino
      ) {
        return {
          ok: false,
          message:
            "Run directory cleanup refused: the directory at the run path is not the exact object Solaris created; it may have been substituted. It was preserved.",
        };
      }
    }
    try {
      await removeDirectoryTreeBounded(root, RUN_DIRECTORY_REMOVAL_ENTRY_BUDGET);
    } catch (error: unknown) {
      return {
        ok: false,
        message: `Run directory cleanup failed: ${describeError(error)}`,
      };
    }
    try {
      await lstat(root);
      return {
        ok: false,
        message:
          "Run directory cleanup failed: the run directory still exists after removal; it was preserved and must be inspected manually.",
      };
    } catch (error: unknown) {
      if (isNotFoundError(error)) {
        return { ok: true };
      }
      return {
        ok: false,
        message: `Run directory cleanup failed: ${describeError(error)}`,
      };
    }
  }

  /**
   * Establishes the runs root as a verified real directory chain: every
   * existing component must be a real directory (never a symlink or
   * junction), missing components are created exclusively with the verified
   * parent identity re-checked immediately before creation, and the full
   * chain must resolve canonically to its logical path (identity
   * comparison is platform aware). The runs root must also be outside the
   * workspace.
   */
  async function establishVerifiedRunsRoot(): Promise<string> {
    // Reject a runs root that contains or sits inside the workspace before
    // creating anything, so a rejected configuration leaves no directories
    // behind inside the workspace.
    const canonicalWorkspace = await realpath(options.workspaceRoot).catch(() => null);
    if (canonicalWorkspace !== null) {
      if (
        samePathIdentity(configuredRunsRoot, canonicalWorkspace) ||
        isWithinPathIdentity(canonicalWorkspace, configuredRunsRoot) ||
        isWithinPathIdentity(configuredRunsRoot, canonicalWorkspace)
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
      await ensureVerifiedDirectoryComponent(current, parsed.root, "runs root");
    }
    const canonical = await realpath(configuredRunsRoot).catch(() => null);
    if (canonical === null || !samePathIdentity(canonical, configuredRunsRoot)) {
      throw new Error(
        "The runs root does not resolve canonically to its configured location; refusing to use it.",
      );
    }
    if (canonicalWorkspace !== null) {
      if (
        samePathIdentity(canonical, canonicalWorkspace) ||
        isWithinPathIdentity(canonicalWorkspace, canonical) ||
        isWithinPathIdentity(canonical, canonicalWorkspace)
      ) {
        throw new Error("The runs root and the project workspace must not contain each other.");
      }
    }
    return canonical;
  }

  /**
   * Verifies a parent component immediately before a child is created:
   * the parent must still be the exact verified real directory, so a
   * parent swapped for a link between verification and creation is
   * detected before the child create call.
   */
  async function verifyParentIdentity(parent: string, label: string): Promise<void> {
    let stats;
    try {
      stats = await lstat(parent);
    } catch (error: unknown) {
      throw new Error(`${label} is not accessible before child creation: ${describeError(error)}`);
    }
    if (stats.isSymbolicLink() || !stats.isDirectory()) {
      throw new Error(`${label} is no longer a real directory; refusing to create beneath it.`);
    }
    let canonical: string;
    try {
      canonical = await realpath(parent);
    } catch {
      throw new Error(`${label} no longer resolves canonically; refusing to create beneath it.`);
    }
    if (!samePathIdentity(canonical, parent)) {
      throw new Error(`${label} resolves through a link; refusing to create beneath it.`);
    }
  }

  /**
   * Verifies or exclusively creates one directory component. An existing
   * component must be a real directory, never a symbolic link or junction;
   * a missing component is created without recursion (after the verified
   * parent identity is re-checked) and then verified. An escaped created
   * component is removed only when the exact created empty object can be
   * identified and verified; otherwise it is preserved and reported.
   */
  async function ensureVerifiedDirectoryComponent(
    target: string,
    ancestor: string,
    label: string,
  ): Promise<void> {
    let created = false;
    let stats;
    try {
      stats = await lstat(target);
    } catch (error: unknown) {
      if (!isNotFoundError(error)) {
        throw new Error(`${label} is not accessible: ${describeError(error)}`);
      }
      created = true;
      await verifyParentIdentity(path.dirname(target), label);
      try {
        await mkdir(target);
      } catch (mkdirError: unknown) {
        if (isExistsError(mkdirError)) {
          // Another concurrent creator won the race; verify what exists.
          created = false;
        } else {
          throw new Error(`${label} could not be created: ${describeError(mkdirError)}`);
        }
      }
      try {
        stats = await lstat(target);
      } catch (verifyError: unknown) {
        throw new Error(
          `${label} could not be verified after creation: ${describeError(verifyError)}`,
        );
      }
    }
    if (stats.isSymbolicLink()) {
      throw new Error(`${label} is a symbolic link; refusing to use it.`);
    }
    if (!stats.isDirectory()) {
      throw new Error(`${label} is not a directory; refusing to use it.`);
    }
    const canonical = await realpath(target).catch(() => null);
    if (
      canonical === null ||
      !samePathIdentity(canonical, target) ||
      !isWithinPathIdentity(ancestor, canonical)
    ) {
      if (created) {
        await removeOnlyIfProvablyCreatedEmpty(target);
      }
      throw new Error(`${label} does not resolve canonically inside its verified parent.`);
    }
  }

  async function createVerifiedChildDirectory(
    target: string,
    parent: string,
    label: string,
  ): Promise<void> {
    await verifyParentIdentity(parent, label);
    try {
      await mkdir(target);
    } catch (error: unknown) {
      if (isExistsError(error)) {
        throw new Error(`${label} directory already exists; refusing to use it.`);
      }
      throw new Error(`${label} directory could not be created: ${describeError(error)}`);
    }
    let stats;
    try {
      stats = await lstat(target);
    } catch (error: unknown) {
      throw new Error(`${label} directory could not be verified: ${describeError(error)}`);
    }
    if (stats.isSymbolicLink() || !stats.isDirectory()) {
      await removeOnlyIfProvablyCreatedEmpty(target);
      throw new Error(`${label} directory is not a real directory; refusing to use it.`);
    }
    const canonical = await realpath(target).catch(() => null);
    if (
      canonical === null ||
      !samePathIdentity(canonical, target) ||
      !isWithinPathIdentity(parent, canonical)
    ) {
      await removeOnlyIfProvablyCreatedEmpty(target);
      throw new Error(`${label} directory does not resolve canonically inside its run.`);
    }
  }

  async function createVerifiedChildFile(
    target: string,
    parent: string,
    label: string,
  ): Promise<void> {
    await verifyParentIdentity(parent, label);
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
      await removeOnlyIfProvablyCreatedEmpty(target);
      throw new Error(`${label} is not a real file; refusing to use it.`);
    }
    const canonical = await realpath(target).catch(() => null);
    if (
      canonical === null ||
      !samePathIdentity(canonical, target) ||
      !isWithinPathIdentity(parent, canonical)
    ) {
      await removeOnlyIfProvablyCreatedEmpty(target);
      throw new Error(`${label} does not resolve canonically inside its run.`);
    }
  }

  /**
   * Removes a component that was exclusively created but escaped the
   * verified tree. The removal targets the exact object that now occupies
   * the created location — resolved canonically and verified to be a real,
   * empty directory (or an empty regular file) — never a link and never an
   * uncertain target. If the object cannot be proven to be the exact empty
   * created one, it is preserved and the failure is reported.
   */
  async function removeOnlyIfProvablyCreatedEmpty(target: string): Promise<void> {
    const canonical = await realpath(target).catch(() => null);
    if (canonical === null) {
      return;
    }
    let stats;
    try {
      stats = await lstat(canonical);
    } catch {
      return;
    }
    if (stats.isSymbolicLink()) {
      return;
    }
    try {
      if (stats.isDirectory()) {
        const entries = await readdir(canonical);
        if (entries.length !== 0) {
          return;
        }
        await rmdir(canonical);
      } else if (stats.isFile()) {
        await unlink(canonical);
      }
    } catch {
      // never delete an uncertain target; report the original failure
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
    if (!samePathIdentity(canonical, root) || !isWithinPathIdentity(runsRoot, canonical)) {
      return {
        ok: false,
        message: "The run directory is not the verified Solaris-owned path.",
      };
    }
    return { ok: true };
  }

  async function verifyDeletableRoot(
    root: string,
  ): Promise<{ ok: true } | { ok: false; message: string }> {
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
    if (!samePathIdentity(canonicalNow, root)) {
      return {
        ok: false,
        message: "Run directory cleanup refused: the run directory resolves through a link.",
      };
    }
    return { ok: true };
  }

  return { create, remove };
}

function isNotFoundError(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}

function isExistsError(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "EEXIST";
}

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "an unknown filesystem error occurred";
}
