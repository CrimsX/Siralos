import type {
  GodotApiDumpProbe,
  GodotHelpProbe,
  GodotInstallation,
  GodotProbeRunner,
  GodotVersionProbe,
} from "@solaris/core";

export interface GodotProbeRunnerDependencies {
  // The dependencies are retained for signature compatibility; the
  // fail-closed runner never uses them.
  readonly backend: unknown;
  readonly runDirectories: unknown;
  /** Sanitized host parent environment (never raw `process.env`). */
  readonly parentEnvironment?: Readonly<Record<string, string>>;
}

export const GODOT_PROBING_UNAVAILABLE_MESSAGE =
  "Godot engine probing is unavailable: Node and the pinned sandbox runtime offer no identity-bound launch primitive, so the staged executable copy's pathname is re-opened at spawn time and a same-user process can substitute different bytes between final verification and launch. The verified fingerprint could then be attached to bytes that never execute. Probing fails closed and the executable is never spawned; it will become available when a mechanically identity-bound launch primitive exists.";

/**
 * Godot engine probing fails closed and never spawns the executable.
 *
 * The required invariant — the executable opened by the OS must be the exact
 * object whose bytes produced the trusted SHA-256 fingerprint — cannot be
 * enforced with Node's `spawn` against a same-user adversary: the backend
 * re-opens the staged copy's pathname at spawn time, and a substitution in
 * the verify-to-launch window executes unverified bytes under a recorded
 * trusted fingerprint. Re-checking after launch is not prevention, and the
 * pinned runtime exposes no exec-by-handle primitive. Rather than weakening
 * the same-user threat model to keep probes available, every probe reports
 * unavailable and the executable is never spawned.
 */
export function createGodotProbeRunner(
  _dependencies: GodotProbeRunnerDependencies,
): GodotProbeRunner {
  return {
    isAvailable(): Promise<boolean> {
      return Promise.resolve(false);
    },

    probeVersion(_installation: GodotInstallation): Promise<GodotVersionProbe> {
      return Promise.resolve({
        status: "unavailable",
        message: GODOT_PROBING_UNAVAILABLE_MESSAGE,
      });
    },

    probeHelp(_installation: GodotInstallation): Promise<GodotHelpProbe> {
      return Promise.resolve({
        status: "unavailable",
        message: GODOT_PROBING_UNAVAILABLE_MESSAGE,
      });
    },

    dumpExtensionApi(_installation: GodotInstallation): Promise<GodotApiDumpProbe> {
      return Promise.resolve({
        status: "unavailable",
        message: GODOT_PROBING_UNAVAILABLE_MESSAGE,
      });
    },
  };
}
