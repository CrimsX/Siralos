/**
 * Generic UTF-8 byte truncation for bounded language-intelligence text
 * (Stage 3R R5).
 *
 * Truncation never splits a code point. JS strings may contain lone
 * surrogates; like Node's UTF-8 encoder, a lone surrogate occupies 3
 * bytes (it encodes as U+FFFD).
 */

/** UTF-8 byte length of one JavaScript code point. */
function utf8ByteLengthOfCodePoint(codePoint: number): number {
  if (codePoint < 0x80) {
    return 1;
  }
  if (codePoint < 0x800) {
    return 2;
  }
  if (codePoint < 0x10000) {
    // BMP characters, including lone surrogates, encode in 3 bytes.
    return 3;
  }
  return 4;
}

/** UTF-8 byte length of a JavaScript string (code-point based). */
export function utf8ByteLength(text: string): number {
  let bytes = 0;
  for (const character of text) {
    bytes += utf8ByteLengthOfCodePoint(character.codePointAt(0) as number);
  }
  return bytes;
}

/** Truncates UTF-8 text to an exact byte bound without splitting a code point. */
export function truncateUtf8Bytes(text: string, maxBytes: number): string {
  if (utf8ByteLength(text) <= maxBytes) {
    return text;
  }
  let result = "";
  let bytes = 0;
  for (const character of text) {
    const size = utf8ByteLengthOfCodePoint(character.codePointAt(0) as number);
    if (bytes + size > maxBytes) {
      break;
    }
    result += character;
    bytes += size;
  }
  return result;
}
