import { lstat, realpath, unlink } from "node:fs/promises";
import type { FileHandle } from "node:fs/promises";
import path from "node:path";
import { foldPathComponent } from "../../../fs-case.js";
import { isWithinPathIdentity, samePathIdentity } from "../../../fs-path-identity.js";
import { describeFsError } from "../workspace-path.js";

export type MutationPathResult =
  | {
      readonly status: "resolved";
      readonly workspaceRelativePath: string;
      readonly absolutePath: string;
    }
  | {
      readonly status: "exists";
      readonly message: string;
    }
  | {
      readonly status: "missing";
      readonly message: string;
    }
  | {
      readonly status: "rejected";
      readonly message: string;
    };

export type ParentChainIdentityResult =
  { readonly ok: true } | { readonly ok: false; readonly message: string };

export interface ExclusiveOpenIdentityFailure {
  readonly ok: false;
  readonly message: string;
  /**
   * dev+ino of the object the exclusive open created, so the caller can
   * unlink exactly that object when the path still resolves to it.
   */
  readonly dev: bigint;
  readonly ino: bigint;
}

export type CreatedObjectCleanupOutcome = "removed" | "preserved" | "absent";

const ABSOLUTE_PATH_PATTERN = /^(?:[A-Za-z]:)?[\\/]/;
const DRIVE_PATTERN = /^[A-Za-z]:/;

export function validateRelativeWorkspacePath(requested: string): string | null {
  if (requested.includes("\0")) {
    return "Path contains a null byte.";
  }
  if (requested.length === 0) {
    return "Path is empty.";
  }
  if (ABSOLUTE_PATH_PATTERN.test(requested) || DRIVE_PATTERN.test(requested)) {
    return "Path must be relative to the workspace.";
  }
  const normalized = requested.split(/[\\/]/);
  if (normalized.some((component) => component === "..")) {
    return "Path must remain inside the workspace.";
  }
  return null;
}

/**
 * Verifies that every ancestor directory of `absolutePath` (down to the
 * workspace root) is a real directory: lstat shows a directory and never a
 * symlink/junction/reparse point, and realpath resolves to itself so no
 * mount substitution or link lies anywhere in the chain. Identity comparison
 * is platform-aware (case folding on Windows/macOS) so valid canonical
 * spellings are never false-rejected. Called immediately before the
 * exclusive-create open, because that open follows intermediate links.
 */
export async function verifyParentChainIdentity(
  workspaceRoot: string,
  absolutePath: string,
): Promise<ParentChainIdentityResult> {
  let canonicalRoot: string;
  try {
    canonicalRoot = await realpath(workspaceRoot);
  } catch (error: unknown) {
    return {
      ok: false,
      message: `Workspace root is not accessible: ${describeFsError(error)}`,
    };
  }
  const rootFailure = await verifyComponentIdentity(canonicalRoot);
  if (rootFailure !== null) {
    return { ok: false, message: `Workspace root ${rootFailure}.` };
  }
  const parent = path.dirname(absolutePath);
  const relative = path.relative(canonicalRoot, parent);
  const components = relative.split(path.sep).filter((component) => component.length > 0);
  let current = canonicalRoot;
  for (const component of components) {
    current = path.join(current, component);
    const failure = await verifyComponentIdentity(current);
    if (failure !== null) {
      return { ok: false, message: `Parent path component "${component}" ${failure}.` };
    }
  }
  return { ok: true };
}

/**
 * Proves, through the opened handle, that the object created by an
 * exclusive ("wx") open is exactly the object the path now resolves to and
 * that no link lies anywhere in the path chain: handle dev+ino must equal
 * the path's lstat dev+ino (a pathname substituted after the open is
 * detected), and realpath must both resolve to the same identity as the
 * logical path and remain inside the workspace root.
 *
 * Residual limitation (fail-closed by design): Node offers no openat-style
 * dirfd primitives, so a parent swapped between the final identity check and
 * the open syscall is detected AFTER the open but BEFORE any byte is written.
 * On failure the caller closes the handle and removes the exact created
 * object only when the path still resolves to the proven dev+ino; an
 * unprovable path is preserved and reported, never unlinked.
 */
