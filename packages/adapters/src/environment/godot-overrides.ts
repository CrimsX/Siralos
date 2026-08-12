import { readParentEnvironment } from "./child-environment.js";

export interface GodotEnvironmentOverrides {
  /** `SIRALOS_GODOT` absolute executable path, when set. */
  readonly path: string | null;
  /** `SIRALOS_GODOT_INSTALLATION` installation id, when set. */
  readonly installationId: string | null;
}

/**
 * Reads the trusted Godot environment overrides from the host environment.
 * This is the only place Godot override environment variables are read:
 * the CLI never touches `process.env` directly and project files can never
 * influence these values.
 */
export function readGodotEnvironmentOverrides(): GodotEnvironmentOverrides {
  const parent = readParentEnvironment();
  return {
    path: parent["SIRALOS_GODOT"] ?? null,
    installationId: parent["SIRALOS_GODOT_INSTALLATION"] ?? null,
  };
}
