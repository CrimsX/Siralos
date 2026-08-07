import type { GodotSelectionPreference } from "@solaris/core";
import type { UserGodotConfig } from "../../config/user-config.js";

export interface GodotSelectionInput {
  readonly cliPath: string | null;
  readonly cliInstallationId: string | null;
  readonly environmentPath: string | null;
  readonly environmentInstallationId: string | null;
  readonly config: UserGodotConfig;
}

export type GodotSelectionResolution =
  | {
      readonly ok: true;
      readonly preference: GodotSelectionPreference;
    }
  | {
      readonly ok: false;
      readonly message: string;
    };

/**
 * Resolves the trusted selection precedence (highest first):
 *
 * `--godot-path`, `--godot-installation`, `SOLARIS_GODOT`,
 * `SOLARIS_GODOT_INSTALLATION`, `godot.activeInstallation`, preferred
 * compatible PATH candidate, no selection.
 *
 * Path selection and installation-id selection are mutually exclusive at
 * the same precedence level. An explicit higher-precedence selection that
 * later fails (unknown id, invalid executable) is a hard failure: Solaris
 * never silently falls back after an explicit selection.
 */
export function resolveGodotSelection(input: GodotSelectionInput): GodotSelectionResolution {
  if (input.cliPath !== null && input.cliInstallationId !== null) {
    return {
      ok: false,
      message: "--godot-path and --godot-installation are mutually exclusive.",
    };
  }
  if (input.cliPath !== null) {
    return { ok: true, preference: { kind: "path", path: input.cliPath } };
  }
  if (input.cliInstallationId !== null) {
    return {
      ok: true,
      preference: { kind: "installation-id", installationId: input.cliInstallationId },
    };
  }
  if (input.environmentPath !== null && input.environmentInstallationId !== null) {
    return {
      ok: false,
      message: "SOLARIS_GODOT and SOLARIS_GODOT_INSTALLATION are mutually exclusive.",
    };
  }
  if (input.environmentPath !== null) {
    return { ok: true, preference: { kind: "path", path: input.environmentPath } };
  }
  if (input.environmentInstallationId !== null) {
    return {
      ok: true,
      preference: { kind: "installation-id", installationId: input.environmentInstallationId },
    };
  }
  if (input.config.activeInstallation !== null) {
    return { ok: true, preference: { kind: "config-active" } };
  }
  if (input.config.discoverOnPath) {
    return { ok: true, preference: { kind: "auto" } };
  }
  return { ok: true, preference: { kind: "none" } };
}