export async function verifyExclusiveOpenIdentity(
  handle: FileHandle,
  absolutePath: string,
  canonicalRoot: string,
): Promise<{ readonly ok: true } | ExclusiveOpenIdentityFailure> {
  let openedStats;
  try {
    openedStats = await handle.stat({ bigint: true });
  } catch (error: unknown) {
    return {
      ok: false,
      message: `The opened object could not be inspected: ${describeFsError(error)}`,
      dev: 0n,
      ino: 0n,
    };
  }
  let pathStats;
  try {
    pathStats = await lstat(absolutePath, { bigint: true });
  } catch (error: unknown) {
    return {
      ok: false,
      message: `The created object's path could not be inspected after the open: ${describeFsError(error)}`,
      dev: openedStats.dev,
      ino: openedStats.ino,
    };
  }
  if (pathStats.isSymbolicLink()) {
    return {
      ok: false,
      message: "The created path now holds a symbolic link instead of the opened object",
      dev: openedStats.dev,
      ino: openedStats.ino,
    };
  }
  if (!pathStats.isFile()) {
    return {
      ok: false,
      message: "The created path no longer resolves to a regular file",
      dev: openedStats.dev,
      ino: openedStats.ino,
    };
  }
  if (pathStats.dev !== openedStats.dev || pathStats.ino !== openedStats.ino) {
    return {
      ok: false,
      message: "The created path was substituted for a different object after the open",
      dev: openedStats.dev,
      ino: openedStats.ino,
    };
  }
  let canonicalPath: string;
  try {
    canonicalPath = await realpath(absolutePath);
  } catch (error: unknown) {
    return {
      ok: false,
      message: `The created path could not be resolved canonically: ${describeFsError(error)}`,
      dev: openedStats.dev,
      ino: openedStats.ino,
    };
  }
  // realpath identity must equal the logical path identity: any junction,
  // reparse point, mount substitution, or symlink in the chain changes the
  // identity and is rejected, including one pointing back inside the
  // workspace (the object would land at the wrong logical location).
  if (!samePathIdentity(canonicalPath, absolutePath)) {
    return {
      ok: false,
      message:
        "A parent directory was swapped for a link before the open completed; the object lies outside the verified parent chain",
      dev: openedStats.dev,
      ino: openedStats.ino,
    };
  }
  if (!isWithinPathIdentity(canonicalRoot, canonicalPath)) {
    return {
      ok: false,
      message: "The created object lies outside the workspace",
      dev: openedStats.dev,
      ino: openedStats.ino,
    };
  }
  return { ok: true };
}

/**
 * Removes the exact object created by an exclusive open when (and only when)
 * the path still resolves to the proven dev+ino. Any other object at the
 * path is preserved: an uncertain target is never unlinked. `absent` means
 * the path no longer resolves to anything.
 */
export async function removeCreatedObjectIfSame(
  absolutePath: string,
  dev: bigint,
  ino: bigint,
): Promise<CreatedObjectCleanupOutcome> {
  let stats;
  try {
    stats = await lstat(absolutePath, { bigint: true });
  } catch {
    return "absent";
  }
  if (stats.dev !== dev || stats.ino !== ino) {
    return "preserved";
  }
  try {
    await unlink(absolutePath);
  } catch {
    return "preserved";
  }
  return "removed";
}

export async function resolveCreateTarget(
  workspaceRoot: string,
  requested: string,
): Promise<MutationPathResult> {
  const validation = validateRequestedPath(requested);
  if (validation !== null) {
    return { status: "rejected", message: validation };
  }
  let canonicalRoot: string;
  try {
    canonicalRoot = await realpath(workspaceRoot);
  } catch (error: unknown) {
    return {
      status: "rejected",
      message: `Workspace root is not accessible: ${describeFsError(error)}`,
    };
  }
  const resolved = path.resolve(canonicalRoot, requested);
  if (!isInside(canonicalRoot, resolved)) {
    return { status: "rejected", message: "Path is outside the Solaris workspace." };
  }
  const protectedMessage = protectedWriteMessage(resolved);
  if (protectedMessage !== null) {
    return { status: "rejected", message: protectedMessage };
  }
  const parent = path.dirname(resolved);
  const parentRelative = path.relative(canonicalRoot, parent);
  const parentComponents = parentRelative
    .split(path.sep)
    .filter((component) => component.length > 0);
  let current = canonicalRoot;
  for (const component of parentComponents) {
    current = path.join(current, component);
    const failure = await verifyComponentIdentity(current);
    if (failure !== null) {
      return { status: "rejected", message: `Parent path component "${component}" ${failure}.` };
    }
  }
  let targetExists = true;
  try {
    await lstat(resolved);
  } catch {
    targetExists = false;
  }
  if (targetExists) {
    return {
      status: "exists",
      message: "The target file already exists; reread the workspace.",
    };
  }
  return {
    status: "resolved",
    workspaceRelativePath: toWorkspaceRelative(canonicalRoot, resolved),
    absolutePath: resolved,
  };
}

