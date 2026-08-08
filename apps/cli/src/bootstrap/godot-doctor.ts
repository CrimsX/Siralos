import type { GodotDoctorReport, GodotInspector } from "@solaris/core";

export interface GodotDoctorOptions {
  /** `--godot-path` override. */
  readonly godotPath?: string;
  /** `--godot-installation` override. */
  readonly godotInstallation?: string;
  /** Test seam; defaults to the composed application inspector. */
  readonly inspectorFactory?: (options: {
    readonly godotPath?: string;
    readonly godotInstallation?: string;
  }) => Promise<GodotInspector>;
}

/**
 * Documented `--godot-doctor` exit codes:
 *
 * - 0: the doctor succeeded — the sandbox enforces every restriction, a
 *   selection was resolved (explicit overrides included), and the selected
 *   installation is profiled.
 * - 1: generic failure — no valid engine is selected when selection was
 *   required (e.g. nothing selectable was discovered under automatic
 *   selection).
 * - 2: selection failure — an explicit `--godot-path`/`--godot-installation`
 *   (or environment override) did not resolve to a valid, probed
 *   installation and the failure was reported inside the report (explicit
 *   selection failures that throw also exit nonzero through startup error
 *   handling).
 * - 3: sandbox unavailable — the backend state is not "available" or one of
 *   the required restrictions (host-read, host-write, network, process
 *   tree) is missing, so probes cannot run safely.
 * - 4: probe failure — the selected installation could not be profiled (no
 *   valid probe result).
 * - 5: identity mismatch — an executable changed after validation and must
 *   be rediscovered before anything can run.
 *
 * Policy: version/help probes and sandbox readiness are required; an API
 * dump failure stays a DEGRADED diagnostic and never fails the doctor by
 * itself. Failed cleanup with uncertain safety surfaces as a probe failure.
 */
export const GODOT_DOCTOR_EXIT_CODES = {
  success: 0,
  genericFailure: 1,
  selectionFailure: 2,
  sandboxUnavailable: 3,
  probeFailure: 4,
  identityMismatch: 5,
} as const;

/**
 * Computes the `--godot-doctor` exit code from the report. Returns 0 only
 * for a successful doctor outcome; degraded optional capabilities (e.g. a
 * degraded extension-api dump) do not fail the doctor.
 */
export function godotDoctorExitCode(report: GodotDoctorReport): number {
  const sandbox = report.sandbox;
  if (
    sandbox.state !== "available" ||
    !sandbox.filesystemReadRestriction ||
    !sandbox.filesystemWriteRestriction ||
    !sandbox.networkRestriction ||
    !sandbox.processTreeRestriction
  ) {
    return GODOT_DOCTOR_EXIT_CODES.sandboxUnavailable;
  }
  for (const candidate of report.discovery.candidates) {
    if (candidate.invalid !== null && candidate.invalid.includes("changed after validation")) {
      return GODOT_DOCTOR_EXIT_CODES.identityMismatch;
    }
  }
  if (report.discovery.selected === null) {
    const explicitSelection = report.discovery.configuration.overrides.some((override) =>
      override.startsWith("explicit"),
    );
    return explicitSelection
      ? GODOT_DOCTOR_EXIT_CODES.selectionFailure
      : GODOT_DOCTOR_EXIT_CODES.genericFailure;
  }
  if (!report.discovery.selected.profiled || report.discovery.selected.invalid !== null) {
    return GODOT_DOCTOR_EXIT_CODES.probeFailure;
  }
  return GODOT_DOCTOR_EXIT_CODES.success;
}

/**
 * Non-interactive `--godot-doctor` mode: runs bounded Godot diagnostics and
 * exits. Requires no model credential and never opens, imports, or runs a
 * project. Explicit selection failures fail loudly.
 */
export async function runGodotDoctor(options: GodotDoctorOptions = {}): Promise<GodotDoctorReport> {
  const inspector =
    options.inspectorFactory === undefined
      ? await defaultInspector(options)
      : await options.inspectorFactory(options);
  return inspector.doctor();
}

async function defaultInspector(options: {
  readonly godotPath?: string;
  readonly godotInstallation?: string;
}): Promise<GodotInspector> {
  const { createCliApplication } = await import("./create-application.js");
  const application = await createCliApplication({
    ...(options.godotPath === undefined ? {} : { godotPath: options.godotPath }),
    ...(options.godotInstallation === undefined
      ? {}
      : { godotInstallation: options.godotInstallation }),
  });
  return application.godot;
}
