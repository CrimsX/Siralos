import { createHash } from "node:crypto";
import { GODOT_LIMITS, type GodotEngineProfile, type GodotInstallation } from "@solaris/core";

/**
 * Fixed Solaris-owned recovery-mode editor invocation. The editor is always
 * launched headless in recovery mode against the disposable mirror project;
 * the mirror path is the only variable argument and it is always
 * Solaris-generated. The architecture check enforces that this module is the
 * only runtime module that may carry the project path option and that it
 * never carries script, scene, import, export, LSP/DAP, or debug options.
 */
export const GODOT_RECOVERY_BASE_ARGUMENTS: readonly string[] = [
  "--headless",
  "--editor",
  "--recovery-mode",
];

/** Canonical placeholder for the Solaris-generated mirror path in digests. */
export const GODOT_RECOVERY_MIRROR_PATH_MARKER = "<disposable-mirror>";

export function godotRecoveryArguments(mirrorProjectPath: string): readonly string[] {
  return [
    ...GODOT_RECOVERY_BASE_ARGUMENTS,
    "--path",
    mirrorProjectPath,
    "--quit-after",
    String(GODOT_LIMITS.recoveryQuitAfterIterations),
  ];
}

/** Fixed argument template with the mirror path canonicalized to the marker. */
export function godotRecoveryArgumentTemplate(): readonly string[] {
  return [
    ...GODOT_RECOVERY_BASE_ARGUMENTS,
    "--path",
    GODOT_RECOVERY_MIRROR_PATH_MARKER,
    "--quit-after",
    String(GODOT_LIMITS.recoveryQuitAfterIterations),
  ];
}

export interface GodotRecoveryCommandDigestParts {
  readonly executableSha256: string;
  readonly argumentTemplate: readonly string[];
  readonly workingDirectoryPolicy: "disposable-mirror";
  readonly profileId: string;
  readonly environmentPolicy: "minimal";
  readonly stdinPolicy: "closed";
  readonly networkPolicy: "denied";
  readonly timeoutMs: number;
  readonly stdoutLimitBytes: number;
  readonly stderrLimitBytes: number;
}

/**
 * Deterministic digest over the fixed recovery command. The mirror path is
 * canonicalized to the marker so the digest is stable between approval and
 * execution while still binding every Solaris-chosen aspect of the command.
 */
export function computeGodotRecoveryCommandDigest(parts: GodotRecoveryCommandDigestParts): string {
  const canonical = JSON.stringify(sortDeep(parts));
  return createHash("sha256").update(canonical, "utf8").digest("hex");
}

export interface GodotRecoveryRunRequest {
  readonly installation: GodotInstallation;
  readonly engineProfile: GodotEngineProfile;
  /** The prepared disposable mirror project path (Solaris-generated). */
  readonly mirrorProjectPath: string;
  readonly runPaths: {
    readonly root: string;
    readonly home: string;
    readonly temp: string;
  };
  readonly signal?: AbortSignal;
}

export type GodotRecoveryRunOutcome =
  | {
      readonly status: "completed";
      readonly result: import("@solaris/core").SandboxedProcessResult;
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
      readonly result: import("@solaris/core").SandboxedProcessResult;
    };

export interface GodotRecoveryRunnerDependencies {
  // The dependencies are retained for signature compatibility; the
  // fail-closed runner never uses them.
  readonly backend: unknown;
  /** Sanitized host parent environment (never raw `process.env`). */
  readonly parentEnvironment?: Readonly<Record<string, string>>;
}

export interface GodotRecoveryRunner {
  isAvailable(): Promise<boolean>;
  run(request: GodotRecoveryRunRequest): Promise<GodotRecoveryRunOutcome>;
}

export const GODOT_RECOVERY_RUN_UNAVAILABLE_MESSAGE =
  "Recovery-mode Godot execution is unavailable: Node and the pinned sandbox runtime offer no exec-by-handle or directory-handle-relative primitive, so the staged executable's pathname is re-opened at spawn time and a same-user process could substitute different bytes between final verification and launch, the verified parent could be substituted before mirror creation, and cleanup could delete a substituted object. The verified fingerprint could then be attached to bytes that never execute. The runner fails closed and never spawns the executable, and no mirror is created; it will become available only when a mechanically identity-bound launch and mirror-lifecycle primitive exists.";

/**
 * The recovery-mode runner fails closed and never spawns the executable.
 *
 * The required invariant — the executable opened by the OS must be the exact
 * object whose bytes produced the trusted SHA-256 fingerprint, running
 * against a mirror that contains exactly the approved bytes with cleanup
 * bound to the exact created objects — cannot be enforced with Node's
 * pathname-based spawn against a same-user adversary: the backend re-opens
 * the staged copy's pathname at spawn time, and a substitution in the
 * verify-to-launch window executes unverified bytes under a recorded
 * trusted fingerprint. Re-checking after launch is not prevention. Rather
 * than weakening the same-user threat model, every run reports a typed
 * `unavailable` outcome and the executable is never spawned.
 *
 * Recovery mode remains a requirement, not a fallback: an engine that does
 * not advertise `--recovery-mode`, `--editor`, `--headless`, and `--path`
 * is reported `unsupported` and no weaker mode is ever substituted.
 */
export function createGodotRecoveryRunner(
  _dependencies: GodotRecoveryRunnerDependencies,
): GodotRecoveryRunner {
  return {
    isAvailable(): Promise<boolean> {
      return Promise.resolve(false);
    },
    async run(request: GodotRecoveryRunRequest): Promise<GodotRecoveryRunOutcome> {
      if (request.signal?.aborted) {
        throw createAbortError();
      }
      const capability = requireRecoveryCapabilities(request);
      if (!capability.ok) {
        return { status: "unsupported", message: capability.message };
      }
      await Promise.resolve();
      return {
        status: "unavailable",
        message: GODOT_RECOVERY_RUN_UNAVAILABLE_MESSAGE,
      };
    },
  };
}

function requireRecoveryCapabilities(request: GodotRecoveryRunRequest):
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
      message: "The selected executable is runtime-only; it cannot run the editor recovery probe.",
    };
  }
  const capabilities = request.engineProfile.capabilities;
  if (!capabilities.recoveryMode) {
    return {
      ok: false,
      message:
        "The selected Godot version does not advertise --recovery-mode; the recovery probe is unsupported and no weaker mode is used.",
    };
  }
  if (!capabilities.editor) {
    return {
      ok: false,
      message:
        "The selected Godot version does not advertise --editor; the recovery probe is unsupported.",
    };
  }
  if (!capabilities.headless) {
    return {
      ok: false,
      message:
        "The selected Godot version does not advertise --headless; the recovery probe is unsupported.",
    };
  }
  if (!capabilities.projectPath) {
    return {
      ok: false,
      message:
        "The selected Godot version does not advertise --path; the recovery probe is unsupported.",
    };
  }
  return { ok: true };
}

function sortDeep(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(sortDeep);
  }
  if (value !== null && typeof value === "object") {
    const record = value as Record<string, unknown>;
    const sorted: Record<string, unknown> = {};
    for (const key of Object.keys(record).sort()) {
      sorted[key] = sortDeep(record[key]);
    }
    return sorted;
  }
  return value;
}

function createAbortError(): Error {
  return new DOMException("The Godot recovery probe was aborted.", "AbortError");
}
