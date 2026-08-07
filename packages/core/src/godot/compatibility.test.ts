import { describe, expect, it } from "vitest";
import {
  assessGodotCompatibility,
  type GodotEngineProfile,
  type GodotProjectProfile,
} from "../index.js";
import { createEmptyGodotCommandCapabilities } from "./capabilities.js";
import { createEmptyGodotProjectProfile } from "./project.js";
import type { GodotVersion } from "./version.js";

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

function engine(overrides: Partial<GodotEngineProfile>): GodotEngineProfile {
  return {
    installationId: "primary",
    fingerprint: "abc",
    version: version({}),
    edition: "standard",
    editionConfidence: "high",
    releaseChannel: "stable",
    capabilities: createEmptyGodotCommandCapabilities(),
    verifiedCapabilities: ["version", "help", "api"],
    degradedCapabilities: [],
    executableSha256: "a".repeat(64),
    apiDumpSha256: null,
    support: "verified",
    diagnostics: [],
    ...overrides,
  };
}

function project(overrides: Partial<GodotProjectProfile>): GodotProjectProfile {
  return {
    ...createEmptyGodotProjectProfile(),
    detected: true,
    declaredEngineVersion: { major: 4, minor: 7, patch: null, raw: "4.7" },
    ...overrides,
  };
}

describe("assessGodotCompatibility", () => {
  it("reports no project for non-Godot workspaces", () => {
    const assessment = assessGodotCompatibility(engine({}), createEmptyGodotProjectProfile());
    expect(assessment.status).toBe("no-project");
    expect(assessment.severity).toBe("info");
  });

  it("reports no engine when none is selected", () => {
    const assessment = assessGodotCompatibility(null, project({}));
    expect(assessment.status).toBe("no-engine");
    expect(assessment.reasons.length).toBeGreaterThan(0);
  });

  it("assesses an exact 4.7 verified engine as compatible", () => {
    const assessment = assessGodotCompatibility(engine({}), project({}));
    expect(assessment.status).toBe("compatible");
    expect(assessment.severity).toBe("info");
  });

  it("assesses a newer patch within the same minor line as compatible", () => {
    const assessment = assessGodotCompatibility(
      engine({ version: version({ raw: "4.7.2.stable.official", patch: 2 }) }),
      project({}),
    );
    expect(assessment.status).toBe("compatible");
  });

  it("warns when the engine minor is older than the declared minor", () => {
    const assessment = assessGodotCompatibility(
      engine({ version: version({ raw: "4.6.3.stable.official", minor: 6, patch: 3 }) }),
      project({}),
    );
    expect(assessment.status).toBe("engine-older-than-project");
    expect(assessment.severity).toBe("error");
  });

  it("errors on a major version mismatch", () => {
    const assessment = assessGodotCompatibility(
      engine({ version: version({ raw: "3.6.1.stable.official", major: 3, minor: 6 }) }),
      project({}),
    );
    expect(assessment.status).toBe("major-version-mismatch");
    expect(assessment.severity).toBe("error");
  });

  it("warns on migration-sensitive newer minor engines", () => {
    const assessment = assessGodotCompatibility(
      engine({ version: version({ raw: "4.8.1.stable.official", minor: 8 }) }),
      project({}),
    );
    expect(assessment.status).toBe("likely-compatible");
    expect(assessment.severity).toBe("warning");
  });

  it("reports an edition mismatch for .NET projects with a standard engine", () => {
    const assessment = assessGodotCompatibility(engine({}), project({ languageProfile: "dotnet" }));
    expect(assessment.status).toBe("edition-mismatch");
    expect(assessment.severity).toBe("error");
  });

  it("warns for GDScript projects with a .NET engine", () => {
    const assessment = assessGodotCompatibility(
      engine({ edition: "dotnet", support: "compatible-untested" }),
      project({ languageProfile: "gdscript" }),
    );
    expect(assessment.status).toBe("likely-compatible");
    expect(assessment.severity).toBe("warning");
  });

  it("warns for unverified custom engines", () => {
    const assessment = assessGodotCompatibility(
      engine({ support: "custom-build-untested" }),
      project({}),
    );
    expect(assessment.status).toBe("engine-unverified");
  });

  it("reports project-version-unknown when the project declares no version", () => {
    const assessment = assessGodotCompatibility(
      engine({}),
      project({ declaredEngineVersion: null }),
    );
    expect(assessment.status).toBe("project-version-unknown");
    expect(assessment.severity).toBe("warning");
  });

  it("always explains itself", () => {
    const assessments = [
      assessGodotCompatibility(engine({}), createEmptyGodotProjectProfile()),
      assessGodotCompatibility(null, project({})),
      assessGodotCompatibility(engine({}), project({})),
    ];
    for (const assessment of assessments) {
      expect(assessment.reasons.length).toBeGreaterThan(0);
    }
  });

  it("never claims guaranteed compatibility", () => {
    const assessment = assessGodotCompatibility(engine({}), project({}));
    for (const reason of assessment.reasons) {
      expect(reason).not.toMatch(/guaranteed/i);
    }
  });
});
