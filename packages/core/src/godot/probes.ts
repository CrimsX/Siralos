import type { GodotCommandCapabilities } from "./capabilities.js";
import type { GodotInstallation } from "./installations.js";
import type { GodotVersion } from "./version.js";

/**
 * Narrow fixed-probe interface owned by core.
 *
 * No arbitrary argument array, provider-controlled argument, project path,
 * or working directory is accepted: the adapter chooses every argument and
 * every working directory. Probes always run through the sandbox backend.
 * Provider adapters cannot invoke this runner directly; only the Godot
 * probe adapter implements it and only Siralos composition consumes it.
 */
export interface GodotProbeRunner {
  /**
   * Reports whether engine probing can execute at all. The pinned runtime
   * cannot bind a sandboxed launch to the exact fingerprinted executable
   * bytes (the staged copy's pathname is re-opened at spawn time by the
   * backend, and a same-user process can substitute it between final
   * verification and spawn); when that boundary cannot be enforced the
   * runner reports unavailable and never spawns the executable.
   */
  isAvailable(): Promise<boolean>;

  probeVersion(installation: GodotInstallation, signal?: AbortSignal): Promise<GodotVersionProbe>;

  probeHelp(installation: GodotInstallation, signal?: AbortSignal): Promise<GodotHelpProbe>;

  dumpExtensionApi(
    installation: GodotInstallation,
    signal?: AbortSignal,
  ): Promise<GodotApiDumpProbe>;
}

export type GodotVersionProbe =
  | {
      readonly status: "success";
      readonly version: GodotVersion;
    }
  | {
      readonly status: "unavailable";
      readonly message: string;
    }
  | {
      readonly status: "failed";
      readonly message: string;
    };

export type GodotHelpProbe =
  | {
      readonly status: "success";
      readonly capabilities: GodotCommandCapabilities;
      /** Count of unrecognized options preserved as a bounded diagnostic. */
      readonly unknownOptionCount: number;
    }
  | {
      readonly status: "unavailable";
      readonly message: string;
    }
  | {
      readonly status: "degraded";
      readonly message: string;
      readonly capabilities: GodotCommandCapabilities;
      readonly unknownOptionCount: number;
    }
  | {
      readonly status: "failed";
      readonly message: string;
    };

/** Bounded summary of an extension API dump; never the dump itself. */
export interface GodotApiDumpSummary {
  readonly headerVersion: string | null;
  readonly apiHash: string | null;
  readonly classCount: number | null;
  readonly builtinClassCount: number | null;
  readonly globalEnumCount: number | null;
  readonly utilityFunctionCount: number | null;
  readonly configurationVersion: number | null;
  readonly fileSizeBytes: number;
  readonly sha256: string;
}

export type GodotApiDumpProbe =
  | {
      readonly status: "success";
      readonly summary: GodotApiDumpSummary;
    }
  | {
      readonly status: "unavailable";
      readonly message: string;
    }
  | {
      readonly status: "degraded" | "failed";
      readonly message: string;
    };
