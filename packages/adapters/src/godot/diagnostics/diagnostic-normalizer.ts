import { GODOT_LIMITS, type GodotGDScriptDiagnostic } from "@solaris/core";
import { truncateUtf8Bytes } from "../knowledge/api-dump-with-docs.js";

/**
 * Conservative normalization of Godot `--check-only` console output.
 *
 * Godot's console output is not a formally versioned machine protocol, so
 * this parser:
 *
 * - recognizes the stable `ERROR:`/`SCRIPT ERROR:`/`WARNING:`/`SCRIPT
 *   WARNING:` prefixes and their `at:` continuation locations;
 * - recognizes inline `res://<path>:<line>:<column>` locations;
 * - normalizes mirror-absolute paths and `res://` paths to workspace-
 *   relative paths;
 * - preserves unmatched error-like lines as generic diagnostics instead of
 *   silently discarding them;
 * - never fabricates line/column values;
 * - sanitizes control characters and bounds every message;
 * - never classifies a warning as an error unless the engine output
 *   explicitly says so.
 *
 * A script parse failure is a VALID diagnostic result; exit-status
 * semantics live in the service, not here.
 */

export interface GodotCheckOutputInput {
  readonly stdout: string;
  readonly stderr: string;
  /**
   * Absolute mirror project path; when present, mirror-absolute location
   * prefixes are normalized to workspace-relative paths and never leak.
   */
  readonly mirrorProjectPath?: string | null;
}

export interface GodotCheckOutputNormalization {
  readonly diagnostics: readonly GodotGDScriptDiagnostic[];
  /** True when the per-script diagnostic bound was applied. */
  readonly truncated: boolean;
  /** Count of ignored banner/unmatched lines (never silently dropped as errors). */
  readonly unmatchedLineCount: number;
}

interface PendingDiagnostic {
  readonly severity: GodotGDScriptDiagnostic["severity"];
  readonly rawCategory: string | null;
  readonly message: string;
  readonly location: {
    readonly path: string | null;
    readonly line: number | null;
    readonly column: number | null;
  } | null;
}

const MAX_MESSAGE_BYTES = GODOT_LIMITS.maxDiagnosticMessageBytes;

export function normalizeGodotCheckOutput(
  input: GodotCheckOutputInput,
  limits: { readonly maxDiagnostics: number } = {
    maxDiagnostics: GODOT_LIMITS.maxDiagnosticsPerScript,
  },
): GodotCheckOutputNormalization {
  const combined = `${input.stdout}\n${input.stderr}`;
  const lines = combined.split(/\r?\n/);
  const diagnostics: GodotGDScriptDiagnostic[] = [];
  let pending: PendingDiagnostic | null = null;
  let unmatchedLineCount = 0;
  let overflowed = false;

  function flush(): void {
    if (pending === null) {
      return;
    }
    diagnostics.push(toDiagnostic(pending, input.mirrorProjectPath ?? null));
    pending = null;
  }

  function append(entry: PendingDiagnostic): void {
    if (diagnostics.length >= limits.maxDiagnostics) {
      overflowed = true;
      return;
    }
    pending = entry;
  }

  for (const rawLine of lines) {
    const line = rawLine;
    const trimmed = line.trim();
    if (trimmed.length === 0) {
      continue;
    }
    // `at:` continuation lines attach the engine location to the pending
    // diagnostic (e.g. `at: GDScript::reload (godot/...:1205)` or
    // `at: res://src/player/player.gd:34:17`).
    if (pending !== null && trimmed.startsWith("at:")) {
      const current: PendingDiagnostic = pending;
      const location = extractLocation(trimmed, input.mirrorProjectPath ?? null);
      if (location !== null && current.location === null) {
        pending = { ...current, location };
      }
      continue;
    }
    // A new prefixed block starts; flush any previous pending block.
    if (pending !== null) {
      flush();
    }
    const prefixed = matchPrefixed(trimmed, input.mirrorProjectPath ?? null);
    if (prefixed !== null) {
      append(prefixed);
      continue;
    }
    // Inline location lines such as
    // `res://src/player/player.gd:34:17 - Identifier "x" not declared.`
    const inline = matchInlineLocation(trimmed, input.mirrorProjectPath ?? null);
    if (inline !== null) {
      append(inline);
      continue;
    }
    // Preserve unmatched error-like lines as generic diagnostics; the
    // banner and other noise are counted, never discarded silently as
    // errors.
    const generic = matchGeneric(trimmed);
    if (generic !== null) {
      append(generic);
      continue;
    }
    unmatchedLineCount += 1;
  }
  flush();
  const truncated = overflowed || diagnostics.length > limits.maxDiagnostics;
  return {
    diagnostics: diagnostics.slice(0, limits.maxDiagnostics),
    truncated,
    unmatchedLineCount,
  };
}

function toDiagnostic(
  pending: PendingDiagnostic,
  mirrorProjectPath: string | null,
): GodotGDScriptDiagnostic {
  return {
    source: "godot-check-only",
    severity: pending.severity,
    path: pending.location?.path ?? null,
    line: pending.location?.line ?? null,
    column: pending.location?.column ?? null,
    code: extractCode(pending.message),
    message: boundMessage(pending.message, mirrorProjectPath),
    rawCategory: pending.rawCategory,
  };
}

