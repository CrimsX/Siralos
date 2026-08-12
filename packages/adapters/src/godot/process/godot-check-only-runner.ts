import type { GodotEngineProfile, GodotInstallation, SandboxedProcessResult } from "@siralos/core";

/** Marker for the disposable mirror project path (never a real path). */
export const GODOT_CHECK_ONLY_MIRROR_PATH_MARKER = "<disposable-mirror>";

/** Marker for the mirrored script path (never a real path). */
export const GODOT_CHECK_ONLY_MIRROR_SCRIPT_MARKER = "<mirror-script>";

/**
 * Fixed Siralos-owned GDScript check-only invocation.
 *
 * The only legitimate `--script` invocation in Siralos is the check-only
 * diagnostic adapter: `--headless --path <disposable-mirror> --script
 * <mirror-script> --check-only`. The architecture check enforces that this
 * module is the only runtime module that may pair `--script` with
 * `--check-only`, that `--path` may only reference the disposable mirror,
 * that `--scene`, `--editor`, `--import`, and LSP/DAP options never appear,
 * and that the source workspace can never become the diagnostic `--path`.
 *
 * `--check-only` is the security-relevant invariant: Godot parses the
 * script and reports diagnostics without executing gameplay, scenes, or
 * scripts. If the selected engine does not advertise `--check-only`, the
 * check refuses as unsupported and the script is never run normally.
 */
export const GODOT_CHECK_ONLY_BASE_ARGUMENTS: readonly string[] = [
  "--headless",
  "--path",
  GODOT_CHECK_ONLY_MIRROR_PATH_MARKER,
  "--script",
  GODOT_CHECK_ONLY_MIRROR_SCRIPT_MARKER,
  "--check-only",
];

/**
 * The fixed argument template with the mirror project and script paths
 * canonicalized to markers, used for the command digest. Digesting the
 * markers (never absolute paths) keeps the template stable across runs and
 * keeps absolute mirror paths out of every digest and event.
 */
export function godotCheckOnlyArgumentTemplate(): readonly string[] {
  return [...GODOT_CHECK_ONLY_BASE_ARGUMENTS];
}

/** The invocation arguments for one check-only run against the mirror. */
export function godotCheckOnlyArguments(
  mirrorProjectPath: string,
  mirrorScriptPath: string,
): readonly string[] {
  return ["--headless", "--path", mirrorProjectPath, "--script", mirrorScriptPath, "--check-only"];
}

export interface GodotCheckOnlyRunRequest {
  readonly installation: GodotInstallation;
  readonly engineProfile: GodotEngineProfile;
  /** Absolute mirror project path; the source workspace is never used. */
  readonly mirrorProjectPath: string;
  /** Absolute mirrored script path inside the mirror project. */
  readonly mirrorScriptPath: string;
  readonly signal?: AbortSignal;
}

export type GodotCheckOnlyRunOutcome =
  | {
      readonly status: "completed";
      readonly result: SandboxedProcessResult;
    }
  | {
      readonly status: "unsupported";
      readonly message: string;
    }
  | {
      readonly status: "unavailable";
      readonly message: string;
    }
  | {
      readonly status:
        | "timed-out"
        | "cancelled"
        | "sandbox-denied"
        | "sandbox-unavailable"
        | "output-limit"
        | "failed";
      readonly message: string;
      readonly result: SandboxedProcessResult;
    };

export interface GodotCheckOnlyRunnerDependencies {
  // The dependencies are retained for signature compatibility; the
  // fail-closed runner never uses them.
  readonly backend: unknown;
  /** Sanitized host parent environment (never raw `process.env`). */
  readonly parentEnvironment?: Readonly<Record<string, string>>;
}

export interface GodotCheckOnlyRunner {
  isAvailable(): Promise<boolean>;
  runCheckOnly(request: GodotCheckOnlyRunRequest): Promise<GodotCheckOnlyRunOutcome>;
}

export const GODOT_CHECK_ONLY_UNAVAILABLE_MESSAGE =
  "GDScript check-only diagnostics are unavailable on this platform: Node and the pinned sandbox runtime offer no exec-by-handle, directory-relative create, or delete-by-handle primitive, so the approved Godot identity cannot be launched against exactly the approved mirrored script bytes and the disposable mirror cannot be constructed or cleaned up identity-bound. Diagnostics fail closed and the engine is never spawned; no mirror is created and nothing is executed.";

/**
 * GDScript check-only execution fails closed and never spawns the
 * executable. The fixed invocation would run
 * `--headless --path <disposable-mirror> --script <mirror-script>
 * --check-only` inside an enforcing sandbox with the workspace excluded
 * from readable roots, stdin closed, and network denied; until launch and
 * mirror lifecycle can be mechanically bound to verified identities, every
 * check reports a typed `unavailable` outcome with zero filesystem side
 * effects.
 */
export function createGodotCheckOnlyRunner(
  _dependencies: GodotCheckOnlyRunnerDependencies,
): GodotCheckOnlyRunner {
  return {
    isAvailable(): Promise<boolean> {
      return Promise.resolve(false);
    },
    async runCheckOnly(request: GodotCheckOnlyRunRequest): Promise<GodotCheckOnlyRunOutcome> {
      if (request.signal?.aborted) {
        throw createAbortError();
      }
      const capability = requireCheckOnlyCapabilities(request);
      if (!capability.ok) {
        return { status: "unsupported", message: capability.message };
      }
      await Promise.resolve();
      return {
        status: "unavailable",
        message: GODOT_CHECK_ONLY_UNAVAILABLE_MESSAGE,
      };
    },
  };
}

function requireCheckOnlyCapabilities(request: GodotCheckOnlyRunRequest):
  | {
      readonly ok: true;
    }
  | {
      readonly ok: false;
      readonly message: string;
    } {
  if (request.installation.status !== "valid") {
    return { ok: false, message: "The installation is invalid; rediscovery is required." };
  }
  if (request.engineProfile.edition === "runtime-only") {
    return {
      ok: false,
      message: "The selected executable is runtime-only; it cannot parse GDScript.",
    };
  }
  if (!request.engineProfile.capabilities.checkOnly) {
    return {
      ok: false,
      message:
        "The selected Godot version does not advertise --check-only; GDScript diagnostics are unsupported and the script is never run normally.",
    };
  }
  if (!request.engineProfile.capabilities.script || !request.engineProfile.capabilities.headless) {
    return {
      ok: false,
      message:
        "The selected Godot version does not advertise --script and --headless; GDScript diagnostics are unsupported.",
    };
  }
  if (!request.engineProfile.capabilities.projectPath) {
    return {
      ok: false,
      message:
        "The selected Godot version does not advertise --path; the mirror project cannot be opened for diagnostics.",
    };
  }
  return { ok: true };
}

function createAbortError(): Error {
  return new DOMException("The GDScript check was aborted.", "AbortError");
}
