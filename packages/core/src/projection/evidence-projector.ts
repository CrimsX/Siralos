/**
 * Provider-neutral EvidenceProjector (Stage 3 milestone 2).
 *
 * An explicit boundary between authoritative raw evidence and the bounded,
 * sanitized representation shown to a model. The projector never modifies
 * raw evidence: the authoritative record (conversation history, task
 * evidence records, tool outputs) stays untouched, and model views are
 * disposable.
 *
 * Transformations are deterministic text operations — never an LLM
 * summarizer. Security transforms are non-revertible, repeated-line collapse
 * is optional reduction, line bounding is a mandatory structural bound, and
 * total truncation is the final hard bound.
 */

export interface EvidenceProjectionOptions {
  /** Known secret values redacted from model views. */
  readonly secrets?: readonly string[];
  /** Hard cap on the projected text representation (UTF-8 bytes). */
  readonly maxTotalBytes?: number;
  /** Hard cap on one line (UTF-8 bytes); longer lines are split. */
  readonly maxLineBytes?: number;
}

export interface ModelEvidenceView {
  /** Opaque reference to the raw evidence (never a private host path). */
  readonly evidenceId: string | null;
  /** Workspace revision handle the evidence concerned, when known. */
  readonly revision: string | null;
  readonly text: string;
  readonly truncated: boolean;
  readonly shownBytes: number;
  readonly originalBytes: number;
  /** Ordered transformation labels applied to the raw text. */
  readonly transformations: readonly string[];
}

export interface EvidenceProjector {
  projectForModel(input: {
    readonly evidenceId?: string;
    /** Workspace revision the raw evidence concerned, when known. */
    readonly revision?: string;
    readonly rawText: string;
  }): ModelEvidenceView;
}

export const DEFAULT_EVIDENCE_MAX_TOTAL_BYTES = 32 * 1024;
export const DEFAULT_EVIDENCE_MAX_LINE_BYTES = 1_024;

function isAnsiEscapeAt(text: string, index: number): boolean {
  if (text.charCodeAt(index) !== 0x1b || text.charCodeAt(index + 1) !== 0x5b) {
    return false;
  }
  for (let cursor = index + 2; cursor < text.length; cursor += 1) {
    const code = text.charCodeAt(cursor);
    if (code >= 0x40 && code <= 0x7e) {
      return true; // final byte of the CSI sequence
    }
    if (code < 0x20 || code === 0x7f) {
      return false;
    }
  }
  return false;
}

function isControlCharacter(code: number): boolean {
  return (
    (code >= 0x00 && code <= 0x08) ||
    code === 0x0b ||
    code === 0x0c ||
    (code >= 0x0e && code <= 0x1f) ||
    code === 0x7f
  );
}

/** Strip ANSI escape sequences and terminal control characters. */
export function stripAnsiAndControl(text: string): string {
  let out = "";
  let index = 0;
  while (index < text.length) {
    if (isAnsiEscapeAt(text, index)) {
      // Consume ESC + "[" plus parameter/intermediate bytes (0x20-0x3f)
      // up to the final byte (0x40-0x7e).
      index += 2;
      while (index < text.length) {
        const code = text.charCodeAt(index);
        index += 1;
        if (code >= 0x40 && code <= 0x7e) {
          break; // final byte consumed
        }
        if (code < 0x20 || code === 0x7f) {
          break; // malformed sequence; stop consuming
        }
      }
      continue;
    }
    const code = text.charCodeAt(index);
    if (isControlCharacter(code)) {
      index += 1;
      continue;
    }
    out += text[index];
    index += 1;
  }
  return out;
}

/** Collapse 3+ consecutive identical lines into one line + "×N". */
export function collapseRepeatedLines(text: string): string {
  const lines = text.split("\n");
  const out: string[] = [];
  let index = 0;
  while (index < lines.length) {
    const line = lines[index] as string;
    let run = 1;
    while (index + run < lines.length && lines[index + run] === line) {
      run += 1;
    }
    if (run >= 3) {
      out.push(`${line} \u00D7${run}`);
    } else {
      for (let repeat = 0; repeat < run; repeat += 1) {
        out.push(line);
      }
    }
    index += run;
  }
  return out.join("\n");
}

/** Replace configured secret values with a fixed placeholder. */
export function redactSecrets(text: string, secrets: readonly string[]): string {
  let out = text;
  for (const secret of secrets) {
    if (secret.length === 0) {
      continue;
    }
    out = out.split(secret).join("\u2588\u2588\u2588[REDACTED]\u2588\u2588\u2588");
  }
  return out;
}

/**
 * Return UTF-16 code-unit offsets that are safe Unicode-scalar boundaries.
 *
 * JavaScript strings may contain lone surrogates, so those remain individual
 * boundaries. A valid surrogate pair is kept together; slicing between its
 * two code units would create a new malformed string and would make the
 * UTF-8 byte bound inaccurate.
 */
function unicodeBoundaries(text: string): number[] {
  const boundaries = [0];
  for (let index = 0; index < text.length; index += 1) {
    const code = text.charCodeAt(index);
    const next = text.charCodeAt(index + 1);
    if (code >= 0xd800 && code <= 0xdbff && next >= 0xdc00 && next <= 0xdfff) {
      index += 1;
    }
    boundaries.push(index + 1);
  }
  return boundaries;
}

