/**
 * Conservative lexical helpers for untrusted GDScript-like text.
 *
 * Only line structure is analyzed: comments (`#` to end of line) and
 * quoted strings (single and double quotes, with backslash escapes and
 * triple-quoted spans) are masked, so tokens such as `@tool` are only
 * detected when they appear as real code. Nothing is parsed or executed.
 */
export function maskCommentsAndStrings(text: string): string {
  const output = new Array<string>(text.length);
  let state: "normal" | "double" | "single" | "comment" = "normal";
  let index = 0;
  while (index < text.length) {
    const character = text[index] as string;
    const next = text[index + 1];
    if (state === "comment") {
      output[index] = " ";
      if (character === "\n") {
        state = "normal";
      }
      index += 1;
      continue;
    }
    if (state === "double" || state === "single") {
      const quote = state === "double" ? '"' : "'";
      output[index] = " ";
      if (character === "\\") {
        if (index + 1 < text.length) {
          output[index + 1] = " ";
          index += 2;
          continue;
        }
        index += 1;
        continue;
      }
      if (character === quote) {
        if (next === quote && text[index + 2] === quote) {
          output[index] = " ";
          output[index + 1] = " ";
          output[index + 2] = " ";
          state = "normal";
          index += 3;
          continue;
        }
        state = "normal";
      }
      index += 1;
      continue;
    }
    if (character === "#") {
      state = "comment";
      output[index] = " ";
      index += 1;
      continue;
    }
    if (character === '"' || character === "'") {
      state = character === '"' ? "double" : "single";
      output[index] = " ";
      if (next === character && text[index + 2] === character) {
        output[index + 1] = " ";
        output[index + 2] = " ";
        index += 3;
        continue;
      }
      index += 1;
      continue;
    }
    output[index] = character;
    index += 1;
  }
  return output.join("");
}

/** True when the exact token appears in code (not comments or strings). */
export function containsCodeToken(text: string, token: string): boolean {
  const masked = maskCommentsAndStrings(text);
  const pattern = new RegExp(`(?:^|[^\\w])${escapeRegExp(token)}(?!\\w)`);
  return pattern.test(masked);
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
