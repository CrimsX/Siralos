/**
 * One final terminal-rendering boundary. Provider responses, repository
 * filenames, Git output, checkpoint metadata, tool activity, errors, and
 * approval information are all untrusted; every byte that reaches the
 * terminal passes through this sanitizer, which neutralizes C0/C1 controls,
 * ANSI CSI sequences, OSC sequences (including OSC 8 links, title changes,
 * and clipboard writes), carriage-return and backspace rewriting, and DEL.
 * Ordinary Unicode and readable newlines survive. Sequences split across
 * stream chunks are tracked across `push` calls; `flush` drops any dangling
 * sequence so truncation can never leave the terminal inside an active
 * escape sequence.
 */
export class TerminalSanitizer {
  private mode: "normal" | "escape" | "csi" | "osc" | "osc_escape" = "normal";
  /**
   * A high surrogate held back because its low surrogate may arrive in the
   * next chunk. Node encodes each `write` call separately, so a pair split
   * across chunks would otherwise be corrupted into replacement characters;
   * pairing across pushes keeps emoji and other non-BMP text intact.
   */
  private pendingHighSurrogate: string | null = null;

  push(text: string): string {
    let out = "";
    for (const character of text) {
      const code = character.codePointAt(0) ?? 0;
      if (this.pendingHighSurrogate !== null) {
        if (code >= 0xdc00 && code <= 0xdfff) {
          out += this.pendingHighSurrogate + character;
          this.pendingHighSurrogate = null;
          continue;
        }
        out += "\uFFFD";
        this.pendingHighSurrogate = null;
      }
      if (this.mode === "normal" && code >= 0xd800 && code <= 0xdbff) {
        this.pendingHighSurrogate = character;
        continue;
      }
      if (code >= 0xdc00 && code <= 0xdfff) {
        // A lone low surrogate is never valid UTF-16; render it visibly.
        out += "\uFFFD";
        continue;
      }
      switch (this.mode) {
        case "normal": {
          if (character === "\u001b") {
            this.mode = "escape";
          } else if (character === "\n" || character === "\t") {
            out += character;
          } else if (code <= 0x1f) {
            out += caretNotation(code);
          } else if (code === 0x7f) {
            out += "^?";
          } else if (code >= 0x80 && code <= 0x9f) {
            out += "\uFFFD";
          } else {
            out += character;
          }
          break;
        }
        case "escape":
          if (character === "[") {
            this.mode = "csi";
          } else if (character === "]") {
            this.mode = "osc";
          } else {
            this.mode = "normal";
          }
          break;
        case "csi":
          if (character >= "\x40" && character <= "\x7e") {
            this.mode = "normal";
          }
          break;
        case "osc":
          if (character === "\u0007") {
            this.mode = "normal";
          } else if (character === "\u001b") {
            this.mode = "osc_escape";
          }
          break;
        case "osc_escape":
          this.mode = character === "\\" ? "normal" : "osc";
          break;
      }
    }
    return out;
  }

  flush(): string {
    this.mode = "normal";
    const dangling = this.pendingHighSurrogate;
    this.pendingHighSurrogate = null;
    return dangling === null ? "" : "\uFFFD";
  }
}

function caretNotation(code: number): string {
  return `^${String.fromCharCode(code + 0x40)}`;
}

export function sanitizeForDisplay(text: string): string {
  const sanitizer = new TerminalSanitizer();
  return sanitizer.push(text) + sanitizer.flush();
}

/**
 * Renders a path-like single-line field safely. Paths are untrusted: a file
 * or checkpoint path may contain embedded newlines, tabs, carriage returns,
 * or other control characters that would otherwise spoof approval prompts,
 * status lines, or undo output by fabricating additional lines. The
 * sanitizer boundary still applies afterwards; this makes the spoofing
 * vector itself visible instead of structural.
 */
export function sanitizePathForDisplay(path: string | null): string {
  if (path === null) {
    return "(none)";
  }
  let out = "";
  for (const character of path) {
    const code = character.codePointAt(0) ?? 0;
    if (character === "\\") {
      out += "\\\\";
    } else if (character === "\n") {
      out += "\\n";
    } else if (character === "\r") {
      out += "\\r";
    } else if (character === "\t") {
      out += "\\t";
    } else if (code < 0x20) {
      out += caretNotation(code);
    } else if (code === 0x7f) {
      out += "^?";
    } else {
      out += character;
    }
  }
  return out;
}

export function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "An unexpected error occurred.";
}
