import { describe, expect, it } from "vitest";
import { toOneBasedPosition, toOneBasedRange } from "./position.js";
import { sanitizeControlCharacters } from "./sanitize.js";
import { truncateUtf8Bytes, utf8ByteLength } from "./truncate.js";
import {
  mapLspSeverity,
  normalizeDiagnosticPayload,
  normalizeDiagnosticSet,
  type LanguageDiagnostic,
} from "./diagnostic.js";
import { normalizeDefinitionLocations } from "./definition.js";

/**
 * Focused reference-semantics tests for the generic language-
 * intelligence modules (Stage 3R R5). These prove the production
 * TypeScript reference behavior that the differential oracle probes
 * exercise; the Godot adapters consume the same functions.
 */

describe("toOneBasedPosition / toOneBasedRange", () => {
  it("converts 0-based LSP positions to the 1-based Siralos convention", () => {
    expect(toOneBasedPosition({ line: 32, character: 15 })).toEqual({ line: 33, column: 16 });
    expect(toOneBasedPosition({ line: 0, character: 0 })).toEqual({ line: 1, column: 1 });
  });

  it("rejects malformed positions without fabricating values", () => {
    expect(toOneBasedPosition(null)).toBeNull();
    expect(toOneBasedPosition({ line: -1, character: 0 })).toBeNull();
    expect(toOneBasedPosition({ line: 1.5, character: 0 })).toBeNull();
    expect(toOneBasedPosition({ line: "1", character: 0 })).toBeNull();
    expect(toOneBasedRange({ start: { line: 1, character: 0 } })).toBeNull();
    expect(toOneBasedRange({ start: { line: -1, character: 0 }, end: { line: 1, character: 1 } })).toBeNull();
  });
});

describe("sanitizeControlCharacters", () => {
  it("strips CSI sequences and replaces remaining controls", () => {
    expect(sanitizeControlCharacters("a\u001b[31mb\u001b[0mc")).toBe("abc");
    expect(sanitizeControlCharacters("a\u0000b\u007f\u009bc")).toBe("a\uFFFDb\uFFFD\uFFFDc");
  });

  it("preserves tab, newline, carriage return, and unicode", () => {
    expect(sanitizeControlCharacters("a\tb\nc\rd")).toBe("a\tb\nc\rd");
    expect(sanitizeControlCharacters("caf\u00e9 \u{1f600}")).toBe("caf\u00e9 \u{1f600}");
  });
});

describe("truncateUtf8Bytes / utf8ByteLength", () => {
  it("truncates to an exact byte bound without splitting a code point", () => {
    expect(truncateUtf8Bytes("h\u00e9llo", 4)).toBe("h\u00e9l");
    expect(utf8ByteLength(truncateUtf8Bytes("\u{1f600}\u{1f600}\u{1f600}", 5))).toBe(4);
    expect(truncateUtf8Bytes("short", 1024)).toBe("short");
    expect(truncateUtf8Bytes("any", 0)).toBe("");
  });
});

describe("mapLspSeverity", () => {
  it("maps the closed LSP vocabulary and preserves unknowns", () => {
    expect(mapLspSeverity(1)).toBe("error");
    expect(mapLspSeverity(2)).toBe("warning");
    expect(mapLspSeverity(3)).toBe("info");
    expect(mapLspSeverity(4)).toBe("info");
    expect(mapLspSeverity(9)).toBe("unknown");
    expect(mapLspSeverity(undefined)).toBe("unknown");
  });
});

