import { createHash } from "node:crypto";
import {
  GODOT_LIMITS,
  GODOT_RECOVERY_PROBE_OFFLINE_PROFILE,
  type GodotEngineProfile,
  type GodotInstallation,
  type SandboxBackend,
  type SandboxBackendStatus,
  type SandboxProfile,
  type SandboxedProcessRequest,
  type SandboxedProcessResult,
} from "@solaris/core";
import { buildChildEnvironment } from "../../environment/child-environment.js";

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
      readonly result: SandboxedProcessResult;
    }
  | {
      readonly status: "unsupported";
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

export interface GodotRecoveryRunnerDependencies {
  readonly backend: SandboxBackend;
  /** Sanitized host parent environment (never raw `process.env`). */
  readonly parentEnvironment: Readonly<Record<string, string>>;
  readonly profile?: SandboxProfile;
  readonly timeoutMs?: number;
  readonly stdoutLimitBytes?: number;
  readonly stderrLimitBytes?: number;
}

/**
 * The recovery-mode runner. Recovery mode is a requirement, not a fallback:
 * an engine that does not advertise `--recovery-mode`, `--editor`,
 * `--headless`, and `--path` is never launched, and no weaker mode is ever
 * substituted. The engine runs through the sandbox backend only.
 */
export function createGodotRecoveryRunner(dependencies: GodotRecoveryRunnerDependencies): {
  run(request: GodotRecoveryRunRequest): Promise<GodotRecoveryRunOutcome>;
} {
  const profile = dependencies.profile ?? GODOT_RECOVERY_PROBE_OFFLINE_PROFILE;
  const timeoutMs = dependencies.timeoutMs ?? GODOT_LIMITS.recoveryProbeTimeoutMs;
  const stdoutLimitBytes = dependencies.stdoutLimitBytes ?? GODOT_LIMITS.maxRecoveryStreamBytes;
  const stderrLimitBytes = dependencies.stderrLimitBytes ?? GODOT_LIMITS.maxRecoveryStreamBytes;

  async function run(request: GodotRecoveryRunRequest): Promise<GodotRecoveryRunOutcome> {
    const capability = requireRecoveryCapabilities(request);
    if (!capability.ok) {
      return { status: "unsupported", message: capability.message };
    }
    if (request.signal?.aborted) {
      throw createAbortError();
    }
    const identity = await revalidateInstallation(request.installation);
    if (!identity.ok) {
      return { status: "failed", message: identity.error, result: emptyResult() };
    }
    if (!(await sandboxEnforced())) {
      return {
        status: "sandbox-unavailable",
        message: "The sandbox is unavailable; the recovery probe did not run (fail closed).",
        result: emptyResult(),
      };
    }
    const arguments_ = godotRecoveryArguments(request.mirrorProjectPath);
    const request_: SandboxedProcessRequest = {
      executable: request.installation.canonicalPath,
      arguments: arguments_,
      workingDirectory: request.mirrorProjectPath,
      profile,
      environment: buildChildEnvironment(dependencies.parentEnvironment, {
        home: request.runPaths.home,
        temp: request.runPaths.temp,
      }),
      runDirectory: request.runPaths.root,
      timeoutMs,
      stdoutLimitBytes,
      stderrLimitBytes,
      ...(request.signal === undefined ? {} : { signal: request.signal }),
    };
    let result: SandboxedProcessResult;
    try {
      result = await dependencies.backend.execute(request_);
    } catch (error: unknown) {
      if (request.signal?.aborted || isAbortError(error)) {
        throw createAbortError();
      }
      return { status: "failed", message: describeFailure(error), result: emptyResult() };
    }
    switch (result.status) {
      case "completed":
        return { status: "completed", result };
      case "cancelled":
        return { status: "cancelled", message: "The recovery probe was cancelled.", result };
      case "timed-out":
        return {
          status: "timed-out",
          message: "The recovery probe timed out and its process tree was terminated.",
          result,
        };
      case "output-limit":
        return {
          status: "output-limit",
          message: "The recovery probe exceeded its output limit and was terminated.",
          result,
        };
      case "sandbox-denied":
        return {
          status: "sandbox-denied",
          message: `The sandbox denied part of the probe: ${result.violations
            .map((violation) => violation.summary)
            .join("; ")}`,
          result,
        };
      case "sandbox-unavailable":
        return {
          status: "sandbox-unavailable",
          message: "The sandbox became unavailable during the probe; the probe failed closed.",
          result,
        };
      case "failed":
        return { status: "failed", message: "The recovery probe process failed to run.", result };
    }
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
        message:
          "The selected executable is runtime-only; it cannot run the editor recovery probe.",
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

  async function revalidateInstallation(
    installation: GodotInstallation,
  ): Promise<{ readonly ok: true } | { readonly ok: false; readonly error: string }> {
    const { revalidateExecutableIdentity } = await import("../discovery/executable-validation.js");
    const result = await revalidateExecutableIdentity({
      canonicalPath: installation.canonicalPath,
      sizeBytes: installation.sizeBytes,
      modifiedAtMs: installation.modifiedAtMs,
      sha256: installation.sha256,
    });
    return result.unchanged ? { ok: true } : { ok: false, error: result.error };
  }

  async function sandboxEnforced(): Promise<boolean> {
    let status: SandboxBackendStatus;
    try {
      status = await dependencies.backend.inspect();
    } catch {
      return false;
    }
    return (
      status.state === "available" &&
      status.capabilities.filesystemWriteRestriction &&
      status.capabilities.networkRestriction &&
      status.capabilities.processTreeRestriction
    );
  }

  return { run };
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

function isAbortError(error: unknown): boolean {
  return (
    error instanceof Error &&
    (error.name === "AbortError" || ("code" in error && error.code === "ABORT_ERR"))
  );
}

function describeFailure(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "An unknown recovery probe failure occurred.";
}

function emptyResult(): SandboxedProcessResult {
  return {
    status: "failed",
    exitCode: null,
    signal: null,
    stdout: "",
    stderr: "",
    stdoutTruncated: false,
    stderrTruncated: false,
    durationMs: 0,
    violations: [],
  };
}

function createAbortError(): Error {
  return new DOMException("The Godot recovery probe was aborted.", "AbortError");
}