function matchPrefixed(
  trimmed: string,
  mirrorProjectPath: string | null,
): PendingDiagnostic | null {
  const match = /^(SCRIPT ERROR|SCRIPT WARNING|ERROR|WARNING):\s?(.*)$/i.exec(trimmed);
  if (match === null) {
    return null;
  }
  const prefix = match[1]?.toUpperCase() ?? "";
  const severity: GodotGDScriptDiagnostic["severity"] = prefix.includes("WARNING")
    ? "warning"
    : "error";
  let message = match[2] ?? "";
  // Some engine builds append the location inline:
  // `ERROR: message (res://path.gd:34:17)`.
  let location: PendingDiagnostic["location"] = null;
  const inline = /\(res:\/\/[^)]+\)\s*$/.exec(message);
  if (inline !== null) {
    location = extractLocation(inline[0] ?? "", mirrorProjectPath);
    if (location !== null) {
      message = message.slice(0, inline.index).trim();
    }
  }
  return {
    severity,
    rawCategory:
      prefix === "ERROR"
        ? "error"
        : prefix === "WARNING"
          ? "warning"
          : prefix === "SCRIPT ERROR"
            ? "script-error"
            : "script-warning",
    message,
    location,
  };
}

function matchInlineLocation(
  trimmed: string,
  mirrorProjectPath: string | null,
): PendingDiagnostic | null {
  const match =
    /^res:\/\/[^\s:]+:\d+(?::\d+)?\s+-\s+(.+)$/.exec(trimmed) ??
    /^[^\s:]+:\d+(?::\d+)?\s+-\s+(.+)$/.exec(trimmed);
  if (match === null) {
    return null;
  }
  const location = extractLocation(trimmed, mirrorProjectPath);
  if (location === null) {
    return null;
  }
  const message = match[1] ?? trimmed;
  const severity: GodotGDScriptDiagnostic["severity"] = /^warning/i.test(message)
    ? "warning"
    : /^error/i.test(message)
      ? "error"
      : "unknown";
  return {
    severity,
    rawCategory: severity === "unknown" ? null : severity,
    message,
    location,
  };
}

function matchGeneric(trimmed: string): PendingDiagnostic | null {
  if (/\berror\b/i.test(trimmed)) {
    return {
      severity: "unknown",
      rawCategory: null,
      message: trimmed,
      location: null,
    };
  }
  if (/\bwarning\b/i.test(trimmed)) {
    return {
      severity: "warning",
      rawCategory: "warning",
      message: trimmed,
      location: null,
    };
  }
  return null;
}

function extractLocation(
  text: string,
  mirrorProjectPath: string | null,
): PendingDiagnostic["location"] {
  // Mirror-absolute path first (never leak the mirror root). The leaf must
  // be a script-like path; engine-internal C++ sources (e.g. `(…gdscript.cpp:1205)`)
  // are never mistaken for script locations.
  if (mirrorProjectPath !== null && mirrorProjectPath.length > 0) {
    const rootPattern = escapeRegExp(mirrorProjectPath.replace(/[\\/]+/g, "/")).replace(
      /\//g,
      "[\\\\/]",
    );
    const absolute = new RegExp(
      `${rootPattern}[\\\\/]([^\\s:()]+\\.gd):(\\d+)(?::(\\d+))?`,
      "i",
    ).exec(text);
    if (absolute !== null) {
      return {
        path: absolute[1] ?? null,
        line: toInteger(absolute[2]),
        column: absolute[3] === undefined ? null : toInteger(absolute[3]),
      };
    }
  }
  const res = /res:\/\/([^\s:()]+\.gd):(\d+)(?::(\d+))?/i.exec(text);
  if (res !== null) {
    return {
      path: res[1] ?? null,
      line: toInteger(res[2]),
      column: res[3] === undefined ? null : toInteger(res[3]),
    };
  }
  const parenthesized = /\(([^()\s:]+)\.gd:(\d+)(?::(\d+))?\)/i.exec(text);
  if (parenthesized !== null) {
    return {
      path: parenthesized[1] ?? null,
      line: toInteger(parenthesized[2]),
      column: parenthesized[3] === undefined ? null : toInteger(parenthesized[3]),
    };
  }
  return null;
}

function toInteger(value: string | undefined): number | null {
  if (value === undefined) {
    return null;
  }
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
}

function extractCode(message: string): string | null {
  if (/^(?:Parse Error:\s*)?Identifier .* not declared/i.test(message)) {
    return "undeclared-identifier";
  }
  if (/^Parse Error/i.test(message)) {
    return "parse-error";
  }
  return null;
}

function boundMessage(message: string, mirrorProjectPath: string | null): string {
  let text = message;
  // Absolute mirror roots must never leak inside message bodies, not only
  // in location fields.
  if (mirrorProjectPath !== null && mirrorProjectPath.length > 0) {
    text = text.split(mirrorProjectPath).join("<mirror>");
  }
  const sanitized = sanitizeControlCharacters(text).trim();
  return truncateUtf8Bytes(sanitized, MAX_MESSAGE_BYTES);
}

function sanitizeControlCharacters(text: string): string {
  // Regex literals and escape strings cannot carry control-character
  // sequences under the lint guard, so the patterns are assembled from
  // explicit code points (C0, DEL, and C1 0x80-0x9F).
  const csi = new RegExp(`${String.fromCharCode(27)}\\[[0-9;?]*[ -/]*[@-~]`, "g");
  const controls = new RegExp(
    `[${String.fromCharCode(0)}-${String.fromCharCode(8)}${String.fromCharCode(11)}${String.fromCharCode(12)}${String.fromCharCode(14)}-${String.fromCharCode(31)}${String.fromCharCode(127)}-${String.fromCharCode(159)}]`,
    "g",
  );
  let result = text.replace(csi, "");
  result = result.replace(controls, "\uFFFD");
  return result;
}

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
