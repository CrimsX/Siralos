/**
 * Exact Godot version model.
 *
 * The raw version text is parsed by the probe adapter (which owns
 * `--version` parsing input); the parsed model and its classification are
 * core-owned and provider-neutral. Unknown suffixes are preserved rather
 * than failing; prerelease statuses are never normalized into stable.
 */
export type GodotVersionStatus = "stable" | "rc" | "beta" | "alpha" | "dev" | "custom" | "unknown";

export interface GodotVersion {
  /** Complete bounded raw version text, sanitized for control characters. */
  readonly raw: string;
  readonly major: number;
  readonly minor: number;
  readonly patch: number | null;
  readonly status: GodotVersionStatus;
  /** Prerelease sequence number, e.g. `1` for `rc1` or `2` for `dev2`. */
  readonly statusNumber: number | null;
  /** Build token such as `official` or `custom_build`. */
  readonly build: string | null;
  /** Git commit hash token when present. */
  readonly commit: string | null;
}

export type GodotReleaseChannel =
  "stable" | "release-candidate" | "beta" | "alpha" | "development" | "custom" | "unknown";

export function classifyGodotReleaseChannel(version: GodotVersion): GodotReleaseChannel {
  switch (version.status) {
    case "stable":
      return "stable";
    case "rc":
      return "release-candidate";
    case "beta":
      return "beta";
    case "alpha":
      return "alpha";
    case "dev":
      return "development";
    case "custom":
      return "custom";
    case "unknown":
      return "unknown";
  }
}

/**
 * Version declared by a project (from `config/features` or project settings).
 * Static and non-authoritative.
 */
export interface GodotDeclaredVersion {
  readonly major: number;
  readonly minor: number;
  readonly patch: number | null;
  /** Raw declared feature token, e.g. `4.7`. */
  readonly raw: string;
}

/** Conservative static parse of a declared `major.minor[.patch]` feature token. */
export function parseDeclaredVersion(raw: string): GodotDeclaredVersion | null {
  const match = /^(\d+)\.(\d+)(?:\.(\d+))?$/.exec(raw.trim());
  if (match === null) {
    return null;
  }
  const major = Number(match[1]);
  const minor = Number(match[2]);
  const patchToken = match[3];
  if (!Number.isSafeInteger(major) || !Number.isSafeInteger(minor)) {
    return null;
  }
  return {
    major,
    minor,
    patch: patchToken === undefined ? null : Number(patchToken),
    raw: raw.trim(),
  };
}
