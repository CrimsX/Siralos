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