/** Split lines longer than the bound without creating surrogate fragments. */
export function boundLineLength(text: string, maxLineBytes: number): string {
  const encoder = new TextEncoder();
  const out: string[] = [];
  for (const line of text.split("\n")) {
    if (encoder.encode(line).length <= maxLineBytes) {
      out.push(line);
      continue;
    }
    let remaining = line;
    while (encoder.encode(remaining).length > maxLineBytes) {
      const boundaries = unicodeBoundaries(remaining);
      const firstBoundary = boundaries[1];
      if (firstBoundary === undefined) {
        break;
      }
      // If even one scalar cannot fit, retain that scalar rather than
      // splitting it. The bound is impossible for that scalar, but the
      // returned text remains well-formed and progress is guaranteed.
      if (encoder.encode(remaining.slice(0, firstBoundary)).length > maxLineBytes) {
        out.push(remaining.slice(0, firstBoundary));
        remaining = remaining.slice(firstBoundary);
        continue;
      }
      // Binary search for the largest Unicode-scalar prefix that fits.
      let low = 1;
      let high = boundaries.length - 1;
      while (low < high) {
        const mid = Math.ceil((low + high) / 2);
        const end = boundaries[mid] as number;
        if (encoder.encode(remaining.slice(0, end)).length <= maxLineBytes) {
          low = mid;
        } else {
          high = mid - 1;
        }
      }
      const end = boundaries[low] as number;
      out.push(remaining.slice(0, end));
      remaining = remaining.slice(end);
    }
    if (remaining.length > 0) {
      out.push(remaining);
    }
  }
  return out.join("\n");
}

/** Deterministic truncation with an explicit marker and byte metadata. */
export function truncateText(text: string, maxBytes: number): { text: string; truncated: boolean } {
  const encoder = new TextEncoder();
  if (encoder.encode(text).length <= maxBytes) {
    return { text, truncated: false };
  }
  const marker = "\n\u2026 [truncated]";
  const boundaries = unicodeBoundaries(text);
  // Binary search for the largest Unicode-scalar prefix that fits with the
  // marker. If the marker itself is larger than the budget, preserving the
  // marker remains the explicit truncation contract.
  let low = 0;
  let high = boundaries.length - 1;
  while (low < high) {
    const mid = Math.ceil((low + high) / 2);
    const end = boundaries[mid] as number;
    if (encoder.encode(text.slice(0, end) + marker).length <= maxBytes) {
      low = mid;
    } else {
      high = mid - 1;
    }
  }
  const end = boundaries[low] as number;
  return { text: text.slice(0, end) + marker, truncated: true };
}

export function createEvidenceProjector(
  options: EvidenceProjectionOptions = {},
): EvidenceProjector {
  const maxTotalBytes = options.maxTotalBytes ?? DEFAULT_EVIDENCE_MAX_TOTAL_BYTES;
  const maxLineBytes = options.maxLineBytes ?? DEFAULT_EVIDENCE_MAX_LINE_BYTES;
  const secrets = options.secrets ?? [];

  return {
    projectForModel(input: {
      readonly evidenceId?: string;
      readonly revision?: string;
      readonly rawText: string;
    }): ModelEvidenceView {
      const originalBytes = new TextEncoder().encode(input.rawText).length;
      let transformations: string[] = [];
      let text = input.rawText;
      // Security transforms first; they are never reverted by size rules.
      const stripped = stripAnsiAndControl(text);
      if (stripped !== text) {
        text = stripped;
        transformations.push("strip-ansi-control");
      }
      if (secrets.some((secret) => secret.length > 0 && text.includes(secret))) {
        text = redactSecrets(text, secrets);
        transformations.push("redact-secrets");
      }
      // Never-worse rule for the optional size-reduction transform. Security
      // transforms above stay applied; the structural line bound below is
      // never reverted merely because it inserts separators.
      const preReduction = text;
      const collapsed = collapseRepeatedLines(text);
      let collapseApplied = false;
      if (collapsed !== text && collapsed.length <= text.length) {
        text = collapsed;
        transformations.push("collapse-repeated-lines");
        collapseApplied = true;
      }
      const bounded = boundLineLength(text, maxLineBytes);
      if (bounded !== text) {
        text = bounded;
        transformations.push("bound-lines");
      }
      if (collapseApplied && new TextEncoder().encode(text).length > originalBytes) {
        // Discard only the optional collapse. Reapply the mandatory structural
        // bound to the post-security text so every feasible provider-visible
        // line remains within maxLineBytes.
        text = boundLineLength(preReduction, maxLineBytes);
        transformations = transformations.filter(
          (transformation) =>
            transformation !== "collapse-repeated-lines" && transformation !== "bound-lines",
        );
        if (text !== preReduction) {
          transformations.push("bound-lines");
        }
      }
      // Truncation is the final deterministic bound and always shrinks.
      const truncated = truncateText(text, maxTotalBytes);
      if (truncated.truncated) {
        text = truncated.text;
        transformations.push("truncate");
      }
      return {
        evidenceId: input.evidenceId ?? null,
        revision: input.revision ?? null,
        text,
        truncated: truncated.truncated,
        shownBytes: new TextEncoder().encode(text).length,
        originalBytes,
        transformations,
      };
    },
  };
}
