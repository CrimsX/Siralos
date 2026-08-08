import { describe, expect, it } from "vitest";
import { GODOT_DOCTOR_EXIT_CODES, godotDoctorExitCode, runGodotDoctor } from "./godot-doctor.js";
import {
  assessGodotCompatibility,
  createEmptyGodotProjectProfile,
  type GodotDoctorReport,
  type GodotInstallationOverview,
  type GodotInspector,
} from "@solaris/core";

function overview(overrides: Partial<GodotInstallationOverview> = {}): GodotInstallationOverview {
  return {
    installationId: "primary",
    version: {
      raw: "4.7.1.stable.official",
      major: 4,
      minor: 7,
      patch: 1,
      status: "stable",
      statusNumber: null,
      build: "official",
      commit: null,
    },
    edition: "standard",
    editionConfidence: "high",
    releaseChannel: "stable",
    sourceLabel: "user config",
    source: "user-config",
    support: "verified",
    invalid: null,
    isDuplicate: false,
    selected: true,
    fingerprint: "a".repeat(12),
    profiled: true,
    ...overrides,
  };
}

function report(overrides: Partial<GodotDoctorReport> = {}): GodotDoctorReport {
  const selected = overview();
  const project = createEmptyGodotProjectProfile();
  return {
    discovery: {
      candidates: [selected],
      configuration: {
        activeInstallation: null,
        configuredCount: 1,
        discoverOnPath: false,
        overrides: [],
      },
      selected,
      rationale: [],
      diagnostics: [],
    },
    project,
    compatibility: assessGodotCompatibility(null, project),
    cache: { schemaVersion: 1, cachedProfileCount: 0, enabled: true },
    sandbox: {
      state: "available",
      backendId: "fake-backend",
      filesystemReadRestriction: true,
      filesystemWriteRestriction: true,
      networkRestriction: true,
      processTreeRestriction: true,
    },
    probes: [{ installationId: "primary", probe: "profile", status: "success" }],
    ...overrides,
  };
}

describe("godotDoctorExitCode", () => {
  it("returns 0 for a successful doctor outcome", () => {
    expect(godotDoctorExitCode(report())).toBe(GODOT_DOCTOR_EXIT_CODES.success);
  });

  it("returns 0 when only optional capabilities are degraded", () => {
    const degraded = report();
    const probes = [{ installationId: "primary", probe: "api", status: "degraded" }];
    expect(godotDoctorExitCode({ ...degraded, probes })).toBe(GODOT_DOCTOR_EXIT_CODES.success);
  });

  it("returns the sandbox-unavailable code when the backend state is not available", () => {
    expect(
      godotDoctorExitCode(report({ sandbox: { ...report().sandbox, state: "setup-required" } })),
    ).toBe(GODOT_DOCTOR_EXIT_CODES.sandboxUnavailable);
  });

  it("returns the sandbox-unavailable code when host-read restriction is missing", () => {
    expect(
      godotDoctorExitCode(
        report({ sandbox: { ...report().sandbox, filesystemReadRestriction: false } }),
      ),
    ).toBe(GODOT_DOCTOR_EXIT_CODES.sandboxUnavailable);
  });

  it("returns the selection-failure code for an explicit override with no selection", () => {
    const explicit = report({
      discovery: {
        ...report().discovery,
        configuration: {
          ...report().discovery.configuration,
          overrides: ["explicit executable path override"],
        },
        selected: null,
      },
    });
    expect(godotDoctorExitCode(explicit)).toBe(GODOT_DOCTOR_EXIT_CODES.selectionFailure);
  });

  it("returns the generic-failure code when no engine was selected without an override", () => {
    const none = report({
      discovery: { ...report().discovery, selected: null },
    });
    expect(godotDoctorExitCode(none)).toBe(GODOT_DOCTOR_EXIT_CODES.genericFailure);
  });

  it("returns the probe-failure code when the selected installation was not profiled", () => {
    const unprofiled = report({
      discovery: {
        ...report().discovery,
        candidates: [overview({ profiled: false, invalid: "version probe failed" })],
        selected: overview({ profiled: false, invalid: "version probe failed" }),
      },
    });
    expect(godotDoctorExitCode(unprofiled)).toBe(GODOT_DOCTOR_EXIT_CODES.probeFailure);
  });

  it("returns the identity-mismatch code when an executable changed after validation", () => {
    const mismatched = report({
      discovery: {
        ...report().discovery,
        candidates: [
          overview({
            invalid: "The executable changed after validation; rediscovery is required.",
          }),
        ],
      },
    });
    expect(godotDoctorExitCode(mismatched)).toBe(GODOT_DOCTOR_EXIT_CODES.identityMismatch);
  });
});

describe("runGodotDoctor", () => {
  it("runs the inspector through the factory seam and returns its report", async () => {
    const expected = report();
    const inspector: GodotInspector = {
      discover: () => Promise.resolve(expected.discovery),
      selected: () => Promise.resolve(null),
      projectProfile: () => Promise.resolve(expected.project),
      compatibility: () => Promise.resolve(expected.compatibility),
      doctor: () => Promise.resolve(expected),
    };
    const actual = await runGodotDoctor({
      godotPath: "/opt/godot",
      inspectorFactory: () => Promise.resolve(inspector),
    });
    expect(actual).toBe(expected);
  });
});
