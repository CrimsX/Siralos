/**
 * Generic language-intelligence diagnostic model and normalization
 * (Stage 3R R5).
 *
 * The vocabulary and semantics are extracted from the reference
 * behavior (ADR 0010/0011): severities are a closed enum, missing
 * locations are never fabricated, messages are sanitized and bounded,
 * per-set and per-run collections are bounded with explicit truncation,
 * and aggregation is deterministic (exact duplicates collapsed, sorted
 * by path, line, column, then message). This module is language-neutral:
 * it never parses engine console output or LSP framing.
 */
import { toOneBasedRange } from "./position.js";
import { sanitizeControlCharacters } from "./sanitize.js";
import { truncateUtf8Bytes } from "./truncate.js";

/** Diagnostic severity vocabulary (reference: error/warning/info/unknown). */
export type DiagnosticSeverity = "error" | "warning" | "info" | "unknown";

/** One normalized, bounded, sanitized diagnostic. */
export interface LanguageDiagnostic {
  /** Source label of the producing language service. */
  readonly source: string;
  readonly severity: DiagnosticSeverity;
  /** Workspace-relative path; null when the service carries none. */
  readonly path: string | null;
  /** One-based line; null when unknown (never fabricated). */
  readonly line: number | null;
  /** One-based column; null when unknown (never fabricated). */
  readonly column: number | null;
  /** Stable diagnostic code when present; else null. */
  readonly code: string | null;
  /** Bounded, control-character-sanitized message. */
  readonly message: string;
  /** Raw category token preserved from the service; else null. */
  readonly rawCategory: string | null;
}

/** Bounds for one diagnostic payload normalization. */
export interface DiagnosticPayloadLimits {
  readonly maxDiagnostics: number;
  readonly maxMessageBytes: number;
}

/** Map an LSP severity integer to the reference vocabulary (1=error, 2=warning, 3/4=info). */
export function mapLspSeverity(value: unknown): DiagnosticSeverity {
  if (value === 1) {
    return "error";
  }
  if (value === 2) {
    return "warning";
  }
  if (value === 3 || value === 4) {
    return "info";
  }
  return "unknown";
}

function mapCode(value: unknown, maxMessageBytes: number): string | null {
  if (typeof value === "string") {
    return truncateUtf8Bytes(value, maxMessageBytes);
  }
  if (typeof value === "number") {
    return String(value);
  }
  return null;
}

function boundMessage(value: unknown, maskRoot: string | null, maxMessageBytes: number): string {
  if (typeof value !== "string") {
    return "";
  }
  let text = sanitizeControlCharacters(value).trim();
  if (maskRoot !== null && maskRoot.length > 0) {
    text = text.split(maskRoot).join("<mirror>");
  }
  return truncateUtf8Bytes(text, maxMessageBytes);
}

/**
 * Normalize one raw LSP-shaped diagnostic payload (0-based positions,
 * unknown severities, untrusted messages) into the bounded 1-based
 * model. Returns null when the payload is not an array; malformed
 * entries and empty messages are skipped conservatively. The
 * workspace-relative `path` is produced by the adapter's URI mapping
 * before this function is called.
 */
export function normalizeDiagnosticPayload(
  rawDiagnostics: unknown,
  source: string,
  path: string,
  maskRoot: string | null,
  limits: DiagnosticPayloadLimits,
): {
  readonly path: string;
  readonly diagnostics: readonly LanguageDiagnostic[];
  readonly truncated: boolean;
} | null {
  if (!Array.isArray(rawDiagnostics)) {
    return null;
  }
  const diagnostics: LanguageDiagnostic[] = [];
  let truncated = false;
  for (const entry of rawDiagnostics) {
    if (diagnostics.length >= limits.maxDiagnostics) {
      truncated = true;
      break;
    }
    if (typeof entry !== "object" || entry === null) {
      continue;
    }
    const record = entry as Record<string, unknown>;
    const range = toOneBasedRange(record["range"]);
    const severity = mapLspSeverity(record["severity"]);
    const code = mapCode(record["code"], limits.maxMessageBytes);
    const message = boundMessage(record["message"], maskRoot, limits.maxMessageBytes);
    if (message.length === 0) {
      continue;
    }
    diagnostics.push({
      source,
      severity,
      path,
      line: range?.start.line ?? null,
      column: range?.start.column ?? null,
      code,
      message,
      rawCategory: typeof record["source"] === "string" ? record["source"] : null,
    });
  }
  return { path, diagnostics, truncated };
}

/**
 * Deterministic diagnostic aggregation: exact duplicates are collapsed
 * (path, line, column, code, message), results are sorted by (path,
 * line, column, message), and the run-wide bound is applied with
 * explicit truncation.
 */
export function normalizeDiagnosticSet<T extends LanguageDiagnostic>(
  diagnostics: readonly T[],
  maxDiagnostics: number,
): { readonly diagnostics: readonly T[]; readonly truncated: boolean } {
  const seen = new Set<string>();
  const unique: T[] = [];
  for (const diagnostic of diagnostics) {
    const key = [
      diagnostic.path ?? "",
      diagnostic.line ?? -1,
      diagnostic.column ?? -1,
      diagnostic.code ?? "",
      diagnostic.message,
    ].join("\u0000");
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    unique.push(diagnostic);
  }
  unique.sort((left, right) => {
    const leftPath = left.path ?? "";
    const rightPath = right.path ?? "";
    if (leftPath !== rightPath) {
      return leftPath < rightPath ? -1 : 1;
    }
    const leftLine = left.line ?? -1;
    const rightLine = right.line ?? -1;
    if (leftLine !== rightLine) {
      return leftLine - rightLine;
    }
    const leftColumn = left.column ?? -1;
    const rightColumn = right.column ?? -1;
    if (leftColumn !== rightColumn) {
      return leftColumn - rightColumn;
    }
    const leftMessage = left.message;
    const rightMessage = right.message;
    if (leftMessage !== rightMessage) {
      return leftMessage < rightMessage ? -1 : 1;
    }
    return 0;
  });
  const truncated = unique.length > maxDiagnostics;
  return { diagnostics: unique.slice(0, maxDiagnostics), truncated };
}
