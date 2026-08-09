import { realpath } from "node:fs/promises";
import path from "node:path";
import { isPathWithin } from "@solaris/core";
import { describeFsError } from "../tools/workspace/workspace-path.js";

/**
 * Reference-root-relative path resolution (Stage 3 milestone 5).
 *
 * The same class of containment as `workspace-path.ts`, applied to an
 * external reference root instead of the workspace root: the requested
 * path must be relative, must not traverse with `..` (checked after
 * segment normalization, so `a/../b` inside the root is fine while any
 * surviving `..` is rejected), must not contain null bytes, and the
 * realpath-resolved target must stay inside the canonical reference root
 * (prefix + separator boundary). Symlink escapes are rejected because the
 * canonical target is compared against the canonical root; when the target
 * itself does not exist, the deepest existing ancestor is realpath'd and
 * checked so an escape through a parent symlink is still named precisely.
 * Windows symlink-unsupported cases fail closed: any realpath failure
 * rejects the path.
 *
 * Backslashes are treated as separators everywhere (matching the core
 * registry's normalization) so a Windows-style path can never slip in as
 * a single filename component on POSIX.
 */

export type ResolveReferencePathResult =
  | {
      readonly ok: true;
      /** Canonical absolute path of the target (realpath'd). */
      readonly resolved: string;
      /** Reference-relative path with forward slashes; "." for the root. */
      readonly relative: string;
    }
  | {
      readonly ok: false;
      readonly reason: string;
    };

const ABSOLUTE_PATH_PATTERN = /^(?:[A-Za-z]:)?[\\/]/;
const DRIVE_PATTERN = /^[A-Za-z]:/;

/**
 * Normalize the requested relative path into forward-slash segments,
 * resolving "." and rejecting any ".." that survives normalization (i.e.
 * any that would traverse above the reference root).
 */
function normalizeRelativeSegments(
  requested: string,
): { readonly ok: true; readonly value: string } | { readonly ok: false; readonly reason: string } {
  const segments = requested.split(/[\\/]/);
  const out: string[] = [];
  for (const segment of segments) {
    if (segment === "" || segment === ".") {
      continue;
    }
    if (segment === "..") {
      if (out.length === 0) {
        return { ok: false, reason: "Path must not traverse outside the reference root." };
      }
      out.pop();
      continue;
    }
    out.push(segment);
  }
  return { ok: true, value: out.length === 0 ? "." : out.join("/") };
}

function isWithinBoundary(canonicalRoot: string, target: string): boolean {
  const rootPrefix = canonicalRoot.endsWith(path.sep)
    ? canonicalRoot
    : `${canonicalRoot}${path.sep}`;
  return target === canonicalRoot || target.startsWith(rootPrefix);
}

/**
 * Realpath the deepest existing ancestor of `target`. Used only to name
 * symlink escapes precisely when the target itself does not exist.
 */
async function realpathDeepestExisting(target: string): Promise<string | null> {
  let current = target;
  for (;;) {
    try {
      return await realpath(current);
    } catch {
      const parent = path.dirname(current);
      if (parent === current) {
        return null;
      }
      current = parent;
    }
  }
}

export async function resolveReferencePath(
  root: string,
  requested: string,
): Promise<ResolveReferencePathResult> {
  if (requested.includes("\0")) {
    return { ok: false, reason: "Path contains a null byte." };
  }
  if (requested.length === 0) {
    return { ok: false, reason: "Path is empty." };
  }
  if (ABSOLUTE_PATH_PATTERN.test(requested) || DRIVE_PATTERN.test(requested)) {
    return { ok: false, reason: "Path must be relative to the reference root." };
  }
  const normalized = normalizeRelativeSegments(requested);
  if (!normalized.ok) {
    return normalized;
  }

  let canonicalRoot: string;
  try {
    canonicalRoot = await realpath(root);
  } catch (error: unknown) {
    return {
      ok: false,
      reason: `Reference root is not accessible: ${describeFsError(error)}`,
    };
  }

  const resolved = path.resolve(canonicalRoot, normalized.value);
  if (!isWithinBoundary(canonicalRoot, resolved)) {
    return { ok: false, reason: "Path is outside the reference root." };
  }

  let canonicalTarget: string;
  try {
    canonicalTarget = await realpath(resolved);
  } catch (error: unknown) {
    // The target does not exist (or a component is unreadable). Before
    // accepting, check the deepest existing ancestor so an escape through
    // a parent symlink is rejected with a precise reason even when the
    // final component is missing.
    const deepest = await realpathDeepestExisting(resolved);
    if (deepest !== null && !isWithinBoundary(canonicalRoot, deepest)) {
      return { ok: false, reason: "Path is outside the reference root (symlink escape)." };
    }
    if (deepest === null) {
      return { ok: false, reason: `Path cannot be resolved: ${describeFsError(error)}` };
    }
    // Missing but lexically inside the root: return the unresolved path so
    // callers can produce a precise not_found result.
    const relative = path.relative(canonicalRoot, resolved).split(path.sep).join("/");
    return { ok: true, resolved, relative };
  }

  if (!isWithinBoundary(canonicalRoot, canonicalTarget)) {
    return { ok: false, reason: "Path is outside the reference root." };
  }

  const relative =
    canonicalTarget === canonicalRoot ? "." : path.relative(canonicalRoot, canonicalTarget);
  return { ok: true, resolved: canonicalTarget, relative: relative.split(path.sep).join("/") };
}

/**
 * Whether a reference root is inside the workspace namespace. The registry
 * already refuses such references at resolution and refresh; this adapter
 * helper is for access-time root verification (defense in depth).
 */
export function isReferenceRootWithin(root: string, workspaceRoot: string): boolean {
  return isPathWithin(workspaceRoot, root);
}

/**
 * Throw when a reference root is inside the workspace namespace. Used by
 * registry callers to reject references inside the workspace at access
 * time; the registry performs its own check at resolution and refresh.
 */
export function assertReferenceRoot(root: string, workspaceRoot: string): void {
  if (isReferenceRootWithin(root, workspaceRoot)) {
    throw new Error("Reference root must be outside the workspace namespace.");
  }
}
