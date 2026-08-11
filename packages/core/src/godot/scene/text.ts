/**
 * Small purpose-built lexical helpers for Godot text resources
 * (Stage 3 milestone 8).
 *
 * Godot scene/resource files are line-oriented `section` / `key=value`
 * records whose values may contain quoted strings, escaped characters,
 * nested arrays/dictionaries, comments, and multi-line structures. A
 * fragile collection of line regexes cannot handle those correctly, so
 * these helpers do real scanning: quotes and escapes are honored, nesting
 * depth is tracked, and every scan is bounded. Nothing here evaluates
 * expressions or executes project code.
 */

/** Max nesting depth tracked by the balance scanner (defense in depth). */
const MAX_TRACKED_NESTING = 256;

export interface BalancedScan {
  /** Index one past the last character of the balanced region. */
  readonly endIndex: number;
  /** True when the region ended at true balance (not EOF/limit). */
  readonly balanced: boolean;
  /** True when an unterminated string was encountered at the end. */
  readonly unterminatedString: boolean;
  /** True when the nesting-depth bound was hit. */
  readonly depthExceeded: boolean;
}

/**
 * Scan `text` from `start` and return the end of the region that keeps
 * every `(`, `[`, `{` balanced, honoring quoted strings and escape
 * sequences. Used to (a) detect whether a `key=value` record continues on
 * following lines and (b) find the true extent of one header attribute
 * value such as `ExtResource("1")` or `groups=["a","b"]`.
 */
export function scanBalanced(text: string, start: number): BalancedScan {
  let index = start;
  const stack: string[] = [];
  let inString = false;
  let stringQuote = "";
  while (index < text.length) {
    const character = text[index] as string;
    if (inString) {
      if (character === "\\") {
        index += 2;
        continue;
      }
      if (character === stringQuote) {
        inString = false;
      }
      index += 1;
      continue;
    }
    if (character === '"' || character === "'") {
      inString = true;
      stringQuote = character;
      index += 1;
      continue;
    }
    if (character === "(" || character === "[" || character === "{") {
      if (stack.length >= MAX_TRACKED_NESTING) {
        return { endIndex: index, balanced: false, unterminatedString: false, depthExceeded: true };
      }
      stack.push(character);
      index += 1;
      continue;
    }
    const closing = closingFor(character);
    if (closing !== null) {
      const open = stack.pop();
      if (open !== closing) {
        // Mismatched closer: treat the region as ended before it so the
        // caller can diagnose; never consume across a malformed boundary.
        return {
          endIndex: index,
          balanced: false,
          unterminatedString: false,
          depthExceeded: false,
        };
      }
      index += 1;
      continue;
    }
    if (stack.length === 0 && (character === " " || character === "\t")) {
      // A bare space at depth zero ends a header attribute value.
      return { endIndex: index, balanced: true, unterminatedString: false, depthExceeded: false };
    }
    index += 1;
  }
  return {
    endIndex: index,
    balanced: stack.length === 0 && !inString,
    unterminatedString: inString,
    depthExceeded: false,
  };
}

function closingFor(character: string): string | null {
  if (character === ")") {
    return "(";
  }
  if (character === "]") {
    return "[";
  }
  if (character === "}") {
    return "{";
  }
  return null;
}

export interface HeaderAttribute {
  readonly name: string;
  /** Exact raw value text (may be quoted; may contain spaces). */
  readonly valueText: string;
  /** True when the value is a double-quoted string literal. */
  readonly quoted: boolean;
  /** One-based offset hint within the section header (unused by parsers). */
  readonly startIndex: number;
}

/**
 * Parse `name=value` attribute pairs from a section header such as
 * `name="Player" type="CharacterBody2D" parent="."` or
 * `instance=ExtResource("1") groups=["a","b"]`. Values run until the next
 * top-level space; quoted strings may contain spaces and escapes.
 * Returns a `truncated` flag when the attribute bound is exceeded.
 */
export function parseHeaderAttributes(
  headerText: string,
  maxAttributes: number,
): { readonly attributes: readonly HeaderAttribute[]; readonly truncated: boolean } {
  const attributes: HeaderAttribute[] = [];
  let index = 0;
  let truncated = false;
  while (index < headerText.length) {
    while (index < headerText.length && (headerText[index] === " " || headerText[index] === "\t")) {
      index += 1;
    }
    if (index >= headerText.length) {
      break;
    }
    if (attributes.length >= maxAttributes) {
      truncated = true;
      break;
    }
    // Attribute name runs until `=` or whitespace.
    const nameStart = index;
    while (
      index < headerText.length &&
      headerText[index] !== "=" &&
      headerText[index] !== " " &&
      headerText[index] !== "\t"
    ) {
      index += 1;
    }
    if (index >= headerText.length || headerText[index] !== "=") {
      // Bare token without a value: preserve it as a boolean-ish attribute.
      const name = headerText.slice(nameStart, index).trim();
      if (name.length > 0) {
        attributes.push({ name, valueText: "", quoted: false, startIndex: nameStart });
      }
      continue;
    }
    const name = headerText.slice(nameStart, index).trim();
    index += 1; // consume "="
    while (index < headerText.length && (headerText[index] === " " || headerText[index] === "\t")) {
      index += 1;
    }
    if (index >= headerText.length) {
      attributes.push({ name, valueText: "", quoted: false, startIndex: nameStart });
      break;
    }
    const quoted = headerText[index] === '"';
    const scan = scanBalanced(headerText, index);
    const valueText = headerText.slice(index, scan.endIndex).trim();
    attributes.push({ name, valueText, quoted, startIndex: nameStart });
    index = scan.endIndex;
  }
  return { attributes, truncated };
}

/**
 * Split one `key=value` record line at the first `=` outside quotes.
 * Keys may themselves be quoted (`"custom/name"`). Returns null when no
 * top-level `=` exists.
 */
export function splitKeyValue(
  line: string,
): { readonly key: string; readonly valueStart: number } | null {
  let index = 0;
  let inString = false;
  let quote = "";
  while (index < line.length) {
    const character = line[index] as string;
    if (inString) {
      if (character === "\\") {
        index += 2;
        continue;
      }
      if (character === quote) {
        inString = false;
      }
      index += 1;
      continue;
    }
    if (character === '"' || character === "'") {
      inString = true;
      quote = character;
      index += 1;
      continue;
    }
    if (character === "=") {
      return { key: line.slice(0, index).trim(), valueStart: index + 1 };
    }
    index += 1;
  }
  return null;
}

/** Whether a trimmed line is a whole-line comment (`;` or `#`). */
export function isCommentLine(trimmed: string): boolean {
  return trimmed.startsWith(";") || trimmed.startsWith("#");
}

/** True when `text` has zero net nesting depth outside strings. */
export function isBalancedText(text: string): boolean {
  const scan = scanBalanced(text, 0);
  return scan.balanced && scan.endIndex >= text.trimEnd().length && !scan.unterminatedString;
}
