import type { GodotVersion, GodotVersionStatus } from "@siralos/core";

export type GodotVersionParseResult =
  | { readonly ok: true; readonly version: GodotVersion }
  | { readonly ok: false; readonly message: string };

const STATUS_PATTERN = /^(stable|rc|beta|alpha|dev|custom_build|custom)(\d*)$/;
const COMMIT_PATTERN = /^[0-9a-f]{7,40}$/;

/**
 * Flexible, adversarial-safe parser for Godot `--version` output.
 *
 * Supported forms include `4.7.1.stable.official`,
 * `4.7.2.rc1.official`, `4.8.dev2.custom_build`, patchless versions, and
 * versions carrying a commit-hash token. Non-numeric major/minor values are
 * rejected, empty and non-Godot output fails, control characters are
 * sanitized, and unknown suffixes are preserved rather than failing.
 */
export function parseGodotVersionText(raw: string): GodotVersionParseResult {
  const sanitized = sanitizeControlCharacters(raw);
  const firstLine = sanitized.split(/\r?\n/, 1)[0]?.trim() ?? "";
  if (firstLine.length === 0) {
    return { ok: false, message: "The Godot version output is empty." };
  }
  const body = stripLeadingGodotPrefix(firstLine);
  if (body.length === 0) {
    return { ok: false, message: "The Godot version output is not recognizable." };
  }
  const segments = body.split(".");
  const major = Number(segments[0]);
  const minor = Number(segments[1]);
  if (!isSegmentInteger(segments[0]) || !isSegmentInteger(segments[1])) {
    return { ok: false, message: "The Godot version has a non-numeric major or minor." };
  }
  if (!Number.isSafeInteger(major) || !Number.isSafeInteger(minor)) {
    return { ok: false, message: "The Godot version numbers are out of range." };
  }
  const rest = segments.slice(2);
  let patch: number | null = null;
  if (rest.length > 0 && isSegmentInteger(rest[0])) {
    patch = Number(rest[0]);
    rest.shift();
  }
  let status: GodotVersionStatus = "unknown";
  let statusNumber: number | null = null;
  let build: string | null = null;
  let commit: string | null = null;
  for (const token of rest) {
    if (token === undefined) {
      continue;
    }
    if (status === "unknown") {
      const statusMatch = STATUS_PATTERN.exec(token);
      if (statusMatch !== null) {
        const statusToken = statusMatch[1] as string;
        status = statusToken === "custom_build" ? "custom" : (statusToken as GodotVersionStatus);
        const numberToken = statusMatch[2];
        statusNumber =
          numberToken !== undefined && numberToken.length > 0 ? Number(numberToken) : null;
        if (status === "custom" && build === null) {
          build = token;
        }
        continue;
      }
    }
    if (commit === null && COMMIT_PATTERN.test(token)) {
      commit = token;
      continue;
    }
    if (build === null) {
      build = token;
    }
  }
  return {
    ok: true,
    version: {
      raw: firstLine,
      major,
      minor,
      patch,
      status,
      statusNumber,
      build,
      commit,
    },
  };
}

function stripLeadingGodotPrefix(line: string): string {
  const match = /^godot(?:\s+engine)?(?:\s+v)?\s*/i.exec(line);
  return match === null ? line : line.slice(match[0].length);
}

function isSegmentInteger(segment: string | undefined): boolean {
  return segment !== undefined && /^\d+$/.test(segment);
}

export function sanitizeControlCharacters(text: string): string {
  let result = "";
  for (const character of text) {
    const code = character.codePointAt(0) ?? 0;
    if (code < 0x20 || code === 0x7f) {
      if (character === "\n" || character === "\r" || character === "\t") {
        result += character;
      } else {
        result += "\uFFFD";
      }
    } else {
      result += character;
    }
  }
  return result;
}
