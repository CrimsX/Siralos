const BINARY_PROBE_BYTES = 8192;

export function looksBinary(buffer: Uint8Array): boolean {
  const probeLength = Math.min(buffer.length, BINARY_PROBE_BYTES);
  for (let index = 0; index < probeLength; index += 1) {
    if (buffer[index] === 0) {
      return true;
    }
  }
  return false;
}

export function decodeUtf8(buffer: Buffer): string | null {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(buffer);
  } catch {
    return null;
  }
}

export function splitIntoLines(text: string): readonly string[] {
  const withoutTrailingNewline = text.endsWith("\n") ? text.slice(0, -1) : text;
  return withoutTrailingNewline
    .split("\n")
    .map((line) => (line.endsWith("\r") ? line.slice(0, -1) : line));
}
