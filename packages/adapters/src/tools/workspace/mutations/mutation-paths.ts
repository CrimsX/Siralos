import { lstat, realpath } from "node:fs/promises";
import path from "node:path";
import { isProtectedBehavioralConfigPath } from "@siralos/core";
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
 * spellings are never false-rejected.
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
 * Parent-chain verification that throws on failure, for use as the
 * fail-closed hook of the rename-based commit primitives.
 */
export async function verifyParentChainIdentityOrThrow(
  workspaceRoot: string,
  absolutePath: string,
): Promise<void> {
  const result = await verifyParentChainIdentity(workspaceRoot, absolutePath);
  if (!result.ok) {
    throw new Error(result.message);
  }
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
    return { status: "rejected", message: "Path is outside the Siralos workspace." };
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
    return { status: "rejected", message: "Path is outside the Siralos workspace." };
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
    return { status: "rejected", message: "Path is outside the Siralos workspace." };
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
    components.some((component) => fold(component) === ".git" || fold(component) === ".siralos")
  ) {
    return true;
  }
  const foldedBasename = fold(basename);
  if (isProtectedBehavioralConfigPath(workspaceRelativePath)) {
    return true;
  }
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
