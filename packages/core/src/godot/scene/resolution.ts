import { GODOT_SCENE_LIMITS } from "./limits.js";

/**
 * Conservative `res://` path resolution (Stage 3 milestone 8).
 *
 * Resolves a Godot `res://` reference to a workspace-relative path when
 * the reference is verifiably contained (no traversal, no absolute
 * components, no alternate forms). Unresolvable references stay
 * unresolved — identity is never invented.
 */

export const UID_PATTERN = /^uid:\/\/[0-9a-z]+$/i;

export type ResPathResolution =
  | { readonly ok: true; readonly relativePath: string }
  | { readonly ok: false; readonly reason: string };

/** Resolve `res://...` to a workspace-relative path, or report why not. */
export function resolveResPath(reference: string): ResPathResolution {
  if (!reference.startsWith("res://")) {
    return { ok: false, reason: "Not a res:// reference." };
  }
  const relative = reference.slice("res://".length);
  if (relative.length === 0) {
    return { ok: false, reason: "Empty res:// reference." };
  }
  if (relative.length > GODOT_SCENE_LIMITS.maxDocumentBytes) {
    return { ok: false, reason: "Reference exceeds the path length bound." };
  }
  if (relative.includes("\0") || relative.includes("\\") || relative.includes(":")) {
    return { ok: false, reason: "Reference contains an unsupported path form." };
  }
  const segments = relative.split("/");
  if (segments.some((segment) => segment === ".." || segment === "." || segment.length === 0)) {
    return { ok: false, reason: "Reference is not a contained relative path." };
  }
  return { ok: true, relativePath: relative };
}

/** True when the string is a well-formed `uid://...` identity. */
export function isGodotUid(value: string): boolean {
  return UID_PATTERN.test(value);
}
