import type { GodotEngineProfile, GodotInstallation, SandboxedProcessResult } from "@solaris/core";

/**
 * Fixed Solaris-owned Godot LSP session invocation. The recovery-mode
 * editor runs against the disposable mirror only, with the dynamically
 * allocated loopback LSP port. The architecture check enforces that this
 * module is the only runtime module that may pair `--lsp-port` with
 * `--recovery-mode`, that `--path` only references the disposable mirror,
 * that `--scene`, `--script`, `--import`, DAP/debug-server, export, and
 * quit options never appear, and that the source workspace never becomes
 * the session project path. Recovery mode remains mandatory; the LSP
 * channel is the only loopback exception to the offline sandbox profile.
 */

/** Marker for the disposable mirror project path (never a real path). */
export const GODOT_LSP_MIRROR_PATH_MARKER = "<disposable-mirror>";

/** Marker for the allocated loopback port (never a real port). */
export const GODOT_LSP_PORT_MARKER = "<allocated-loopback-port>";

export const GODOT_LSP_BASE_ARGUMENTS: readonly string[] = [
  "--headless",
  "--editor",
  "--recovery-mode",
  "--path",
  GODOT_LSP_MIRROR_PATH_MARKER,
  "--lsp-port",
  GODOT_LSP_PORT_MARKER,
];

/** The fixed argument template with markers, used for the command digest. */
export function godotLSPArgumentTemplate(): readonly string[] {
  return [...GODOT_LSP_BASE_ARGUMENTS];
}

/** The invocation arguments for one LSP session against the mirror. */
export function godotLSPArguments(
  mirrorProjectPath: string,
  allocatedPort: number,
): readonly string[] {
  return [
    "--headless",
    "--editor",
    "--recovery-mode",
    "--path",
    mirrorProjectPath,
    "--lsp-port",
    String(allocatedPort),
  ];
}

export interface GodotLSPSessionCommandDigestParts {
  readonly executableSha256: string;
  readonly argumentTemplate: readonly string[];
  readonly workingDirectoryPolicy: "disposable-mirror";
  readonly profileId: string;
  readonly environmentPolicy: "minimal";
  readonly stdinPolicy: "closed";
  readonly networkPolicy: "denied";
  readonly loopbackPolicy: "lsp-only";
  readonly timeoutMs: number;
  readonly stdoutLimitBytes: number;
  readonly stderrLimitBytes: number;
}

export interface GodotLSPServerStartRequest {
  readonly installation: GodotInstallation;
  readonly engineProfile: GodotEngineProfile;
  /** Absolute mirror project path; the source workspace is never used. */
  readonly mirrorProjectPath: string;
  /** Solaris-allocated loopback-only port. */
  readonly allocatedPort: number;
  readonly signal?: AbortSignal;
}

export type GodotLSPServerStartOutcome =
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

export interface GodotLSPServerRunnerDependencies {
  // Retained for signature compatibility; the fail-closed runner never uses them.
  readonly backend: unknown;
  /** Sanitized host parent environment (never raw `process.env`). */
  readonly parentEnvironment?: Readonly<Record<string, string>>;
}

export interface GodotLSPServerRunner {
  isAvailable(): Promise<boolean>;
  startServer(request: GodotLSPServerStartRequest): Promise<GodotLSPServerStartOutcome>;
}

export const GODOT_LSP_UNAVAILABLE_MESSAGE =
  "The Godot GDScript language session is unavailable on this platform: Node and the pinned sandbox runtime offer no exec-by-handle, directory-relative create, or delete-by-handle primitive, so the approved Godot editor cannot be launched against exactly the approved mirrored project bytes, the disposable mirror cannot be constructed or cleaned up identity-bound, and the loopback LSP channel cannot be tied to a verified process identity. Sessions fail closed and the editor is never spawned; no mirror is created, no port is opened, and nothing is executed.";

/**
 * Godot LSP server startup fails closed and never spawns the executable.
 * The fixed invocation would run
 * `--headless --editor --recovery-mode --path <disposable-mirror>
 * --lsp-port <allocated-loopback-port>` inside an enforcing sandbox with
 * the workspace excluded from readable roots, external network denied,
 * stdin closed, and the process tree confined; until launch and mirror
 * lifecycle can be mechanically bound to verified identities, every start
 * reports a typed `unavailable` outcome with zero filesystem or network
 * side effects.
 */
export function createGodotLSPServerRunner(
  _dependencies: GodotLSPServerRunnerDependencies,
): GodotLSPServerRunner {
  return {
    isAvailable(): Promise<boolean> {
      return Promise.resolve(false);
    },
    async startServer(request: GodotLSPServerStartRequest): Promise<GodotLSPServerStartOutcome> {
      if (request.signal?.aborted) {
        throw createAbortError();
      }
      const capability = requireLSPSessionCapabilities(request);
      if (!capability.ok) {
        return { status: "unsupported", message: capability.message };
      }
      await Promise.resolve();
      return {
        status: "unavailable",
        message: GODOT_LSP_UNAVAILABLE_MESSAGE,
      };
    },
  };
}

function requireLSPSessionCapabilities(request: GodotLSPServerStartRequest):
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
      message:
        "The selected executable is runtime-only; it cannot host a GDScript language server.",
    };
  }
  if (!request.engineProfile.capabilities.lsp) {
    return {
      ok: false,
      message:
        "The selected Godot version does not advertise --lsp-port; the language session is unsupported.",
    };
  }
  if (
    !request.engineProfile.capabilities.recoveryMode ||
    !request.engineProfile.capabilities.editor ||
    !request.engineProfile.capabilities.headless ||
    !request.engineProfile.capabilities.projectPath
  ) {
    return {
      ok: false,
      message:
        "The selected Godot version does not advertise the required --recovery-mode, --editor, --headless, and --path options; the language session is unsupported.",
    };
  }
  return { ok: true };
}

function createAbortError(): Error {
  return new DOMException("The Godot language session startup was aborted.", "AbortError");
}