export async function resolveMutationTarget(
  workspaceRoot: string,
  requested: string,
): Promise<MutationPathResult> {
  const validation = validateRequestedPath(requested);
  if (validation !== null) {
    return { status: "rejected", message: validation };
  }
  let canonicalRoot: string;
  try {
    canonicalRoot = await realpath(workspaceRoot);
  } catch (error: unknown) {
    return {
      status: "rejected",
      message: `Workspace root is not accessible: ${describeFsError(error)}`,
    };
  }
  const resolved = path.resolve(canonicalRoot, requested);
  if (!isInside(canonicalRoot, resolved)) {
    return { status: "rejected", message: "Path is outside the Solaris workspace." };
  }
  const protectedMessage = protectedWriteMessage(resolved);
  if (protectedMessage !== null) {
    return { status: "rejected", message: protectedMessage };
  }
  const relative = path.relative(canonicalRoot, resolved);
  const components = relative.split(path.sep).filter((component) => component.length > 0);
  let current = canonicalRoot;
  for (const component of components) {
    current = path.join(current, component);
    const isTarget = current === resolved;
    let stats;
    try {
      stats = await lstat(current);
    } catch (error: unknown) {
      return {
        status: "missing",
        message: `Target is missing: ${describeFsError(error)}`,
      };
    }
    if (stats.isSymbolicLink()) {
      return {
        status: "rejected",
        message:
          current === resolved
            ? "The target is a symbolic link."
            : "A parent directory is a symbolic link.",
      };
    }
    if (isTarget) {
      if (!stats.isFile()) {
        return { status: "rejected", message: "The target is not a regular file." };
      }
    } else {
      const failure = await verifyComponentIdentity(current);
      if (failure !== null) {
        return { status: "rejected", message: `Parent path component "${component}" ${failure}.` };
      }
    }
  }
  let canonicalTarget: string;
  try {
    canonicalTarget = await realpath(resolved);
  } catch (error: unknown) {
    return { status: "rejected", message: `Target cannot be resolved: ${describeFsError(error)}` };
  }
  if (!samePathIdentity(canonicalTarget, resolved)) {
    return {
      status: "rejected",
      message:
        "The target resolves to a different object (junction, reparse point, or mount substitution).",
    };
  }
  if (!isWithinPathIdentity(canonicalRoot, canonicalTarget)) {
    return { status: "rejected", message: "Path is outside the Solaris workspace." };
  }
  return {
    status: "resolved",
    workspaceRelativePath: toWorkspaceRelative(canonicalRoot, canonicalTarget),
    absolutePath: canonicalTarget,
  };
}

export function isProtectedWriteTarget(
  workspaceRelativePath: string,
  platform: NodeJS.Platform = process.platform,
): boolean {
  const components = workspaceRelativePath.split("/").filter((component) => component.length > 0);
  const basename = components.at(-1) ?? "";
  const fold = (value: string): string => foldPathComponent(value, platform);
  if (
    components.some((component) => fold(component) === ".git" || fold(component) === ".solaris")
  ) {
    return true;
  }
  const foldedBasename = fold(basename);
  return (
    foldedBasename === ".env" ||
    foldedBasename.startsWith(".env.") ||
    foldedBasename.endsWith(".pem") ||
    foldedBasename.endsWith(".key")
  );
}

function protectedWriteMessage(absolutePath: string): string | null {
  const relativePath = path.relative(path.parse(absolutePath).root, absolutePath);
  const workspaceRelative = relativePath.split(path.sep).join("/");
  return isProtectedWriteTarget(workspaceRelative)
    ? "Path is protected from workspace writes."
    : null;
}

/**
 * Canonical identity verification of one path component: it must be a real
 * directory (never a symlink/junction/reparse point) and realpath must
 * resolve to itself, so no mount substitution lies in the component's own
 * resolution either.
 */
async function verifyComponentIdentity(absolutePath: string): Promise<string | null> {
  let stats;
  try {
    stats = await lstat(absolutePath);
  } catch (error: unknown) {
    return `cannot be inspected: ${describeFsError(error)}`;
  }
  if (stats.isSymbolicLink()) {
    return "is a symbolic link";
  }
  if (!stats.isDirectory()) {
    return "is not a directory";
  }
  let canonical: string;
  try {
    canonical = await realpath(absolutePath);
  } catch (error: unknown) {
    return `cannot be resolved canonically: ${describeFsError(error)}`;
  }
  if (!samePathIdentity(canonical, absolutePath)) {
    return "resolves to a different object (junction, reparse point, or mount substitution)";
  }
  return null;
}

function validateRequestedPath(requested: string): string | null {
  if (requested.includes("\0")) {
    return "Path contains a null byte.";
  }
  if (requested.length === 0) {
    return "Path is empty.";
  }
  if (ABSOLUTE_PATH_PATTERN.test(requested) || DRIVE_PATTERN.test(requested)) {
    return "Path must be relative to the workspace.";
  }
  return null;
}

function isInside(root: string, target: string): boolean {
  const rootPrefix = root.endsWith(path.sep) ? root : `${root}${path.sep}`;
  return target === root || target.startsWith(rootPrefix);
}

function toWorkspaceRelative(root: string, target: string): string {
  const relative = path.relative(root, target);
  return relative.length === 0 ? "." : relative.split(path.sep).join("/");
}
