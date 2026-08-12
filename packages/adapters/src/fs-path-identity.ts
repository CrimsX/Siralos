import { isCaseInsensitivePlatform } from "./fs-case.js";

/**
 * Platform-aware path identity and containment.
 *
 * Filesystem identity is not string equality: on Windows the same object
 * can be spelled with different case, with either separator, with or
 * without an extended-length prefix, and drive letters may differ in case.
 * Every "is this the same canonical path" comparison in Siralos funnels
 * through this module so a raw `realpath(a) === b` comparison can never
 * reject a valid canonical spelling or accept a different object.
 *
 * The helper compares only *identity spellings*: callers must still
 * canonicalize with `realpath` (or build the logical path with
 * `path.resolve`) before comparing, and must keep their own no-follow
 * discipline — this module never weakens a link check.
 */

/**
 * Windows extended-length/device prefixes that do not change identity.
 * The extended-length UNC form (`\\?\UNC\server\share\...`) denotes the
 * same object as `\\server\share\...` and is converted before the prefix
 * itself is stripped.
 */
const WINDOWS_IDENTITY_PREFIXES: readonly string[] = ["\\\\?\\", "\\\\.\\"];

/** Extended-length or device UNC prefix denoting a plain UNC path. */
const WINDOWS_EXTENDED_UNC_PATTERN = /^\\\\(?:\?|\.)\\UNC\\/i;

/**
 * Normalizes a path into its canonical identity spelling for comparison.
 * Never used for display, construction, or filesystem access.
 */
export function normalizePathIdentity(
  value: string,
  platform: NodeJS.Platform = process.platform,
): string {
  let normalized = value;
  if (platform === "win32") {
    if (WINDOWS_EXTENDED_UNC_PATTERN.test(normalized)) {
      normalized = normalized.replace(WINDOWS_EXTENDED_UNC_PATTERN, "\\\\");
    } else {
      for (const prefix of WINDOWS_IDENTITY_PREFIXES) {
        if (normalized.startsWith(prefix)) {
          normalized = normalized.slice(prefix.length);
          break;
        }
      }
    }
    normalized = normalized.replaceAll("/", "\\");
    // Collapse repeated separators (keeping a leading UNC pair).
    normalized = normalized.replace(/\\{2,}/g, (_, offset: number) =>
      offset === 0 ? "\\\\" : "\\",
    );
    // Normalize drive-letter casing.
    if (/^[A-Za-z]:/.test(normalized)) {
      normalized = normalized.slice(0, 1).toUpperCase() + normalized.slice(1);
    }
  }
  if (isCaseInsensitivePlatform(platform)) {
    normalized = normalized.toLowerCase();
  }
  const isRoot =
    normalized === "\\" ||
    normalized === "/" ||
    (platform === "win32" && /^[a-z]:[\\/]?$/i.test(normalized)) ||
    (platform === "win32" && /^\\\\[^\\]+\\[^\\]+$/.test(normalized));
  if (isRoot) {
    // A bare drive letter and a drive root with a separator denote the same
    // object: unify them so `C:` and `c:\` compare equal.
    if (platform === "win32" && /^[a-z]:$/i.test(normalized)) {
      normalized = `${normalized}\\`;
    }
  } else if (normalized.endsWith("\\") || normalized.endsWith("/")) {
    normalized = normalized.slice(0, -1);
  }
  return normalized;
}

/**
 * True when both spellings denote the same path identity on the platform.
 */
export function samePathIdentity(
  left: string,
  right: string,
  platform: NodeJS.Platform = process.platform,
): boolean {
  return normalizePathIdentity(left, platform) === normalizePathIdentity(right, platform);
}

/**
 * True when `target` is `root` itself or a descendant of `root`, with a
 * separator boundary so `C:\foo` never contains `C:\foobar`, and with the
 * platform's case policy applied.
 */
export function isWithinPathIdentity(
  root: string,
  target: string,
  platform: NodeJS.Platform = process.platform,
): boolean {
  const normalizedRoot = normalizePathIdentity(root, platform);
  const normalizedTarget = normalizePathIdentity(target, platform);
  if (normalizedRoot === normalizedTarget) {
    return true;
  }
  const separator = platform === "win32" ? "\\" : "/";
  const rootPrefix = normalizedRoot.endsWith(separator)
    ? normalizedRoot
    : `${normalizedRoot}${separator}`;
  return normalizedTarget.startsWith(rootPrefix);
}
