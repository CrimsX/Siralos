import {
  assessGodotCompatibility,
  computeGodotCheckOnlyCommandDigest,
  computeGodotPreparedCheckDigest,
  GODOT_DIAGNOSTICS_OFFLINE_PROFILE,
  GODOT_LIMITS,
  type GodotApplicationEvent,
  type GodotCheckPreparationResult,
  type GodotDiagnostics,
  type GodotDiagnosticsExecutionContext,
  type GodotDiagnosticsRequest,
  type GodotDiagnosticsStatus,
  type GodotDiagnosticsSupport,
  type GodotDiagnosticPreview,
  type GodotEngineProfile,
  type GodotInstallation,
  type GodotProjectCheckResult,
  type GodotProbeRunner,
  type GodotScriptCheckTarget,
  type GodotSelectionPreference,
  type PreparedGDScriptCheck,
  type ProjectMirror,
  type SandboxBackend,
} from "@solaris/core";
import type { RunDirectoryProvider } from "../../process/run-directories.js";
import type { GodotEngineProfileCache } from "../cache/engine-profile-cache.js";
import type { UserGodotConfig } from "../../config/user-config.js";
import { createGodotEngineProfiler, type GodotEngineProfiler } from "../profile/engine-profiler.js";
import { createProjectMirror } from "../mirror/project-mirror.js";
import {
  createGodotCheckOnlyRunner,
  godotCheckOnlyArgumentTemplate,
  type GodotCheckOnlyRunner,
} from "../process/godot-check-only-runner.js";
import { createAbortError, refreshGodotProjectRiskManifest } from "../probe/risk-manifest.js";
import {
  enumerateGDScriptFiles,
  hashScriptTarget,
  validateCheckScript,
} from "./script-enumeration.js";
import { createPreparedCheckStore, type PreparedCheckPlan } from "./prepared-check-store.js";

export const GODOT_CHECK_EXECUTION_UNAVAILABLE_MESSAGE =
  "GDScript check-only diagnostics are unavailable on this platform: the exact approved Godot identity cannot be launched against exactly the approved mirrored script bytes, the disposable mirror cannot be constructed with exactly the approved bytes, and its cleanup cannot be bound to the exact created objects, because Node and the pinned sandbox runtime offer no exec-by-handle, directory-relative create, or delete-by-handle primitive. Nothing was created, nothing was deleted, and no engine was launched.";

export interface GodotDiagnosticsServiceDependencies {
  readonly workspaceRoot: string;
  readonly config: UserGodotConfig;
  readonly preference: GodotSelectionPreference;
  readonly overrideSource: "cli" | "environment" | null;
  readonly backend: SandboxBackend;
  readonly probeRunner: GodotProbeRunner;
  readonly cache: GodotEngineProfileCache;
  readonly hostPath: string | null;
  readonly hostPathExt: string | null;
  readonly platform: NodeJS.Platform;
  readonly runDirectories: RunDirectoryProvider;
  /** Disposable mirror adapter; production wires the fail-closed mirror. */
  readonly mirror?: ProjectMirror;
  /** Check-only runner; production wires the fail-closed runner. */
  readonly checkRunner?: GodotCheckOnlyRunner;
  /** Verified checkpoint storage root; mirrors must never resolve inside it. */
  readonly checkpointRoot: string | null;
  /** Sanitized host parent environment (never raw `process.env`). */
  readonly parentEnvironment: Readonly<Record<string, string>>;
  readonly onEvent?: (event: GodotApplicationEvent) => void;
  /** Prepared-check store configuration; tests inject bounds and clocks. */
  readonly preparedCheckStoreConfig?: {
    readonly maxChecks?: number;
    readonly maxStateBytes?: number;
    readonly ttlMs?: number;
    readonly now?: () => number;
  };
}

/**
 * Authoritative read-only GDScript diagnostics.
 *
 * `prepare` statically validates and hashes the exact requested scripts
 * (single path or bounded project-wide enumeration), refreshes the risk
 * inventory, and freezes a digest; approval binds to that digest; `execute`
 * revalidates every security-relevant input (executable identity, risk
 * manifest, script content hashes, policy constants) and then refuses with
 * a typed `unavailable` outcome unless the platform can mechanically bind
 * execution to the approved bytes. The designed execution is strictly
 * sequential — one disposable mirror per run, one `--check-only`
 * invocation per script, deterministic aggregation, full cancellation, and
 * mirror cleanup — and it always uses the disposable mirror, never the
 * source workspace. A changed project, engine, or script is a conflict
 * that requires a new approval; an approval is never reused, never
 * persisted, and every prepared check is single-use, expiring, and
 * disposable.
 */
