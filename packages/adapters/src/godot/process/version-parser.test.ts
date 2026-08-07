import { describe, expect, it } from "vitest";
import { parseGodotVersionText, sanitizeControlCharacters } from "./version-parser.js";

describe("parseGodotVersionText", () => {
  it("parses a stable release", () => {
    const result = parseGodotVersionText("4.7.1.stable.official\n");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.version).toMatchObject({
        major: 4,
        minor: 7,
        patch: 1,
        status: "stable",
        statusNumber: null,
        build: "official",
      });
    }
  });

  it("parses a patchless version conservatively", () => {
    const result = parseGodotVersionText("4.7.stable.official");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.version.patch).toBeNull();
      expect(result.version.minor).toBe(7);
    }
  });

  it("parses a release candidate with a sequence number", () => {
    const result = parseGodotVersionText("4.7.2.rc1.official");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.version.status).toBe("rc");
      expect(result.version.statusNumber).toBe(1);
    }
  });

  it("parses a beta version", () => {
    const result = parseGodotVersionText("4.7.2.beta3.official");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.version.status).toBe("beta");
      expect(result.version.statusNumber).toBe(3);
    }
  });

  it("parses a dev build with a sequence number", () => {
    const result = parseGodotVersionText("4.8.dev2.custom_build");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.version.status).toBe("dev");
      expect(result.version.statusNumber).toBe(2);
      expect(result.version.build).toBe("custom_build");
    }
  });

  it("parses a custom build", () => {
    const result = parseGodotVersionText("4.7.1.custom_build");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.version.status).toBe("custom");
      expect(result.version.build).toBe("custom_build");
    }
  });

  it("parses a version with a git commit token", () => {
    const result = parseGodotVersionText("4.2.2.stable.official.15073afe3");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.version.build).toBe("official");
      expect(result.version.commit).toBe("15073afe3");
    }
  });

  it("preserves unknown suffixes without failing", () => {
    const result = parseGodotVersionText("4.7.1.stable.weird_suffix");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.version.build).toBe("weird_suffix");
      expect(result.version.status).toBe("stable");
    }
  });

  it("detects Godot 3 as parsed with a low major", () => {
    const result = parseGodotVersionText("3.6.1.stable.official");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.version.major).toBe(3);
    }
  });

  it("rejects empty output", () => {
    const result = parseGodotVersionText("");
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.message).toMatch(/empty/);
    }
  });

  it("rejects non-Godot output", () => {
    const result = parseGodotVersionText("hello world");
    expect(result.ok).toBe(false);
  });

  it("rejects non-numeric major", () => {
    const result = parseGodotVersionText("four.7.1.stable.official");
    expect(result.ok).toBe(false);
  });

  it("rejects non-numeric minor", () => {
    const result = parseGodotVersionText("4.seven.1.stable.official");
    expect(result.ok).toBe(false);
  });

  it("accepts a leading Godot prefix leniently", () => {
    const result = parseGodotVersionText("Godot Engine v4.7.1.stable.official");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.version.major).toBe(4);
    }
  });

  it("takes the first line only", () => {
    const result = parseGodotVersionText("4.7.1.stable.official\nGodot Engine 4.7.1");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.version.raw).toBe("4.7.1.stable.official");
    }
  });
});

describe("sanitizeControlCharacters", () => {
  it("replaces control characters with U+FFFD", () => {
    expect(sanitizeControlCharacters("4.7.1\x1b[31m.stable.official")).toBe(
      "4.7.1\uFFFD[31m.stable.official",
    );
  });

  it("preserves line breaks and tabs", () => {
    expect(sanitizeControlCharacters("4.7.1.stable.official\n\ttail")).toBe(
      "4.7.1.stable.official\n\ttail",
    );
  });
});
