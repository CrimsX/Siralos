import { describe, expect, it } from "vitest";
import type {
  GodotCompatibilityAssessment,
  GodotDiscoveryResult,
  GodotDoctorReport,
  GodotInspector,
  GodotProjectProfile,
  GodotSelectedInstallation,
  GodotVersion,
} from "@solaris/core";
import { createGodotInspectEngineTool } from "./godot-inspect-engine-tool.js";
import { createGodotInspectProjectTool } from "./godot-inspect-project-tool.js";
import { createEmptyGodotProjectProfile } from "@solaris/core";
import { createEmptyGodotCommandCapabilities } from "@solaris/core";

function version(overrides: Partial<GodotVersion> = {}): GodotVersion {
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

function selected(): GodotSelectedInstallation {
  return {
    installationId: "primary",
    sourceLabel: "user config",
    source: "user-config",
    version: version(),
    edition: "standard",
    editionConfidence: "high",
    releaseChannel: "stable",
    support: "verified",
    capabilities: createEmptyGodotCommandCapabilities(),
    verifiedCapabilities: ["version", "help", "extension-api-dump"],
    degradedCapabilities: [],
    executableFingerprint: "abcdef123456",
    apiDumpSha256: null,
    diagnostics: [],
  };
}

function project(overrides: Partial<GodotProjectProfile> = {}): GodotProjectProfile {
  return {
    ...createEmptyGodotProjectProfile(),
    detected: true,
    name: "Test Game",
    declaredEngineVersion: { major: 4, minor: 7, patch: null, raw: "4.7" },
    ...overrides,
  };
}

function assessment(): GodotCompatibilityAssessment {
  return {
    status: "compatible",
    severity: "info",
    reasons: ["The engine matches the declared project version."],
  };
}

function stubInspector(options: {
  readonly selectedResult?: GodotSelectedInstallation | null;
  readonly projectResult?: GodotProjectProfile;
  readonly assessmentResult?: GodotCompatibilityAssessment;
}): GodotInspector {
  return {
    discover(): Promise<GodotDiscoveryResult> {
      return Promise.reject(new Error("not used"));
    },
    selected(): Promise<GodotSelectedInstallation | null> {
      return Promise.resolve(options.selectedResult ?? null);
    },
    projectProfile(): Promise<GodotProjectProfile> {
      return Promise.resolve(options.projectResult ?? createEmptyGodotProjectProfile());
    },
    compatibility(): Promise<GodotCompatibilityAssessment> {
      return Promise.resolve(options.assessmentResult ?? assessment());
    },
    doctor(): Promise<GodotDoctorReport> {
      return Promise.reject(new Error("not used"));
    },
  };
}

describe("createGodotInspectEngineTool", () => {
  it("returns a bounded safe result for a selected installation", async () => {
    const tool = createGodotInspectEngineTool(stubInspector({ selectedResult: selected() }));
    const result = await tool.execute({}, {});
    expect(result.status).toBe("success");
    if (result.status === "success") {
      const output = result.output as Record<string, unknown>;
      expect(output["installationId"]).toBe("primary");
      expect(output["version"]).toBe("4.7.1.stable.official");
      expect(output["edition"]).toBe("standard");
      expect(output["support"]).toBe("verified");
      expect(Array.isArray(output["verifiedCapabilities"])).toBe(true);
      expect(output["capabilities"]).toBeDefined();
      expect(JSON.stringify(result.output)).not.toContain("C:\\");
      expect(JSON.stringify(result.output)).not.toContain("extension_api.json");
    }
  });

  it("reports no installation truthfully", async () => {
    const tool = createGodotInspectEngineTool(stubInspector({ selectedResult: null }));
    const result = await tool.execute({}, {});
    expect(result.status).toBe("success");
    if (result.status === "success") {
      expect((result.output as Record<string, unknown>)["selected"]).toBe(false);
    }
  });

  it("declares the godot.inspect capability", () => {
    const tool = createGodotInspectEngineTool(stubInspector({}));
    expect(tool.capability).toBe("godot.inspect");
    expect(tool.definition.inputSchema).toEqual({ type: "object", additionalProperties: false });
  });

  it("fails on inspector errors", async () => {
    const failing: GodotInspector = {
      ...stubInspector({}),
      selected(): Promise<GodotSelectedInstallation | null> {
        return Promise.reject(new Error("probe failed"));
      },
    };
    const tool = createGodotInspectEngineTool(failing);
    const result = await tool.execute({}, {});
    expect(result.status).toBe("failed");
  });
});

describe("createGodotInspectProjectTool", () => {
  it("returns the static project profile and states that no code ran", async () => {
    const tool = createGodotInspectProjectTool(
      stubInspector({ projectResult: project(), assessmentResult: assessment() }),
    );
    const result = await tool.execute({}, {});
    expect(result.status).toBe("success");
    if (result.status === "success") {
      const output = result.output as Record<string, unknown>;
      expect(output["detected"]).toBe(true);
      expect(output["name"]).toBe("Test Game");
      expect(output["static"]).toBe(true);
      expect(output["projectCodeExecuted"]).toBe(false);
      expect(output["projectImportPerformed"]).toBe(false);
      expect((output["compatibility"] as Record<string, unknown>)["status"]).toBe("compatible");
    }
  });

  it("works without an engine selection", async () => {
    const tool = createGodotInspectProjectTool(
      stubInspector({
        projectResult: project(),
        assessmentResult: { status: "no-engine", severity: "warning", reasons: ["No engine."] },
      }),
    );
    const result = await tool.execute({}, {});
    expect(result.status).toBe("success");
    if (result.status === "success") {
      expect((result.output as Record<string, unknown>)["detected"]).toBe(true);
    }
  });

  it("reports non-Godot workspaces truthfully", async () => {
    const tool = createGodotInspectProjectTool(stubInspector({}));
    const result = await tool.execute({}, {});
    expect(result.status).toBe("success");
    if (result.status === "success") {
      expect((result.output as Record<string, unknown>)["detected"]).toBe(false);
      expect((result.output as Record<string, unknown>)["projectCodeExecuted"]).toBe(false);
    }
  });

  it("never exposes the complete API dump", async () => {
    const tool = createGodotInspectProjectTool(
      stubInspector({ projectResult: project(), assessmentResult: assessment() }),
    );
    const result = await tool.execute({}, {});
    const serialized = JSON.stringify(result);
    expect(serialized).not.toContain("builtin_class_members");
    expect(serialized).not.toContain("native_structures");
  });
});