export function createGodotDiagnosticsService(
  dependencies: GodotDiagnosticsServiceDependencies,
): GodotDiagnostics {
  const profiler: GodotEngineProfiler = createGodotEngineProfiler(dependencies);
  const mirrorAdapter: ProjectMirror = dependencies.mirror ?? createProjectMirror();
  const checkRunner: GodotCheckOnlyRunner =
    dependencies.checkRunner ?? createGodotCheckOnlyRunner({ backend: dependencies.backend });
  const preparedStore = createPreparedCheckStore(dependencies.preparedCheckStoreConfig);
  let invalidated = false;

  async function support(): Promise<GodotDiagnosticsSupport> {
    const available =
      (await mirrorAdapter.isAvailable()) &&
      (await checkRunner.isAvailable()) &&
      (await sandboxEnforced());
    return {
      state: available ? "available" : "unavailable",
      reason: available ? null : GODOT_CHECK_EXECUTION_UNAVAILABLE_MESSAGE,
      platform: dependencies.platform,
    };
  }

  async function prepare(
    request: GodotDiagnosticsRequest,
    signal?: AbortSignal,
  ): Promise<GodotCheckPreparationResult> {
    if (signal?.aborted) {
      throw createAbortError();
    }
    if (request.paths !== undefined && !Array.isArray(request.paths)) {
      return {
        status: "invalid_input",
        message: "The paths filter must be an array of workspace-relative .gd paths.",
      };
    }
    const selection = await profiler.selectedProfile(signal);
    if (selection === null) {
      return {
        status: "unsupported",
        message: "No trusted Godot installation is selected; GDScript diagnostics cannot run.",
      };
    }
    const capability = requireDiagnosticCapabilities(selection.profile);
    if (!capability.ok) {
      return { status: "unsupported", message: capability.message };
    }
    const refresh = await refreshGodotProjectRiskManifest({
      workspaceRoot: dependencies.workspaceRoot,
      installation: selection.installation,
      profile: selection.profile,
      ...(signal === undefined ? {} : { signal }),
    });
    if (!refresh.ok) {
      return { status: "failed", message: refresh.message };
    }
    const { manifest, projectProfile } = refresh;
    const targets = await resolveTargets(
      request.paths !== undefined && request.paths.length > 0 ? request.paths : undefined,
      signal,
    );
    if (!targets.ok) {
      return {
        status: targets.reason === "invalid-input" ? "invalid_input" : "failed",
        message: targets.message,
      };
    }
    if (targets.targets.length === 0) {
      return {
        status: "failed",
        message: "No .gd scripts were found to check.",
      };
    }
    const compatibility = assessGodotCompatibility(selection.profile, projectProfile);
    const single = request.paths !== undefined && request.paths.length > 0;
    const preview: GodotDiagnosticPreview = {
      projectName: projectProfile.name,
      engineVersion: selection.profile.version.raw,
      installationId: selection.installation.id,
      engineEdition: selection.profile.edition,
      support: selection.profile.support,
      compatibility: compatibility.status,
      scripts: {
        count: targets.targets.length,
        paths: single ? targets.targets.map((target) => target.path) : null,
        totalBytes: targets.targets.reduce((total, target) => total + target.bytes, 0),
      },
      operation: "parse-only",
      isolation: {
        sourceWorkspace: "not-used-as-project",
        disposableMirror: true,
        checkOnly: true,
        headless: true,
        sceneExecution: "disabled",
        gameExecution: "disabled",
        network: "denied",
        environment: "minimal",
        stdin: "closed",
      },
      manifestDigest: manifest.digest,
    };
    const digest = computeGodotPreparedCheckDigest({
      scriptTargets: targets.targets,
      manifestDigest: manifest.digest,
      commandDigest: checkCommandDigest(selection.installation),
      sandboxProfileId: GODOT_DIAGNOSTICS_OFFLINE_PROFILE.id,
      checkLimits: {
        timeoutMs: GODOT_LIMITS.gdscriptCheckTimeoutMs,
        maxScripts: GODOT_LIMITS.maxGDScriptFilesPerProject,
        maxTotalBytes: GODOT_LIMITS.maxGDScriptTotalBytes,
        maxDiagnosticsPerScript: GODOT_LIMITS.maxDiagnosticsPerScript,
        maxDiagnosticsPerRun: GODOT_LIMITS.maxDiagnosticsPerRun,
      },
    });
    const stored = preparedStore.put({
      preview,
      digest,
      manifestDigest: manifest.digest,
      scriptTargets: targets.targets,
      selection,
    });
    if (!stored.ok) {
      return { status: "failed", message: stored.message };
    }
    return { status: "ready", check: stored.check, preview, digest };
  }

  async function execute(
    check: PreparedGDScriptCheck,
    context: GodotDiagnosticsExecutionContext,
  ): Promise<GodotProjectCheckResult> {
    const plan = preparedStore.consume(check);
    if (plan === null) {
      return {
        status: "failed",
        message: "The prepared check is not valid for this session; prepare a new check.",
      };
    }
    if (context.approvedDigest !== plan.digest) {
      invalidated = true;
      return {
        status: "conflict",
        message: "The approval does not match the prepared check; a new approval is required.",
      };
    }
    const revalidated = await revalidatePreparedState(plan, context.signal);
    if (!revalidated.ok) {
      invalidated = true;
      return { status: "conflict", message: revalidated.message };
    }
    if (context.signal?.aborted) {
      throw createAbortError();
    }
    // The execution gate: the mirror, the runner, and the sandbox must all
    // be able to enforce their invariants before anything runs. On this
    // stage every component is fail-closed, so this refusal happens before
    // a mirror is created or an engine is launched, with zero side effects.
    if (!(await executionAvailable())) {
      return {
        status: "unavailable",
        message: GODOT_CHECK_EXECUTION_UNAVAILABLE_MESSAGE,
      };
    }
    return {
      status: "unavailable",
      message: GODOT_CHECK_EXECUTION_UNAVAILABLE_MESSAGE,
    };
  }

  async function revalidatePreparedState(
    plan: PreparedCheckPlan,
    signal: AbortSignal | undefined,
  ): Promise<{ readonly ok: true } | { readonly ok: false; readonly message: string }> {
    if (plan.selection.installation.status !== "valid") {
      return { ok: false, message: "The installation is invalid; rediscovery is required." };
    }
    const { revalidateExecutableIdentity } = await import("../discovery/executable-validation.js");
    const identity = await revalidateExecutableIdentity({
      canonicalPath: plan.selection.installation.canonicalPath,
      sizeBytes: plan.selection.installation.sizeBytes,
      modifiedAtMs: plan.selection.installation.modifiedAtMs,
      sha256: plan.selection.installation.sha256,
    });
    if (!identity.unchanged) {
      return {
        ok: false,
        message:
          "The selected Godot executable changed after approval; a new approval is required.",
      };
    }
    const refresh = await refreshGodotProjectRiskManifest({
      workspaceRoot: dependencies.workspaceRoot,
      installation: plan.selection.installation,
      profile: plan.selection.profile,
      ...(signal === undefined ? {} : { signal }),
    });
    if (!refresh.ok) {
      return { ok: false, message: refresh.message };
    }
    if (refresh.manifest.digest !== plan.manifestDigest) {
      return {
        ok: false,
        message: "The project risk manifest changed after approval; a new approval is required.",
      };
    }
    for (const target of plan.scriptTargets) {
      const current = await hashScriptTarget({
        workspaceRoot: dependencies.workspaceRoot,
        relativePath: target.path,
        ...(signal === undefined ? {} : { signal }),
      });
      if (current === null || current.sha256 !== target.sha256 || current.bytes !== target.bytes) {
        return {
          ok: false,
          message: `The script ${target.path} changed after approval; a new approval is required.`,
        };
      }
    }
    return { ok: true };
  }

  async function resolveTargets(
    paths: readonly string[] | undefined,
    signal: AbortSignal | undefined,
  ): Promise<
    | { readonly ok: true; readonly targets: readonly GodotScriptCheckTarget[] }
    | { readonly ok: false; readonly reason: "invalid-input" | "failed"; readonly message: string }
  > {
    if (paths !== undefined) {
      const targets: GodotScriptCheckTarget[] = [];
      for (const path of paths) {
        if (typeof path !== "string") {
          return {
            ok: false,
            reason: "invalid-input",
            message: "The paths filter contains a non-string entry.",
          };
        }
        const validated = await validateCheckScript({
          workspaceRoot: dependencies.workspaceRoot,
          relativePath: path,
          ...(signal === undefined ? {} : { signal }),
        });
        if (!validated.ok) {
          return { ok: false, reason: "invalid-input", message: validated.message };
        }
        targets.push({
          path: path.split(/[\\/]/).join("/"),
          sha256: validated.sha256,
          bytes: validated.bytes,
        });
      }
      return { ok: true, targets };
    }
    const enumeration = await enumerateGDScriptFiles({
      workspaceRoot: dependencies.workspaceRoot,
      ...(signal === undefined ? {} : { signal }),
    });
    if (enumeration.truncated) {
      return {
        ok: false,
        reason: "failed",
        message:
          "The project contains more GDScript content than the immutable limits allow; project-wide diagnostics were refused.",
      };
    }
    const targets: GodotScriptCheckTarget[] = [];
    for (const entry of enumeration.targets) {
      const hashed = await hashScriptTarget({
        workspaceRoot: dependencies.workspaceRoot,
        relativePath: entry.path,
        ...(signal === undefined ? {} : { signal }),
      });
      if (hashed === null) {
        return {
          ok: false,
          reason: "failed",
          message: `The script ${entry.path} could not be verified; diagnostics were refused.`,
        };
      }
      targets.push(hashed);
    }
    return { ok: true, targets };
  }

  async function executionAvailable(): Promise<boolean> {
    return (
      (await mirrorAdapter.isAvailable()) &&
      (await checkRunner.isAvailable()) &&
      (await sandboxEnforced())
    );
  }

  async function sandboxEnforced(): Promise<boolean> {
    let status;
    try {
      status = await dependencies.backend.inspect();
    } catch {
      return false;
    }
    return (
      status.state === "available" &&
      status.capabilities.filesystemReadRestriction &&
      status.capabilities.filesystemWriteRestriction &&
      status.capabilities.networkRestriction &&
      status.capabilities.processTreeRestriction
    );
  }

  function status(): GodotDiagnosticsStatus {
    const state = invalidated ? "check-invalidated" : "untrusted";
    return {
      state,
      lastResult: null,
      lastManifestDigest: null,
      lastEngineVersion: null,
    };
  }

  function disposeAll(): void {
    preparedStore.disposeAll();
  }

  return { support, prepare, execute, status, disposeAll };
}

