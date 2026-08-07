import { lstat, realpath } from "node:fs/promises";
import path from "node:path";
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

const ABSOLUTE_PATH_PATTERN = /^(?:[A-Za-z]:)?[\\/]/;
const DRIVE_PATTERN = /^[A-Za-z]:/;

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
    let stats;
    try {
      stats = await lstat(current);
    } catch (error: unknown) {
      return {
        status: "rejected",
        message: `Parent directory is missing: ${describeFsError(error)}`,
      };
    }
    if (stats.isSymbolicLink()) {
      return { status: "rejected", message: "A parent directory is a symbolic link." };
    }
    if (!stats.isDirectory()) {
      return { status: "rejected", message: "A parent path component is not a directory." };
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
    if (current === resolved && !stats.isFile()) {
      return { status: "rejected", message: "The target is not a regular file." };
    }
  }
  let canonicalTarget: string;
  try {
    canonicalTarget = await realpath(resolved);
  } catch (error: unknown) {
    return { status: "rejected", message: `Target cannot be resolved: ${describeFsError(error)}` };
  }
  if (!isInside(canonicalRoot, canonicalTarget)) {
    return { status: "rejected", message: "Path is outside the Solaris workspace." };
  }
  return {
    status: "resolved",
    workspaceRelativePath: toWorkspaceRelative(canonicalRoot, canonicalTarget),
    absolutePath: canonicalTarget,
  };
}

export function isProtectedWriteTarget(workspaceRelativePath: string): boolean {
  const components = workspaceRelativePath.split("/").filter((component) => component.length > 0);
  const basename = components.at(-1) ?? "";
  const fold =
    process.platform === "win32"
      ? (value: string) => value.toLowerCase()
      : (value: string) => value;
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