describe("normalizeDiagnosticPayload", () => {
  it("normalizes positions, severities, codes, and messages with bounds", () => {
    const payload = normalizeDiagnosticPayload(
      [
        {
          range: { start: { line: 33, character: 16 }, end: { line: 33, character: 27 } },
          severity: 1,
          code: "undeclared_identifier",
          message: 'Identifier "velocityy" not declared.',
          source: "gdscript",
        },
        { severity: 2, message: "no range" },
        "not an object",
        { severity: 1, message: "   " },
      ],
      "langsvc",
      "scripts/player.gd",
      null,
      { maxDiagnostics: 100, maxMessageBytes: 8192 },
    );
    expect(payload?.path).toBe("scripts/player.gd");
    expect(payload?.diagnostics).toHaveLength(2);
    expect(payload?.diagnostics[0]).toMatchObject({
      source: "langsvc",
      severity: "error",
      line: 34,
      column: 17,
      code: "undeclared_identifier",
      rawCategory: "gdscript",
    });
    // Missing locations are never fabricated.
    expect(payload?.diagnostics[1].line).toBeNull();
    expect(payload?.diagnostics[1].column).toBeNull();
    expect(payload?.truncated).toBe(false);
  });

  it("returns null for non-array payloads", () => {
    expect(
      normalizeDiagnosticPayload("nope", "langsvc", "a.gd", null, {
        maxDiagnostics: 100,
        maxMessageBytes: 8192,
      }),
    ).toBeNull();
  });
});

describe("normalizeDiagnosticSet", () => {
  it("collapses duplicates and sorts deterministically by path, line, column, message", () => {
    const make = (partial: Partial<LanguageDiagnostic>): LanguageDiagnostic => ({
      source: "langsvc",
      severity: "error",
      path: "scripts/player.gd",
      line: 1,
      column: 1,
      code: null,
      message: "m",
      rawCategory: null,
      ...partial,
    });
    const result = normalizeDiagnosticSet(
      [
        make({ message: "z" }),
        make({ message: "a" }),
        make({ message: "a" }), // exact duplicate
        make({ path: null, message: "orphan" }),
        make({ line: 9 }),
      ],
      100,
    );
    expect(result.truncated).toBe(false);
    expect(result.diagnostics.map((entry) => entry.message)).toEqual([
      "orphan",
      "a",
      "z",
      "m",
    ]);
  });

  it("applies the bound with explicit truncation", () => {
    const many = Array.from({ length: 12 }, (_, index) => ({
      source: "langsvc",
      severity: "error" as const,
      path: "a.gd",
      line: index + 1,
      column: 1,
      code: null,
      message: "m",
      rawCategory: null,
    }));
    const result = normalizeDiagnosticSet(many, 10);
    expect(result.diagnostics).toHaveLength(10);
    expect(result.truncated).toBe(true);
  });
});

describe("normalizeDefinitionLocations", () => {
  it("normalizes LocationLink and Location forms with external redaction", () => {
    const mapUri = (uri: string) => (uri.startsWith("file:///work/") ? uri.slice("file:///work/".length) : null);
    const result = normalizeDefinitionLocations(
      [
        {
          targetUri: "file:///work/scripts/player.gd",
          targetRange: { start: { line: 10, character: 4 }, end: { line: 10, character: 12 } },
        },
        { uri: "file:///elsewhere/engine/core.gd", range: { start: { line: 1, character: 0 }, end: { line: 1, character: 1 } } },
        { targetUri: "file:///work/scripts/x.gd" }, // missing range: skipped
      ],
      "scripts/player.gd",
      mapUri,
      { maxLocations: 100 },
    );
    expect(result.path).toBe("scripts/player.gd");
    expect(result.locations).toHaveLength(2);
    expect(result.locations[0]).toEqual({
      path: "scripts/player.gd",
      range: { start: { line: 11, column: 5 }, end: { line: 11, column: 13 } },
      external: false,
    });
    // Out-of-workspace targets are conservative basenames only.
    expect(result.locations[1]).toEqual({
      path: "core.gd",
      range: { start: { line: 2, column: 1 }, end: { line: 2, column: 2 } },
      external: true,
    });
    expect(result.truncated).toBe(false);
  });

  it("bounds the result with explicit truncation", () => {
    const locations = Array.from({ length: 5 }, (_, index) => ({
      uri: `file:///work/scripts/f${index}.gd`,
      range: { start: { line: index, character: 0 }, end: { line: index, character: 1 } },
    }));
    const result = normalizeDefinitionLocations(
      locations,
      "q.gd",
      (uri) => uri.slice("file:///work/".length),
      { maxLocations: 3 },
    );
    expect(result.locations).toHaveLength(3);
    expect(result.truncated).toBe(true);
  });
});