function requireDiagnosticCapabilities(profile: GodotEngineProfile):
  | {
      readonly ok: true;
    }
  | {
      readonly ok: false;
      readonly message: string;
    } {
  if (profile.edition === "runtime-only") {
    return {
      ok: false,
      message: "The selected executable is runtime-only; it cannot parse GDScript.",
    };
  }
  if (!profile.capabilities.checkOnly) {
    return {
      ok: false,
      message:
        "The selected Godot version does not advertise --check-only; GDScript diagnostics are unsupported and the script is never run normally.",
    };
  }
  if (
    !profile.capabilities.script ||
    !profile.capabilities.headless ||
    !profile.capabilities.projectPath
  ) {
    return {
      ok: false,
      message:
        "The selected Godot version does not advertise the required --script, --headless, and --path options; GDScript diagnostics are unsupported.",
    };
  }
  return { ok: true };
}

function checkCommandDigest(installation: GodotInstallation): string {
  return computeGodotCheckOnlyCommandDigest({
    executableSha256: installation.sha256,
    argumentTemplate: godotCheckOnlyArgumentTemplate(),
    workingDirectoryPolicy: "disposable-mirror",
    profileId: GODOT_DIAGNOSTICS_OFFLINE_PROFILE.id,
    environmentPolicy: "minimal",
    stdinPolicy: "closed",
    networkPolicy: "denied",
    timeoutMs: GODOT_LIMITS.gdscriptCheckTimeoutMs,
    stdoutLimitBytes: GODOT_LIMITS.maxCheckStreamBytes,
    stderrLimitBytes: GODOT_LIMITS.maxCheckStreamBytes,
  });
}
