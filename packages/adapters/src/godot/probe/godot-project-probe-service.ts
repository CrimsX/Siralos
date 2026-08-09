import { createHash } from "node:crypto";
import { lstat, open } from "node:fs/promises";
import { dirname, join, sep } from "node:path";
import { samePathIdentity } from "../../fs-path-identity.js";
import {
  assessGodotCompatibility,
  computeGodotPreparedProbeDigest,
  computeGodotRiskManifestDigest,
  GODOT_LIMITS,
  GODOT_RECOVERY_PROBE_OFFLINE_PROFILE,
  parseDeclaredVersion,
  type GodotApplicationEvent,
  type GodotEngineProfile,
  type GodotGDExtensionRiskEntry,
  type GodotInstallation,
  type GodotLibraryRiskEntry,
  type GodotPluginRiskEntry,
  type GodotProbeExecutionContext,
  type GodotProbePreparationResult,
  type GodotProbePreview,
  type GodotProjectProbe,
  type GodotProjectProbeStatus,
  type GodotProjectRiskManifest,
  type GodotProbeRunner,
  type GodotRecoveryProbeResult,
  type GodotRecoveryProbeSupport,
  type GodotSelectionPreference,
  type PreparedGodotProbe,
  type ProjectMirror,
  type SafeDiagnostic,
  type SandboxBackend,
} from "@solaris/core";
import type { RunDirectoryProvider } from "../../process/run-directories.js";
import type { GodotEngineProfileCache } from "../cache/engine-profile-cache.js";
import type { UserGodotConfig } from "../../config/user-config.js";
import { createGodotEngineProfiler, type GodotEngineProfiler } from "../profile/engine-profiler.js";
import { readProjectFile } from "../project/project-files.js";
import { scanProjectFile } from "../project/project-scanner.js";
import { inventoryExecutableContent } from "../project/content-inventory.js";
import { detectLanguageProfile } from "../project/language-profile.js";
import {
  DEFAULT_FS_OPS,
  validateProjectRelativePath,
  verifyProjectPathContainment,
} from "../project/traversal-limits.js";
import {
  computeGodotRecoveryCommandDigest,
  createGodotRecoveryRunner,
  GODOT_RECOVERY_RUN_UNAVAILABLE_MESSAGE,
  godotRecoveryArgumentTemplate,
  type GodotRecoveryRunner,
} from "../process/godot-recovery-runner.js";
import {
  createProjectMirror,
  PROJECT_MIRROR_UNAVAILABLE_MESSAGE,
} from "../mirror/project-mirror.js";
import { scanAuthoredFiles } from "./authored-files.js";
import {
  createPreparedProbeStore,
  type PreparedProbePlan,
  type PreparedProbeStoreConfig,
} from "./prepared-probe-store.js";
import type { GitInspector } from "@solaris/core";

export const GODOT_MIRROR_COPY_POLICY_VERSION = 1;

export const GODOT_RECOVERY_EXECUTION_UNAVAILABLE_MESSAGE =
  "Recovery-mode project probing is unavailable on this platform: the exact approved Godot identity cannot be launched, the disposable mirror cannot be constructed with exactly the approved bytes, and its cleanup cannot be bound to the exact created objects, because Node and the pinned sandbox runtime offer no exec-by-handle, directory-relative create, or delete-by-handle primitive. Nothing was created, nothing was deleted, and no engine was launched.";

export interface GodotProjectProbeServiceDependencies {
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
  /** Recovery runner; production wires the fail-closed runner. */
  readonly recoveryRunner?: GodotRecoveryRunner;
  /** Verified checkpoint storage root; mirrors must never resolve inside it. */
  readonly checkpointRoot: string | null;
  /** Read-only Git inspector; optional, integrity is never Git-only. */
  readonly git?: GitInspector;
  /** Sanitized host parent environment (never raw `process.env`). */
  readonly parentEnvironment: Readonly<Record<string, string>>;
  readonly onEvent?: (event: GodotApplicationEvent) => void;
  /** Prepared-probe store configuration; tests inject bounds and clocks. */
  readonly preparedStoreConfig?: PreparedProbeStoreConfig;
}

