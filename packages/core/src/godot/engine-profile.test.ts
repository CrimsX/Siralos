import { describe, expect, it } from "vitest";
import {
  classifyGodotEdition,
  classifyGodotSupport,
  type GodotEditionEvidence,
} from "./engine-profile.js";
import type { GodotVersion } from "./version.js";
import {
  createEmptyGodotCommandCapabilities,
  type GodotCommandCapabilities,
} from "./capabilities.js";

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

function capabilities(overrides: Partial<GodotCommandCapabilities>): GodotCommandCapabilities {
  return { ...createEmptyGodotCommandCapabilities(), ...overrides };
}

function evidence(overrides: Partial<GodotEditionEvidence>): GodotEditionEvidence {
  return {
    explicitHint: null,
    filename: "Godot_v4.7.1-stable_win64.exe",
    capabilities: capabilities({}),
    apiConfigurationFeatures: [],
    probesSucceeded: { version: true, help: true, apiDump: false },
    ...overrides,
  };
}

function editorCapabilities(): GodotCommandCapabilities {
  return capabilities({ editor: true, projectManager: true, extensionApiDump: true });
}

describe("classifyGodotEdition", () => {
  it("classifies standard evidence with an explicit standard hint", () => {
    const result = classifyGodotEdition(
      evidence({ explicitHint: "standard", capabilities: editorCapabilities() }),
    );
    expect(result.edition).toBe("standard");
    expect(result.confidence).toBe("medium");
  });

  it("classifies standard evidence with probe corroboration", () => {
    const result = classifyGodotEdition(
      evidence({
        explicitHint: "standard",
        capabilities: editorCapabilities(),
        probesSucceeded: { version: true, help: true, apiDump: true },
      }),
    );
    expect(result.edition).toBe("standard");
    expect(result.confidence).toBe("high");
  });

  it("classifies .NET evidence cautiously from a single filename marker", () => {
    const result = classifyGodotEdition(
      evidence({ explicitHint: null, filename: "Godot_v4.7.1-stable_mono_win64.exe" }),
    );
    expect(result.edition).toBe("dotnet");
    expect(result.confidence).toBe("medium");
  });

  it("classifies .NET evidence confidently with corroborating signals", () => {
    const result = classifyGodotEdition(
      evidence({
        explicitHint: "dotnet",
        filename: "Godot_v4.7.1-stable_mono_win64.exe",
        capabilities: capabilities({ buildSolutions: true }),
      }),
    );
    expect(result.edition).toBe("dotnet");
    expect(result.confidence).toBe("high");
  });

  it("reports conflicting evidence and lowers confidence", () => {
    const result = classifyGodotEdition(
      evidence({
        explicitHint: "standard",
        filename: "Godot_v4.7.1-stable_mono_win64.exe",
      }),
    );
    expect(result.edition).toBe("dotnet");
    expect(result.confidence).toBe("medium");
    expect(result.conflicts.length).toBeGreaterThan(0);
  });

  it("never claims standard solely because a filename lacks mono", () => {
    const result = classifyGodotEdition(
      evidence({
        explicitHint: null,
        filename: "godot-win64.exe",
        capabilities: editorCapabilities(),
      }),
    );
    expect(result.edition).toBe("editor-unknown");
  });

  it("classifies runtime-only binaries by missing editor signals", () => {
    const result = classifyGodotEdition(evidence({ capabilities: capabilities({}) }));
    expect(result.edition).toBe("runtime-only");
  });

  it("keeps the edition unknown when the version probe failed", () => {
    const result = classifyGodotEdition(
      evidence({ probesSucceeded: { version: false, help: false, apiDump: false } }),
    );
    expect(result.edition).toBe("unknown");
  });

  it("keeps the edition unknown when the help probe failed", () => {
    const result = classifyGodotEdition(
      evidence({ probesSucceeded: { version: true, help: false, apiDump: false } }),
    );
    expect(result.edition).toBe("unknown");
  });

  it("treats the API dump feature list as .NET evidence", () => {
    const result = classifyGodotEdition(evidence({ apiConfigurationFeatures: ["dotnet"] }));
    expect(result.edition).toBe("dotnet");
  });
});

describe("classifyGodotSupport", () => {
  it("marks the exact 4.7.1 stable standard baseline as verified", () => {
    const support = classifyGodotSupport({
      version: version({}),
      edition: "standard",
      editionConfidence: "high",
      isVerifiedBaseline: true,
    });
    expect(support).toBe("verified");
  });

  it("marks other 4.7 stable standard editors as compatible-untested", () => {
    const support = classifyGodotSupport({
      version: version({ raw: "4.7.2.stable.official", patch: 2 }),
      edition: "standard",
      editionConfidence: "high",
      isVerifiedBaseline: false,
    });
    expect(support).toBe("compatible-untested");
  });

  it("marks 4.7 prereleases as prerelease-untested", () => {
    const support = classifyGodotSupport({
      version: version({ raw: "4.7.2.rc1.official", patch: 2, status: "rc", statusNumber: 1 }),
      edition: "standard",
      editionConfidence: "high",
      isVerifiedBaseline: false,
    });
    expect(support).toBe("prerelease-untested");
  });

  it("marks 4.8 development builds as prerelease-untested", () => {
    const support = classifyGodotSupport({
      version: version({
        raw: "4.8.dev2.custom_build",
        major: 4,
        minor: 8,
        patch: null,
        status: "dev",
        statusNumber: 2,
        build: "custom_build",
      }),
      edition: "standard",
      editionConfidence: "medium",
      isVerifiedBaseline: false,
    });
    expect(support).toBe("prerelease-untested");
  });

  it("marks Godot 3 as unsupported-major", () => {
    const support = classifyGodotSupport({
      version: version({ raw: "3.6.1.stable.official", major: 3, minor: 6, patch: 1 }),
      edition: "standard",
      editionConfidence: "high",
      isVerifiedBaseline: false,
    });
    expect(support).toBe("unsupported-major");
  });

  it("marks .NET editions as compatible-untested", () => {
    const support = classifyGodotSupport({
      version: version({}),
      edition: "dotnet",
      editionConfidence: "high",
      isVerifiedBaseline: false,
    });
    expect(support).toBe("compatible-untested");
  });

  it("marks custom builds as custom-build-untested", () => {
    const support = classifyGodotSupport({
      version: version({ status: "custom", build: "custom_build", raw: "4.7.1.custom_build" }),
      edition: "standard",
      editionConfidence: "medium",
      isVerifiedBaseline: false,
    });
    expect(support).toBe("custom-build-untested");
  });

  it("marks runtime-only editions as runtime-only", () => {
    const support = classifyGodotSupport({
      version: version({}),
      edition: "runtime-only",
      editionConfidence: "medium",
      isVerifiedBaseline: false,
    });
    expect(support).toBe("runtime-only");
  });
});
