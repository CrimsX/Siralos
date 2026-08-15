/**
 * Generic source positions and ranges (Stage 3R R5).
 *
 * Siralos external positions are 1-based: line and column both start at
 * 1, matching the CLI and the check-only diagnostic convention. LSP
 * line/character positions are 0-based and are converted to this
 * convention explicitly at the adapter boundary (never silently mixed).
 *
 * Malformed external positions are rejected with null, never fabricated
 * and never panicked on.
 */

export interface LanguagePosition {
  readonly line: number;
  readonly column: number;
}

export interface LanguageRange {
  readonly start: LanguagePosition;
  readonly end: LanguagePosition;
}

/**
 * Convert an unknown LSP position ({ line, character }, both 0-based
 * non-negative integers) to a 1-based Siralos position, or null when
 * malformed.
 */
export function toOneBasedPosition(position: unknown): LanguagePosition | null {
  if (typeof position !== "object" || position === null) {
    return null;
  }
  const line = (position as Record<string, unknown>)["line"];
  const character = (position as Record<string, unknown>)["character"];
  if (typeof line !== "number" || typeof character !== "number") {
    return null;
  }
  if (!Number.isInteger(line) || !Number.isInteger(character) || line < 0 || character < 0) {
    return null;
  }
  return { line: line + 1, column: character + 1 };
}

/**
 * Convert an unknown LSP range ({ start, end }) to a 1-based Siralos
 * range, or null when either position is malformed.
 */
export function toOneBasedRange(range: unknown): LanguageRange | null {
  if (typeof range !== "object" || range === null) {
    return null;
  }
  const start = toOneBasedPosition((range as Record<string, unknown>)["start"]);
  const end = toOneBasedPosition((range as Record<string, unknown>)["end"]);
  if (start === null || end === null) {
    return null;
  }
  return { start, end };
}

/** True when the position is a valid 1-based Siralos position. */
export function isOneBasedPosition(position: LanguagePosition): boolean {
  return (
    Number.isInteger(position.line) &&
    Number.isInteger(position.column) &&
    position.line >= 1 &&
    position.column >= 1
  );
}

/** True when the range is valid under the 1-based reference ordering. */
export function isOrderedRange(range: LanguageRange): boolean {
  if (!isOneBasedPosition(range.start) || !isOneBasedPosition(range.end)) {
    return false;
  }
  if (range.start.line > range.end.line) {
    return false;
  }
  if (range.start.line === range.end.line && range.start.column > range.end.column) {
    return false;
  }
  return true;
}