/**
 * Recovery-mode Godot project probe. `prepare` refreshes the static risk
 * inventory and freezes a digest; approval binds to that digest; `execute`
 * revalidates every security-relevant input (manifest, executable, policy
 * constants) and then refuses with a typed `unavailable` outcome unless the
 * platform can mechanically bind execution to the approved bytes — the
 * disposable mirror, the engine launch, and the cleanup. A changed project
 * or engine is a conflict that requires a new approval; an approval is
 * never reused, never persisted, and every prepared probe is single-use,
 * expiring, and disposable.
 */
export function createGodotProjectProbeService(
  dependencies: GodotProjectProbeServiceDependencies,
): GodotProjectProbe {
  const profiler: GodotEngineProfiler = createGodotEngineProfiler(dependencies);
  const projectMirrorAdapter: ProjectMirror = dependencies.mirror ?? createProjectMirror();
  const recoveryRunner: GodotRecoveryRunner =
    dependencies.recoveryRunner ?? createGodotRecoveryRunner({ backend: dependencies.backend });
  const preparedStore = createPreparedProbeStore(dependencies.preparedStoreConfig);
  let invalidated = false;

  async function support(): Promise<GodotRecoveryProbeSupport> {
    const available =
      (await projectMirrorAdapter.isAvailable()) &&
      (await recoveryRunner.isAvailable()) &&
      (await sandboxEnforced());
    return {
      state: available ? "available" : "unavailable",
      reason: available ? null : GODOT_RECOVERY_EXECUTION_UNAVAILABLE_MESSAGE,
      platform: dependencies.platform,
    };
  }

  async function prepare(signal?: AbortSignal): Promise<GodotProbePreparationResult> {
    if (signal?.aborted) {
      throw createAbortError();
    }
    const selection = await profiler.selectedProfile(signal);
    if (selection === null) {
      return {
        status: "unsupported",
        message: "No trusted Godot editor is selected; the project probe cannot run.",
      };
    }
    const capability = requireRecoveryCapabilities(selection.profile);
    if (!capability.ok) {
      return { status: "unsupported", message: capability.message };
    }
    const refresh = await refreshRiskManifest(selection.installation, selection.profile, signal);
    if (!refresh.ok) {
      return { status: "failed", message: refresh.message };
    }
    const { manifest, projectProfile } = refresh;
    const compatibility = assessGodotCompatibility(selection.profile, projectProfile);
    const preview: GodotProbePreview = {
      projectName: projectProfile.name,
      engineVersion: selection.profile.version.raw,
      installationId: selection.installation.id,
      engineEdition: selection.profile.edition,
      support: selection.profile.support,
      compatibility: compatibility.status,
      risks: {
        toolScripts: manifest.toolScripts.length,
        enabledEditorPlugins: manifest.enabledEditorPlugins.length,
        gdextensions: manifest.gdextensionDescriptors.length,
        autoloads: manifest.autoloads.length,
        dotnetProjects: manifest.dotnetProjects.length,
      },
      mirror: {
        estimatedFileCount: manifest.authoredFileManifest.fileCount,
        estimatedBytes: manifest.authoredFileManifest.totalBytes,
      },
      isolation: {
        sourceWorkspace: "not-used-as-project",
        disposableMirror: true,
        recoveryMode: true,
        headless: true,
        network: "denied",
        environment: "minimal",
        stdin: "closed",
      },
      manifestDigest: manifest.digest,
    };
    const digest = computeGodotPreparedProbeDigest({
      manifestDigest: manifest.digest,
      commandDigest: computeGodotRecoveryCommandDigest({
        executableSha256: selection.installation.sha256,
        argumentTemplate: godotRecoveryArgumentTemplate(),
        workingDirectoryPolicy: "disposable-mirror",
        profileId: GODOT_RECOVERY_PROBE_OFFLINE_PROFILE.id,
        environmentPolicy: "minimal",
        stdinPolicy: "closed",
        networkPolicy: "denied",
        timeoutMs: GODOT_LIMITS.recoveryProbeTimeoutMs,
        stdoutLimitBytes: GODOT_LIMITS.maxRecoveryStreamBytes,
        stderrLimitBytes: GODOT_LIMITS.maxRecoveryStreamBytes,
      }),
      mirrorPolicyVersion: GODOT_MIRROR_COPY_POLICY_VERSION,
      sandboxProfileId: GODOT_RECOVERY_PROBE_OFFLINE_PROFILE.id,
      probeLimits: {
        timeoutMs: GODOT_LIMITS.recoveryProbeTimeoutMs,
        maxFiles: GODOT_LIMITS.maxMirrorFiles,
        maxBytes: GODOT_LIMITS.maxMirrorBytes,
        maxSingleFileBytes: GODOT_LIMITS.maxMirrorSingleFileBytes,
        maxDepth: GODOT_LIMITS.maxMirrorDepth,
        maxRelativePathBytes: GODOT_LIMITS.maxMirrorRelativePathBytes,
      },
    });
    const stored = preparedStore.put({
      preview,
      digest,
      manifestDigest: manifest.digest,
      manifest,
      selection,
    });
    if (!stored.ok) {
      return { status: "failed", message: stored.message };
    }
    return { status: "ready", probe: stored.probe, preview, digest };
  }

  async function execute(
    probe: PreparedGodotProbe,
    context: GodotProbeExecutionContext,
  ): Promise<GodotRecoveryProbeResult> {
    const plan = preparedStore.consume(probe);
    if (plan === null) {
      return {
        ...emptyResultBase(),
        status: "failed",
        message: "The prepared probe is not valid for this session; prepare a new probe.",
      };
    }
    if (context.approvedDigest !== plan.digest) {
      invalidated = true;
      return {
        ...emptyResultBase(),
        status: "conflict",
        message: "The approval does not match the prepared probe; a new approval is required.",
      };
    }
    const revalidated = await revalidatePreparedState(plan, context.signal);
    if (!revalidated.ok) {
      invalidated = true;
      return {
        ...emptyResultBase(),
        status: "conflict",
        message: revalidated.message,
      };
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
        ...resultBaseFor(plan),
        status: "unavailable",
        message: GODOT_RECOVERY_EXECUTION_UNAVAILABLE_MESSAGE,
        cleanup: { completed: true },
      };
    }
    return {
      ...resultBaseFor(plan),
      status: "unavailable",
      message: GODOT_RECOVERY_EXECUTION_UNAVAILABLE_MESSAGE,
      cleanup: { completed: true },
    };
  }

  async function revalidatePreparedState(
    plan: PreparedProbePlan,
    signal: AbortSignal | undefined,
  ): Promise<{ readonly ok: true } | { readonly ok: false; readonly message: string }> {
    const identity = await revalidateExecutable(plan.selection.installation);
    if (!identity.ok) {
      return { ok: false, message: identity.error };
    }
    const refresh = await refreshRiskManifest(
      plan.selection.installation,
      plan.selection.profile,
      signal,
    );
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

  async function revalidateExecutable(
    installation: GodotInstallation,
  ): Promise<{ readonly ok: true } | { readonly ok: false; readonly error: string }> {
    if (installation.status !== "valid") {
      return { ok: false, error: "The installation is invalid; rediscovery is required." };
    }
    const { revalidateExecutableIdentity } = await import("../discovery/executable-validation.js");
    const result = await revalidateExecutableIdentity({
      canonicalPath: installation.canonicalPath,
      sizeBytes: installation.sizeBytes,
      modifiedAtMs: installation.modifiedAtMs,
      sha256: installation.sha256,
    });
    return result.unchanged
      ? { ok: true }
      : {
          ok: false,
          error:
            "The selected Godot executable changed after approval; a new approval is required.",
        };
  }

  async function refreshRiskManifest(
    installation: GodotInstallation,
    profile: GodotEngineProfile,
    signal: AbortSignal | undefined,
  ): Promise<
    | {
        readonly ok: true;
        readonly manifest: GodotProjectRiskManifest;
        readonly projectProfile: import("@solaris/core").GodotProjectProfile;
      }
    | { readonly ok: false; readonly message: string }
  > {
    const canonicalRoot = await canonicalWorkspaceRoot();
    if (canonicalRoot === null) {
      return { ok: false, message: "The workspace root could not be resolved." };
    }
    const read = await readProjectFile(canonicalRoot, signal);
    if (!read.ok) {
      return {
        ok: false,
        message:
          read.reason === "missing"
            ? "No project.godot exists at the workspace root; the project probe cannot run."
            : read.message,
      };
    }
    const scan = scanProjectFile(read.content);
    const content = await inventoryExecutableContent({
      workspaceRoot: canonicalRoot,
      ...(signal === undefined ? {} : { signal }),
      enabledPlugins: scan.enabledPlugins,
      autoloadCount: scan.autoloads.length,
    });
    const language = await detectLanguageProfile({
      workspaceRoot: canonicalRoot,
      ...(signal === undefined ? {} : { signal }),
      dotnetAssemblyName: scan.dotnetAssemblyName,
      declaredFeatures: scan.declaredFeatures,
    });
    const authored = await scanAuthoredFiles({
      workspaceRoot: canonicalRoot,
      ...(signal === undefined ? {} : { signal }),
    });
    const toolScripts = [];
    for (const relativePath of content.inventory.toolScripts) {
      const hashed = await hashWorkspaceFile(relativePath, signal);
      if (hashed !== null) {
        toolScripts.push(hashed);
      }
    }
    const enabledEditorPlugins: GodotPluginRiskEntry[] = [];
    for (const plugin of content.inventory.editorPlugins) {
      if (!plugin.enabled) {
        continue;
      }
      const hashed = await hashWorkspaceFile(
        join(plugin.path, plugin.scriptPath.replace(/^res:\/\//, ""))
          .split(sep)
          .join("/"),
        signal,
      );
      if (hashed !== null) {
        enabledEditorPlugins.push({
          path: plugin.path,
          name: plugin.name,
          enabled: true,
          sha256: hashed.sha256,
          bytes: hashed.bytes,
        });
      }
    }
    const gdextensionDescriptors: GodotGDExtensionRiskEntry[] = [];
    for (const descriptor of content.inventory.gdextensionDescriptors) {
      const hashed = await hashWorkspaceFile(descriptor.path, signal);
      if (hashed === null) {
        continue;
      }
      const referencedLibraries: GodotLibraryRiskEntry[] = [];
      for (const target of descriptor.libraryTargets) {
        // Library targets are project-controlled: the absolute target must
        // pass lexical + canonical containment before any filesystem call.
        // An escaping target is reported as an unverified library (null
        // hash) and never read.
        const relativeTarget = target.startsWith("res://")
          ? target.slice("res://".length)
          : join(dirname(descriptor.path), target.replace(/^\.\//, "")).split(sep).join("/");
        let verifiedTarget: string | null = null;
        if (
          validateProjectRelativePath(relativeTarget, GODOT_LIMITS.maxResReferencePathBytes).ok ===
          true
        ) {
          const verified = await verifyProjectPathContainment(
            canonicalRoot,
            join(canonicalRoot, relativeTarget),
            DEFAULT_FS_OPS,
          );
          if (verified.ok) {
            verifiedTarget = verified.canonicalPath;
          }
        }
        const library =
          verifiedTarget === null ? null : await hashAbsoluteFile(verifiedTarget, signal);
        referencedLibraries.push(
          library === null
            ? { path: target, sha256: null, bytes: null }
            : { path: target, sha256: library.sha256, bytes: library.bytes },
        );
      }
      gdextensionDescriptors.push({
        path: descriptor.path,
        sha256: hashed.sha256,
        bytes: hashed.bytes,
        referencedLibraries,
      });
    }
    const warnings: SafeDiagnostic[] = [
      ...content.warnings,
      ...language.evidence.map((message) => ({ severity: "info" as const, message })),
    ];
    if (authored.truncated) {
      warnings.push({
        severity: "warning",
        message:
          "The authored-file baseline was truncated by its bounds; integrity coverage is bounded.",
      });
    }
    const manifestWithoutDigest: Omit<GodotProjectRiskManifest, "digest"> = {
      projectFileSha256: read.sha256,
      engineSelection: {
        installationId: installation.id,
        executableSha256: installation.sha256,
        version: profile.version.raw,
      },
      toolScripts,
      enabledEditorPlugins,
      gdextensionDescriptors,
      autoloads: scan.autoloads.map((autoload) => ({
        name: autoload.name,
        target: autoload.target,
      })),
      dotnetProjects: content.inventory.dotnetProjectFiles,
      authoredFileManifest: {
        fileCount: authored.fileCount,
        totalBytes: authored.totalBytes,
        digest: authored.digest,
        truncated: authored.truncated,
      },
      scanWarnings: warnings,
    };
    const manifest: GodotProjectRiskManifest = {
      ...manifestWithoutDigest,
      digest: computeGodotRiskManifestDigest(manifestWithoutDigest),
    };
    const declaredEngineVersion = extractDeclaredEngineVersion(scan.declaredFeatures);
    const projectProfile: import("@solaris/core").GodotProjectProfile = {
      detected: true,
      projectFileSha256: read.sha256,
      configVersion: scan.configVersion,
      name: scan.name,
      applicationVersion: scan.applicationVersion,
      declaredFeatures: scan.declaredFeatures,
      declaredEngineVersion,
      mainScene: scan.mainScene,
      mainSceneExists: null,
      mainSceneIsSymlink: false,
      renderingMethods: scan.renderingMethods,
      languageProfile: language.profile,
      autoloads: scan.autoloads,
      enabledEditorPlugins: scan.enabledPlugins,
      executableContent: content.inventory,
      warnings,
    };
    return { ok: true, manifest, projectProfile };
  }

  async function hashWorkspaceFile(
    relativePath: string,
    signal: AbortSignal | undefined,
  ): Promise<{ readonly path: string; readonly sha256: string; readonly bytes: number } | null> {
    if (
      validateProjectRelativePath(relativePath, GODOT_LIMITS.maxResReferencePathBytes).ok !== true
    ) {
      return null;
    }
    const canonicalRoot = await canonicalWorkspaceRoot();
    if (canonicalRoot === null) {
      return null;
    }
    const verified = await verifyProjectPathContainment(
      canonicalRoot,
      join(canonicalRoot, relativePath),
      DEFAULT_FS_OPS,
    );
    if (!verified.ok) {
      return null;
    }
    return hashAbsoluteFile(verified.canonicalPath, signal);
  }

  let canonicalRootCache: string | null | undefined;

  async function canonicalWorkspaceRoot(): Promise<string | null> {
    if (canonicalRootCache !== undefined) {
      return canonicalRootCache;
    }
    try {
      const { realpath } = await import("node:fs/promises");
      const canonical = await realpath(dependencies.workspaceRoot);
      canonicalRootCache = canonical;
      return canonical;
    } catch {
      canonicalRootCache = null;
      return null;
    }
  }

  async function hashAbsoluteFile(
    absolutePath: string,
    signal: AbortSignal | undefined,
  ): Promise<{ readonly path: string; readonly sha256: string; readonly bytes: number } | null> {
    // The path identity anchor is the containment-verified canonical path.
    // The leaf itself is lstat'd without following: a symlink/junction leaf
    // (even one planted inside the workspace) must never dereference to an
    // outside object, so its content is never read or hashed.
    let leafMetadata;
    try {
      leafMetadata = await lstat(absolutePath);
    } catch {
      return null;
    }
    if (leafMetadata.isSymbolicLink() || !leafMetadata.isFile()) {
      return null;
    }
    if (leafMetadata.size > GODOT_LIMITS.maxMirrorSingleFileBytes) {
      return null;
    }
    let handle;
    try {
      handle = await open(absolutePath, "r");
    } catch {
      return null;
    }
    try {
      const hash = createHash("sha256");
      const buffer = Buffer.alloc(1024 * 1024);
      for (;;) {
        if (signal?.aborted) {
          throw createAbortError();
        }
        const { bytesRead } = await handle.read(buffer, 0, buffer.length, null);
        if (bytesRead === 0) {
          break;
        }
        hash.update(buffer.subarray(0, bytesRead));
      }
      // The path must still resolve to the same object after the read; a
      // swap during inspection discards the result rather than trusting it.
      const after = await realpathOf(absolutePath);
      if (after === null || !samePathIdentity(after, absolutePath)) {
        return null;
      }
      return {
        path: absolutePath.split(sep).join("/"),
        sha256: hash.digest("hex"),
        bytes: leafMetadata.size,
      };
    } catch (error: unknown) {
      if (isAbortError(error)) {
        throw error;
      }
      return null;
    } finally {
      await handle.close().catch(() => undefined);
    }
  }

  async function realpathOf(path: string): Promise<string | null> {
    try {
      const { realpath } = await import("node:fs/promises");
      return await realpath(path);
    } catch {
      return null;
    }
  }

  async function executionAvailable(): Promise<boolean> {
    return (
      (await projectMirrorAdapter.isAvailable()) &&
      (await recoveryRunner.isAvailable()) &&
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

  function status(): GodotProjectProbeStatus {
    // Execution is unavailable at this stage, so no probe has ever
    // completed and there is no last result to report; the state machine
    // still distinguishes untrusted from invalidated approvals.
    const state = invalidated ? "probe-invalidated" : "untrusted";
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

function requireRecoveryCapabilities(profile: GodotEngineProfile):
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
      message: "The selected executable is runtime-only; it cannot run the editor recovery probe.",
    };
  }
  if (!profile.capabilities.recoveryMode) {
    return {
      ok: false,
      message:
        "The selected Godot version does not advertise --recovery-mode; the recovery probe is unsupported and no weaker mode is used.",
    };
  }
  if (
    !profile.capabilities.editor ||
    !profile.capabilities.headless ||
    !profile.capabilities.projectPath
  ) {
    return {
      ok: false,
      message:
        "The selected Godot version does not advertise the required --editor, --headless, and --path options; the recovery probe is unsupported.",
    };
  }
  return { ok: true };
}

function resultBaseFor(
  plan: PreparedProbePlan,
): Omit<GodotRecoveryProbeResult, "status" | "message" | "cleanup"> {
  return {
    engine: {
      installationId: plan.selection.installation.id,
      version: plan.selection.profile.version.raw,
      executableFingerprint: plan.selection.profile.fingerprint,
    },
    recoveryMode: true,
    mirror: {
      sourceFiles: 0,
      sourceBytes: 0,
      generatedGodotDirectory: false,
      generatedBytes: null,
      generatedFiles: null,
      importState: "import state unknown",
    },
    diagnostics: { errors: [], warnings: [], truncated: false },
    process: { exitCode: null, durationMs: 0, timedOut: false },
    workspaceIntegrity: { unchanged: true, bounded: false },
  };
}

function emptyResultBase(): Omit<GodotRecoveryProbeResult, "status" | "message"> {
  return {
    engine: { installationId: "", version: "", executableFingerprint: "" },
    recoveryMode: true,
    mirror: {
      sourceFiles: 0,
      sourceBytes: 0,
      generatedGodotDirectory: false,
      generatedBytes: null,
      generatedFiles: null,
      importState: "import state unknown",
    },
    diagnostics: { errors: [], warnings: [], truncated: false },
    process: { exitCode: null, durationMs: 0, timedOut: false },
    workspaceIntegrity: { unchanged: true, bounded: false },
    cleanup: { completed: false },
  };
}

function extractDeclaredEngineVersion(features: readonly string[]): {
  major: number;
  minor: number;
  patch: number | null;
  raw: string;
} | null {
  for (const feature of features) {
    const parsed = parseDeclaredVersion(feature);
    if (parsed !== null) {
      return parsed;
    }
  }
  return null;
}

function isAbortError(error: unknown): boolean {
  return (
    error instanceof Error &&
    (error.name === "AbortError" || ("code" in error && error.code === "ABORT_ERR"))
  );
}

function createAbortError(): Error {
  return new DOMException("The Godot project probe was aborted.", "AbortError");
}

export { GODOT_RECOVERY_RUN_UNAVAILABLE_MESSAGE, PROJECT_MIRROR_UNAVAILABLE_MESSAGE };
