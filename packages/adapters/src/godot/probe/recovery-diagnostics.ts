import { GODOT_LIMITS } from "@solaris/core";
import type { GodotDiagnostic } from "@solaris/core";

export interface RecoveryDiagnosticSummary {
  readonly errors: readonly GodotDiagnostic[];
  readonly warnings: readonly GodotDiagnostic[];
  readonly truncated: boolean;
}

export interface RecoveryDiagnosticLimits {
  readonly maxErrors?: number;
  readonly maxWarnings?: number;
  readonly maxRawLines?: number;
}

/**
 * Conservative, bounded classification of engine output. Engine stdout is
 * not structured truth: only lines carrying well-known Godot markers are
 * classified, every diagnostic message is control-character sanitized, and
 * the retained counts are capped so hostile or noisy output cannot flood the
 * provider result.
 */
export function classifyRecoveryDiagnostics(
  stdout: string,
  stderr: string,
  limits: RecoveryDiagnosticLimits = {},
): RecoveryDiagnosticSummary {
  const maxErrors = limits.maxErrors ?? GODOT_LIMITS.maxRecoveryDiagnostics;
  const maxWarnings = limits.maxWarnings ?? GODOT_LIMITS.maxRecoveryDiagnostics;
  const maxRawLines = limits.maxRawLines ?? GODOT_LIMITS.maxRawDiagnosticLines;
  const errors: GodotDiagnostic[] = [];
  const warnings: GodotDiagnostic[] = [];
  let truncated = false;

  const classifyLine = (line: string): void => {
    const classified = classifyDiagnosticLine(line);
    if (classified === null) {
      return;
    }
    if (classified.severity === "error") {
      if (errors.length >= maxErrors) {
        truncated = true;
        return;
      }
      errors.push(classified);
      return;
    }
    if (warnings.length >= maxWarnings) {
      truncated = true;
      return;
    }
    warnings.push(classified);
  };

  let retained = 0;
  for (const stream of [stdout, stderr]) {
    for (const rawLine of stream.split(/\r?\n/)) {
      if (rawLine.length === 0) {
        continue;
      }
      retained += 1;
      if (retained > maxRawLines) {
        truncated = true;
        return { errors, warnings, truncated };
      }
      classifyLine(rawLine);
    }
  }
  return { errors, warnings, truncated };
}

export function classifyDiagnosticLine(line: string): GodotDiagnostic | null {
  const marker =
    /^(?:SCRIPT\s+)?(ERROR|WARNING):\s*(.*)$/.exec(line) ??
    /^(?:SCRIPT\s+)?(ERROR|WARNING):(.*)$/.exec(line);
  if (marker !== null) {
    const severity = marker[1] === "ERROR" ? "error" : "warning";
    const message = sanitizeDiagnosticText(marker[2] ?? "");
    if (message.length === 0) {
      return null;
    }
    return { severity, category: classifyCategory(message), message };
  }
  const errorPatterns: ReadonlyArray<{
    readonly pattern: RegExp;
    readonly category: GodotDiagnostic["category"];
  }> = [
    { pattern: /parse\s+error/i, category: "parser" },
    { pattern: /failed\s+to\s+(load|import)|cannot\s+import|import\s+failed/i, category: "import" },
    {
      pattern: /cannot\s+open\s+file|failed\s+loading\s+resource|missing\s+resource/i,
      category: "resource",
    },
    {
      pattern: /failed\s+to\s+load\s+script|cannot\s+load\s+script|missing\s+script/i,
      category: "script",
    },
  ];
  for (const entry of errorPatterns) {
    if (entry.pattern.test(line)) {
      return {
        severity: "error",
        category: entry.category,
        message: sanitizeDiagnosticText(line),
      };
    }
  }
  return null;
}

function classifyCategory(message: string): GodotDiagnostic["category"] {
  if (/\bparse\b/i.test(message)) {
    return "parser";
  }
  if (/\bimport/i.test(message)) {
    return "import";
  }
  if (/\b(script)\b/i.test(message)) {
    return "script";
  }
  if (/\b(resource|load|open)\b/i.test(message)) {
    return "resource";
  }
  if (/\b(editor|plugin|addon)\b/i.test(message)) {
    return "editor";
  }
  return "unknown";
}

export function sanitizeDiagnosticText(text: string): string {
  let out = "";
  for (const character of text) {
    const code = character.codePointAt(0) ?? 0;
    if (code === 0x09 || code === 0x0a || code === 0x0d || code >= 0x20) {
      out += character;
    }
  }
  return out.trim();
}

export function emptyRecoveryDiagnosticSummary(): RecoveryDiagnosticSummary {
  return { errors: [], warnings: [], truncated: false };
}
