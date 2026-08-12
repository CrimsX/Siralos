/**
 * Filesystem case-sensitivity policy. Windows is case-insensitive;
 * macOS volumes are case-insensitive by default (conservatively treated as
 * case-insensitive everywhere, since Siralos cannot cheaply prove a given
 * volume is case-sensitive). Protected-path and exclusion comparisons fold
 * case on these platforms so `.GIT`, `.Git`, `.ENV` and equivalents can
 * never bypass protection. A platform parameter is accepted for tests.
 */
export function isCaseInsensitivePlatform(platform: NodeJS.Platform = process.platform): boolean {
  return platform === "win32" || platform === "darwin";
}

export function foldPathComponent(
  value: string,
  platform: NodeJS.Platform = process.platform,
): string {
  return isCaseInsensitivePlatform(platform) ? value.toLowerCase() : value;
}
