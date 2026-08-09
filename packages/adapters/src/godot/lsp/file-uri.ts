import { sep } from "node:path";

/**
 * Robust file-URI conversion for the LSP boundary.
 *
 * The LSP adapter translates workspace-relative paths to mirror `file://`
 * URIs and back. Windows drive paths, spaces, Unicode, percent encoding,
 * and POSIX paths are handled explicitly; URIs with a host component or a
 * non-file scheme are rejected; and results never expose mirror absolute
 * paths to provider-facing models.
 */

/** Converts a `file://` URI to an absolute native path, or null when unsafe. */
export function fileUriToPath(uri: string): string | null {
  if (!uri.startsWith("file://")) {
    return null;
  }
  const rest = uri.slice("file://".length);
  // A non-empty authority (file://host/path) is rejected: only the local
  // machine's paths are meaningful here.
  const authorityEnd = rest.indexOf("/");
  if (authorityEnd === -1) {
    return null;
  }
  const authority = rest.slice(0, authorityEnd);
  if (authority.length > 0 && authority !== "localhost") {
    return null;
  }
  const pathText = rest.slice(authorityEnd);
  let decoded: string;
  try {
    decoded = decodeURIComponent(pathText);
  } catch {
    return null;
  }
  // Windows drive URIs: file:///C:/dir/file.gd
  if (/^\/[a-zA-Z]:\//.test(decoded)) {
    return decoded.slice(1).replace(/\//g, "\\");
  }
  return decoded;
}

/** Converts an absolute native path to a `file://` URI. */
export function pathToFileUri(absolutePath: string): string {
  const normalized = absolutePath.replace(/\\/g, "/");
  const withScheme = normalized.startsWith("/") ? normalized : `/${normalized}`;
  return `file://${encodeURI(withScheme).replace(/#/g, "%23")}`;
}

/**
 * Converts a mirror file URI to a workspace-relative path with `/`
 * separators. Returns null when the URI is not under the mirror root or
 * cannot be decoded safely (out-of-mirror URIs are rejected, never
 * guessed).
 */
export function mirrorUriToWorkspaceRelative(uri: string, mirrorRootPath: string): string | null {
  const absolute = fileUriToPath(uri);
  if (absolute === null) {
    return null;
  }
  const mirrorRoot = normalizePath(mirrorRootPath);
  const normalized = normalizePath(absolute);
  if (normalized === mirrorRoot) {
    return null;
  }
  const prefix = `${mirrorRoot}${sep}`;
  if (!normalized.startsWith(prefix)) {
    return null;
  }
  const relative = normalized.slice(prefix.length);
  if (relative.length === 0) {
    return null;
  }
  return relative.replace(/\\/g, "/");
}

/** Converts a workspace-relative path to the mirror file URI, or null. */
export function workspaceRelativeToMirrorUri(
  relativePath: string,
  mirrorRootPath: string,
): string | null {
  if (
    relativePath.length === 0 ||
    relativePath.startsWith("/") ||
    /^[a-zA-Z]:[\\/]/.test(relativePath)
  ) {
    return null;
  }
  if (relativePath.includes("..")) {
    return null;
  }
  const joined = `${normalizePath(mirrorRootPath)}${sep}${relativePath.replace(/\//g, sep)}`;
  return pathToFileUri(joined);
}

function normalizePath(value: string): string {
  return value.replace(/[\\/]+/g, sep).replace(/[\\/]$/, "");
}
