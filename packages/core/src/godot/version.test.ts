import { describe, expect, it } from "vitest";
import { classifyGodotReleaseChannel, parseDeclaredVersion, type GodotVersion } from "./version.js";

function version(overrides: Partial<GodotVersion>): GodotVersion {
  return {
    raw: "4.7.1.stable.official",
    major: 4,
    minor: 7,
    patch: 1,
    status: "stable",
    statusNumber: null,
    build: "official",
    commit: null,
    ...overrides,
  };
}

describe("classifyGodotReleaseChannel", () => {
  it("classifies stable versions", () => {
    expect(classifyGodotReleaseChannel(version({ status: "stable" }))).toBe("stable");
  });

  it("classifies release candidates", () => {
    expect(classifyGodotReleaseChannel(version({ status: "rc", statusNumber: 1 }))).toBe(
      "release-candidate",
    );
  });

  it("classifies beta and alpha versions", () => {
    expect(classifyGodotReleaseChannel(version({ status: "beta" }))).toBe("beta");
    expect(classifyGodotReleaseChannel(version({ status: "alpha" }))).toBe("alpha");
  });

  it("classifies development and custom builds", () => {
    expect(classifyGodotReleaseChannel(version({ status: "dev", statusNumber: 2 }))).toBe(
      "development",
    );
    expect(classifyGodotReleaseChannel(version({ status: "custom" }))).toBe("custom");
  });

  it("classifies unknown statuses conservatively", () => {
    expect(classifyGodotReleaseChannel(version({ status: "unknown" }))).toBe("unknown");
  });
});

describe("parseDeclaredVersion", () => {
  it("parses major.minor feature tokens", () => {
    expect(parseDeclaredVersion("4.7")).toEqual({ major: 4, minor: 7, patch: null, raw: "4.7" });
  });

  it("parses major.minor.patch tokens", () => {
    expect(parseDeclaredVersion("4.7.1")).toEqual({
      major: 4,
      minor: 7,
      patch: 1,
      raw: "4.7.1",
    });
  });

  it("rejects non-numeric tokens", () => {
    expect(parseDeclaredVersion("four.seven")).toBeNull();
  });

  it("rejects partial tokens", () => {
    expect(parseDeclaredVersion("4")).toBeNull();
    expect(parseDeclaredVersion("4.")).toBeNull();
  });

  it("trims surrounding whitespace", () => {
    expect(parseDeclaredVersion(" 4.7 ")).toEqual({ major: 4, minor: 7, patch: null, raw: "4.7" });
  });
});
