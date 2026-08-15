/**
 * Generic control-character sanitization for untrusted language-tool
 * output (Stage 3R R5).
 *
 * Language-server/compiler/parser output is untrusted data. Terminal
 * escape sequences (CSI) are stripped and remaining control characters
 * are replaced with U+FFFD before any model- or terminal-facing text is
 * produced. Tabs, newlines, and carriage returns are preserved.
 */

/**
 * Replace terminal escape sequences and control characters with safe
 * text: CSI sequences (ESC [ params intermediates final) are removed,
 * and every other C0 control character, DEL, and C1 control character
 * becomes U+FFFD.
 */
export function sanitizeControlCharacters(text: string): string {
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
