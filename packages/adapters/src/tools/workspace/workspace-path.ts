import { realpath, stat } from "node:fs/promises";
import path from "node:path";

export const DEFAULT_EXCLUDED_DIRECTORIES: readonly string[] = [
  "node_modules",
  ".git",
  "dist",
  "coverage",
];

export type ResolveWorkspacePathResult =
  | {
      readonly status: "resolved";
      readonly workspaceRelativePath: string;
      readonly absolutePath: string;
    }
  | {
      readonly status: "rejected";
      readonly message: string;
    };

const ABSOLUTE_PATH_PATTERN = /^(?:[A-Za-z]:)?[\\/]/;
const DRIVE_PATTERN = /^[A-Za-z]:/;

export async function resolveWorkspaceRoot(cwd: string): Promise<string> {
  let canonical: string;
  try {
    canonical = await realpath(cwd);
  } catch (error: unknown) {
    throw new Error(`Workspace root is not accessible: ${describeFsError(error)}`);
  }
  let stats;
  try {
    stats = await stat(canonical);
  } catch (error: unknown) {
    throw new Error(`Workspace root is not accessible: ${describeFsError(error)}`);
  }
  if (!stats.isDirectory()) {
    throw new Error("Workspace root is not a directory.");
  }
  return canonical;
}

export async function resolveWorkspacePath(
  root: string,
  requested: string,
): Promise<ResolveWorkspacePathResult> {
  if (requested.includes("\0")) {
    return { status: "rejected", message: "Path contains a null byte." };
  }
  if (requested.length === 0) {
    return { status: "rejected", message: "Path is empty." };
  }
  if (ABSOLUTE_PATH_PATTERN.test(requested) || DRIVE_PATTERN.test(requested)) {
    return { status: "rejected", message: "Path must be relative to the workspace." };
  }

  let canonicalRoot: string;
  try {
    canonicalRoot = await realpath(root);
  } catch (error: unknown) {
    return {
      status: "rejected",
      message: `Workspace root is not accessible: ${describeFsError(error)}`,
    };
  }

  const resolved = path.resolve(canonicalRoot, requested);
  const rootPrefix = canonicalRoot.endsWith(path.sep)
    ? canonicalRoot
    : `${canonicalRoot}${path.sep}`;
  if (resolved !== canonicalRoot && !resolved.startsWith(rootPrefix)) {
    return { status: "rejected", message: "Path is outside the Solaris workspace." };
  }

  let canonicalTarget: string;
  try {
    canonicalTarget = await realpath(resolved);
  } catch (error: unknown) {
    return { status: "rejected", message: `Path cannot be resolved: ${describeFsError(error)}` };
  }

  const targetPrefix = canonicalRoot.endsWith(path.sep)
    ? canonicalRoot
    : `${canonicalRoot}${path.sep}`;
  if (canonicalTarget !== canonicalRoot && !canonicalTarget.startsWith(targetPrefix)) {
    return { status: "rejected", message: "Path is outside the Solaris workspace." };
  }

  const relativePath =
    canonicalTarget === canonicalRoot ? "." : path.relative(canonicalRoot, canonicalTarget);
  return {
    status: "resolved",
    workspaceRelativePath: relativePath.split(path.sep).join("/"),
    absolutePath: canonicalTarget,
  };
}

export function findExcludedComponent(
  workspaceRelativePath: string,
  excludedDirectories: readonly string[],
): string | null {
  const components = workspaceRelativePath
    .split("/")
    .filter((component) => component.length > 0 && component !== ".");
  for (const component of components) {
    if (excludedDirectories.includes(component)) {
      return component;
    }
  }
  return null;
}

export function describeFsError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message.replace(/,\s*'[^']*'$/, "");
  }
  return "A filesystem error occurred.";
}
