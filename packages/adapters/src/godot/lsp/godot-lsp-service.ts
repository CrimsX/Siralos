import {
  assessGodotCompatibility,
  computeGDScriptPreparedSessionDigest,
  GODOT_LIMITS,
  GODOT_LSP_LOCAL_PROFILE,
  type GDScriptLanguageService,
  type GDScriptLanguageSupport,
  type GDScriptLSPSessionPreview,
  type GDScriptSessionPreparationResult,
  type GDScriptSessionStartContext,
  type GDScriptSessionStartResult,
  type GDScriptSessionStatus,
  type GodotApplicationEvent,
  type GodotEngineProfile,
  type GodotProbeRunner,
  type GodotSelectionPreference,
  type LanguageSessionEvent,
  type PreparedGDScriptSession,
  type ProjectMirror,
  type SandboxBackend,
} from "@solaris/core";
import type { RunDirectoryProvider } from "../../process/run-directories.js";
import type { GodotEngineProfileCache } from "../cache/engine-profile-cache.js";
import type { UserGodotConfig } from "../../config/user-config.js";
import { createGodotEngineProfiler, type GodotEngineProfiler } from "../profile/engine-profiler.js";
import { createProjectMirror } from "../mirror/project-mirror.js";
import {
  createGodotLSPServerRunner,
  type GodotLSPServerRunner,
} from "../process/godot-lsp-runner.js";
import { createAbortError, refreshGodotProjectRiskManifest } from "../probe/risk-manifest.js";
import { enumerateGDScriptFiles } from "../diagnostics/script-enumeration.js";
import {
  createPreparedLSPSessionStore,
  type PreparedLSPSessionPlan,
} from "./prepared-session-store.js";

export const GODOT_LSP_EXECUTION_UNAVAILABLE_MESSAGE =
  "The Godot GDScript language session is unavailable on this platform: the exact approved Godot editor cannot be launched against exactly the approved mirrored project bytes, the disposable mirror cannot be constructed or cleaned up identity-bound, and the loopback LSP channel cannot be tied to a verified process identity, because Node and the pinned sandbox runtime offer no exec-by-handle, directory-relative create, or delete-by-handle primitive. Nothing was created, no port was opened, no engine was launched.";

export interface GDScriptLanguageServiceDependencies {
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
  /** Godot LSP server runner; production wires the fail-closed runner. */
  readonly lspRunner?: GodotLSPServerRunner;
  /** Verified checkpoint storage root; mirrors must never resolve inside it. */
  readonly checkpointRoot: string | null;
  /** Sanitized host parent environment (never raw `process.env`). */
  readonly parentEnvironment: Readonly<Record<string, string>>;
  readonly onEvent?: (event: GodotApplicationEvent | LanguageSessionEvent) => void;
  /** Prepared-session store configuration; tests inject bounds and clocks. */
  readonly preparedSessionStoreConfig?: {
    readonly maxSessions?: number;
    readonly maxStateBytes?: number;
    readonly ttlMs?: number;
    readonly now?: () => number;
  };
}

/**
 * Provider-neutral GDScript language service.
 *
 * `prepare` refreshes the static risk inventory, enumerates the bounded
 * GDScript inputs, and freezes the immutable session plan; approval binds
 * to that digest; `start` revalidates every security-relevant input
 * (executable identity, risk manifest, policy constants) and then refuses
 * with a typed `unavailable` outcome unless the platform can mechanically
 * bind the Godot launch, the disposable mirror, and the cleanup to the
 * approved bytes. At this stage every session start fails closed before a
 * mirror is created, a port is opened, or an editor is launched; the live
 * isolation probe is reported skipped, never passed.
 */
