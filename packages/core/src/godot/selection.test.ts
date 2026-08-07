import { describe, expect, it } from "vitest";
import {
  rankCandidate,
  rankGodotCandidates,
  type GodotEngineProfile,
  type GodotInstallation,
} from "../index.js";
import { createEmptyGodotCommandCapabilities } from "./capabilities.js";
import type { GodotVersion } from "./version.js";

function installation(overrides: Partial<GodotInstallation>): GodotInstallation {
  return {
    id: "primary",
    sourceLabel: "user config",
    source: "user-config",
    canonicalPath: "C:\\Tools\\Godot.exe",
    sizeBytes: 100,
    modifiedAtMs: 0,
    sha256: "a".repeat(64),
    editionHint: "unknown",
    status: "valid",
    ...overrides,
  };
}

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

function profile(overrides: Partial<GodotEngineProfile>): GodotEngineProfile {
  return {
    installationId: "primary",
    fingerprint: "abc123",
    version: version({}),
    edition: "standard",
    editionConfidence: "high",
    releaseChannel: "stable",
    capabilities: createEmptyGodotCommandCapabilities(),
    verifiedCapabilities: ["version", "help"],
    degradedCapabilities: [],
    executableSha256: "a".repeat(64),
    apiDumpSha256: null,
    support: "compatible-untested",
    diagnostics: [],
    ...overrides,
  };
}

describe("rankCandidate", () => {
  it("ranks the verified baseline highest", () => {
    const rank = rankCandidate({
      installation: installation({}),
      profile: profile({ support: "verified" }),
    });
    expect(rank).toBe(6);
  });

  it("ranks compatible stable standard editors below the baseline", () => {
    const rank = rankCandidate({
      installation: installation({}),
      profile: profile({ support: "compatible-untested" }),
    });
    expect(rank).toBe(7);
  });

  it("ranks compatible stable .NET editors below standard", () => {
    const rank = rankCandidate({
      installation: installation({}),
      profile: profile({ support: "compatible-untested", edition: "dotnet" }),
    });
    expect(rank).toBe(8);
  });

  it("ranks prerelease editors below stable candidates", () => {
    const rank = rankCandidate({
      installation: installation({}),
      profile: profile({ support: "prerelease-untested" }),
    });
    expect(rank).toBe(9);
  });

  it("never ranks runtime-only binaries", () => {
    const rank = rankCandidate({
      installation: installation({}),
      profile: profile({ support: "runtime-only" }),
    });
    expect(rank).toBeNull();
  });

  it("never ranks unsupported Godot 3", () => {
    const rank = rankCandidate({
      installation: installation({}),
      profile: profile({ support: "unsupported-major" }),
    });
    expect(rank).toBeNull();
  });

  it("never ranks invalid profiles", () => {
    const rank = rankCandidate({
      installation: installation({}),
      profile: profile({ support: "invalid" }),
    });
    expect(rank).toBeNull();
  });
});

describe("rankGodotCandidates", () => {
  it("selects the verified baseline over a compatible stable editor", () => {
    const ranked = rankGodotCandidates([
      {
        installation: installation({ id: "other", canonicalPath: "C:\\b.exe" }),
        profile: profile({ installationId: "other", support: "compatible-untested" }),
      },
      {
        installation: installation({ id: "primary", canonicalPath: "C:\\a.exe" }),
        profile: profile({ installationId: "primary", support: "verified" }),
      },
    ]);
    expect(ranked[0]?.installation.id).toBe("primary");
  });

  it("prefers stable over prerelease within the same rank", () => {
    const ranked = rankGodotCandidates([
      {
        installation: installation({ id: "rc", canonicalPath: "C:\\b.exe" }),
        profile: profile({
          installationId: "rc",
          support: "prerelease-untested",
          releaseChannel: "release-candidate",
          version: version({ raw: "4.7.1.rc1.official", status: "rc" }),
        }),
      },
      {
        installation: installation({ id: "stable", canonicalPath: "C:\\a.exe" }),
        profile: profile({
          installationId: "stable",
          support: "prerelease-untested",
          releaseChannel: "stable",
        }),
      },
    ]);
    expect(ranked[0]?.installation.id).toBe("stable");
  });

  it("prefers standard over .NET for the GDScript-first roadmap", () => {
    const ranked = rankGodotCandidates([
      {
        installation: installation({ id: "dotnet", canonicalPath: "C:\\b.exe" }),
        profile: profile({ installationId: "dotnet", edition: "dotnet" }),
      },
      {
        installation: installation({ id: "standard", canonicalPath: "C:\\a.exe" }),
        profile: profile({ installationId: "standard" }),
      },
    ]);
    expect(ranked[0]?.installation.id).toBe("standard");
  });

  it("prefers a higher patch version within the same rank", () => {
    const ranked = rankGodotCandidates([
      {
        installation: installation({ id: "lower", canonicalPath: "C:\\b.exe" }),
        profile: profile({
          installationId: "lower",
          version: version({ raw: "4.7.0.stable.official", patch: 0 }),
        }),
      },
      {
        installation: installation({ id: "higher", canonicalPath: "C:\\a.exe" }),
        profile: profile({
          installationId: "higher",
          version: version({ raw: "4.7.2.stable.official", patch: 2 }),
        }),
      },
    ]);
    expect(ranked[0]?.installation.id).toBe("higher");
  });

  it("uses deterministic path ordering as the final tie-breaker", () => {
    const ranked = rankGodotCandidates([
      {
        installation: installation({ id: "b", canonicalPath: "C:\\zeta.exe" }),
        profile: profile({ installationId: "b" }),
      },
      {
        installation: installation({ id: "a", canonicalPath: "C:\\alpha.exe" }),
        profile: profile({ installationId: "a" }),
      },
    ]);
    expect(ranked[0]?.installation.id).toBe("a");
  });

  it("produces no selection when no candidate is selectable", () => {
    const ranked = rankGodotCandidates([
      {
        installation: installation({}),
        profile: profile({ support: "runtime-only" }),
      },
    ]);
    expect(ranked[0]?.rank).toBeNull();
  });
});