export function createGDScriptLanguageService(
  dependencies: GDScriptLanguageServiceDependencies,
): GDScriptLanguageService {
  const profiler: GodotEngineProfiler = createGodotEngineProfiler(dependencies);
  const mirrorAdapter: ProjectMirror = dependencies.mirror ?? createProjectMirror();
  const lspRunner: GodotLSPServerRunner =
    dependencies.lspRunner ?? createGodotLSPServerRunner({ backend: dependencies.backend });
  const preparedStore = createPreparedLSPSessionStore(dependencies.preparedSessionStoreConfig);
  let invalidated = false;

  async function support(): Promise<GDScriptLanguageSupport> {
    const available =
      (await mirrorAdapter.isAvailable()) &&
      (await lspRunner.isAvailable()) &&
      (await sandboxEnforced());
    return {
      state: available ? "available" : "unavailable",
      reason: available ? null : GODOT_LSP_EXECUTION_UNAVAILABLE_MESSAGE,
      platform: dependencies.platform,
    };
  }

  async function prepare(signal?: AbortSignal): Promise<GDScriptSessionPreparationResult> {
    if (signal?.aborted) {
      throw createAbortError();
    }
    const selection = await profiler.selectedProfile(signal);
    if (selection === null) {
      return {
        status: "unsupported",
        message: "No trusted Godot installation is selected; the language session cannot start.",
      };
    }
    const capability = requireLSPSessionCapabilities(selection.profile);
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
    const enumeration = await enumerateGDScriptFiles({
      workspaceRoot: dependencies.workspaceRoot,
      ...(signal === undefined ? {} : { signal }),
    });
    const compatibility = assessGodotCompatibility(selection.profile, projectProfile);
    const preview: GDScriptLSPSessionPreview = {
      projectName: projectProfile.name,
      engineVersion: selection.profile.version.raw,
      installationId: selection.installation.id,
      engineEdition: selection.profile.edition,
      support: selection.profile.support,
      compatibility: compatibility.status,
      projectIntelligence: {
        gdscriptFiles: enumeration.targets.length,
        toolScripts: manifest.toolScripts.length,
        editorPlugins: manifest.enabledEditorPlugins.length,
        gdextensions: manifest.gdextensionDescriptors.length,
      },
      session: {
        sourceProject: "disposable mirror",
        godotMode: "headless recovery editor",
        lspNetwork: "loopback only",
        externalNetwork: "denied",
        sourceWrites: "denied",
        providerSecrets: "removed",
        lspMutations: "disabled",
      },
      capabilities: { diagnostics: true, hover: true, completion: true, definition: true },
      manifestDigest: manifest.digest,
    };
    const digest = computeGDScriptPreparedSessionDigest({
      manifestDigest: manifest.digest,
      executableSha256: selection.installation.sha256,
      engineVersion: selection.profile.version.raw,
      mirrorPolicyVersion: 1,
      capabilities: preview.capabilities,
      sandboxProfileId: GODOT_LSP_LOCAL_PROFILE.id,
      lspPolicyVersion: GODOT_LIMITS.lspPolicyVersion,
      sessionLimits: {
        startupTimeoutMs: GODOT_LIMITS.lspStartupTimeoutMs,
        idleTimeoutMs: GODOT_LIMITS.lspIdleTimeoutMs,
        maxLifetimeMs: GODOT_LIMITS.lspMaxSessionLifetimeMs,
        requestTimeoutMs: GODOT_LIMITS.lspRequestTimeoutMs,
        shutdownTimeoutMs: GODOT_LIMITS.lspShutdownTimeoutMs,
      },
    });
    const stored = preparedStore.put({
      preview,
      digest,
      manifestDigest: manifest.digest,
      selection,
    });
    if (!stored.ok) {
      return { status: "failed", message: stored.message };
    }
    return { status: "ready", session: stored.session, preview, digest };
  }

  async function start(
    session: PreparedGDScriptSession,
    context: GDScriptSessionStartContext,
  ): Promise<GDScriptSessionStartResult> {
    const plan = preparedStore.consume(session);
    if (plan === null) {
      return {
        status: "failed",
        message:
          "The prepared language session is not valid for this session; prepare a new session.",
      };
    }
    if (context.approvedDigest !== plan.digest) {
      invalidated = true;
      return {
        status: "conflict",
        message:
          "The approval does not match the prepared language session; a new approval is required.",
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
    // The execution gate: the mirror, the LSP runner, and the sandbox must
    // all be able to enforce their invariants before anything runs. On this
    // stage every component is fail-closed, so this refusal happens before
    // a mirror is created, a port is opened, or an editor is launched.
    return {
      status: "unavailable",
      message: GODOT_LSP_EXECUTION_UNAVAILABLE_MESSAGE,
    };
  }

  async function revalidatePreparedState(
    plan: PreparedLSPSessionPlan,
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
    return { ok: true };
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

  function status(): GDScriptSessionStatus {
    return {
      state: invalidated ? "stale" : "unavailable",
      sessionId: null,
      engineVersion: null,
      projectName: null,
      startedAtMs: null,
      idleMs: null,
      capabilities: { diagnostics: false, hover: false, completion: false, definition: false },
      openDocumentCount: 0,
      diagnosticCount: 0,
      networkIsolation: "unavailable",
    };
  }

  function closeAll(): Promise<void> {
    preparedStore.disposeAll();
    return Promise.resolve();
  }

  return { support, prepare, start, status, closeAll };
}

function requireLSPSessionCapabilities(profile: GodotEngineProfile):
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
      message:
        "The selected executable is runtime-only; it cannot host a GDScript language server.",
    };
  }
  if (!profile.capabilities.lsp) {
    return {
      ok: false,
      message:
        "The selected Godot version does not advertise --lsp-port; the language session is unsupported.",
    };
  }
  if (
    !profile.capabilities.recoveryMode ||
    !profile.capabilities.editor ||
    !profile.capabilities.headless ||
    !profile.capabilities.projectPath
  ) {
    return {
      ok: false,
      message:
        "The selected Godot version does not advertise the required --recovery-mode, --editor, --headless, and --path options; the language session is unsupported.",
    };
  }
  return { ok: true };
}
