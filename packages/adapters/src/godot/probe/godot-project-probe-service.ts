import { createHash } from "node:crypto";
import { lstat, open, readdir, stat } from "node:fs/promises";
import { dirname, join, sep } from "node:path";
import {
  assessGodotCompatibility,
  computeGodotPreparedProbeDigest,
  computeGodotRiskManifestDigest,
  createPreparedGodotProbe,
  GODOT_LIMITS,
  GODOT_RECOVERY_PROBE_OFFLINE_PROFILE,
  parseDeclaredVersion,
  type GodotApplicationEvent,
  type GodotDiagnostic,
  type GodotEngineProfile,
  type GodotGDExtensionRiskEntry,
  type GodotImportState,
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
  godotRecoveryArgumentTemplate,
  type GodotRecoveryRunOutcome,
} from "../process/godot-recovery-runner.js";
import { scanAuthoredFiles } from "./authored-files.js";
import { classifyRecoveryDiagnostics } from "./recovery-diagnostics.js";
import { compareWorkspaceIntegrity, snapshotWorkspaceIntegrity } from "./workspace-integrity.js";
import { createProjectMirror } from "../mirror/project-mirror.js";
import type { GitInspector } from "@solaris/core";

export const GODOT_MIRROR_COPY_POLICY_VERSION = 1;

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
  /** Disposable mirror adapter; defaults to the verified filesystem mirror. */
  readonly mirror?: ProjectMirror;
  /** Verified checkpoint storage root; mirrors must never resolve inside it. */
  readonly checkpointRoot: string | null;
  /** Read-only Git inspector; optional, integrity is never Git-only. */
  readonly git?: GitInspector;
  /** Sanitized host parent environment (never raw `process.env`). */
  readonly parentEnvironment: Readonly<Record<string, string>>;
  readonly onEvent?: (event: GodotApplicationEvent) => void;
}

/**
 * Recovery-mode Godot project probe. `prepare` refreshes the static risk
 * inventory and freezes a digest; approval binds to that digest; `execute`
 * revalidates every security-relevant input (manifest, executable, policy
 * constants) before constructing the disposable mirror, launching the
 * editor in recovery mode against the mirror only, capturing bounded
 * diagnostics, verifying the source workspace, and destroying the mirror.
 * A changed project or engine is a conflict that requires a new approval;
 * an approval is never reused and never persisted.
 */
export function createGodotProjectProbeService(
  dependencies: GodotProjectProbeServiceDependencies,
): GodotProjectProbe {
  const profiler: GodotEngineProfiler = createGodotEngineProfiler(dependencies);
  const projectMirrorAdapter: ProjectMirror = dependencies.mirror ?? createProjectMirror();
  const recoveryRunner = createGodotRecoveryRunner({
    backend: dependencies.backend,
    parentEnvironment: dependencies.parentEnvironment,
  });
  const preparedProbes = new Map<PreparedGodotProbe, PreparedProbeData>();
  let invalidated = false;
  let inFlight = false;
  let lastResult: GodotRecoveryProbeResult | null = null;
  let lastManifestDigest: string | null = null;
  let lastEngineVersion: string | null = null;

  interface PreparedProbeData {
    readonly preview: GodotProbePreview;
    readonly digest: string;
    readonly manifestDigest: string;
    readonly manifest: GodotProjectRiskManifest;
    readonly selection: {
      readonly installation: GodotInstallation;
      readonly profile: GodotEngineProfile;
    };
  }

  function emit(event: GodotApplicationEvent): void {
    dependencies.onEvent?.(event);
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
    const probe = createPreparedGodotProbe();
    preparedProbes.set(probe, {
      preview,
      digest,
      manifestDigest: manifest.digest,
      manifest,
      selection,
    });
    return { status: "ready", probe, preview, digest };
  }

  async function execute(
    probe: PreparedGodotProbe,
    context: GodotProbeExecutionContext,
  ): Promise<GodotRecoveryProbeResult> {
    const data = preparedProbes.get(probe);
    if (data === undefined) {
      return {
        ...emptyResultBase(),
        status: "failed",
        message: "The prepared probe is not valid for this session; prepare a new probe.",
      };
    }
    preparedProbes.delete(probe);
    const startedAt = Date.now();
    if (context.approvedDigest !== data.digest) {
      invalidated = true;
      return {
        ...emptyResultBase(),
        status: "conflict",
        message: "The approval does not match the prepared probe; a new approval is required.",
      };
    }
    // Revalidate the manifest and the engine before anything runs. Any
    // change is a conflict: the approval is bound to the frozen digest and
    // is never silently refreshed.
    const revalidated = await revalidatePreparedState(data, context.signal);
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
    const sandboxStatus = await sandboxEnforced();
    if (!sandboxStatus) {
      return {
        ...emptyResultBase(),
        status: "sandbox_failed",
        message: "The sandbox cannot enforce the recovery probe boundaries; the probe did not run.",
      };
    }
    inFlight = true;
    try {
      return await runProbe(data, context.signal, startedAt);
    } finally {
      inFlight = false;
    }
  }

  async function runProbe(
    data: PreparedProbeData,
    signal: AbortSignal | undefined,
    startedAt: number,
  ): Promise<GodotRecoveryProbeResult> {
    const { installation, profile } = data.selection;
    let runPaths;
    try {
      runPaths = await dependencies.runDirectories.create();
    } catch (error: unknown) {
      return {
        ...emptyResultBase(),
        status: "failed",
        message: `The private probe run directory could not be created: ${describeError(error)}`,
      };
    }
    const releaseRun = async (): Promise<string | null> => {
      const cleanup = await dependencies.runDirectories.remove(runPaths.runId);
      return cleanup.ok ? null : cleanup.message;
    };
    const destroyMirror = async (
      mirror: PreparedProjectMirrorResult | null,
    ): Promise<string | null> => {
      if (mirror === null) {
        return null;
      }
      const outcome = await projectMirrorAdapter.destroy(mirror);
      return outcome.ok ? null : outcome.message;
    };

    let mirror: PreparedProjectMirrorResult | null = null;
    try {
      // Record the source baseline before constructing the mirror.
      const baseline = await snapshotWorkspaceIntegrity({
        workspaceRoot: dependencies.workspaceRoot,
        ...(dependencies.git === undefined ? {} : { git: dependencies.git }),
        ...(signal === undefined ? {} : { signal }),
      });

      const prepared = await projectMirrorAdapter.prepare({
        workspaceRoot: dependencies.workspaceRoot,
        parentDirectory: runPaths.root,
        ...(dependencies.checkpointRoot === null
          ? {}
          : { forbiddenRoots: [dependencies.checkpointRoot] }),
        ...(signal === undefined ? {} : { signal }),
      });
      if (prepared.status !== "ready") {
        const runMessage = await releaseRun();
        return {
          ...emptyResultBase(),
          status: mapMirrorPreparationStatus(prepared.status),
          message: prepared.message,
          cleanup: {
            completed: runMessage === null,
            ...(runMessage === null ? {} : { message: runMessage }),
          },
        };
      }
      mirror = prepared.mirror;
      const verification = await projectMirrorAdapter.verify(prepared.mirror, signal);
      if (!verification.ok) {
        const cleanupMessage = await destroyMirror(mirror);
        mirror = null;
        const runMessage = await releaseRun();
        return {
          ...emptyResultBase(),
          status: "conflict",
          message: `The disposable mirror failed verification: ${verification.message}`,
          cleanup: {
            completed: cleanupMessage === null && runMessage === null,
            ...(cleanupMessage === null && runMessage === null
              ? {}
              : { message: (cleanupMessage ?? runMessage) as string }),
          },
        };
      }

      emit({ type: "godot_probe_started", installationId: installation.id, probe: "recovery" });
      const outcome = await recoveryRunner.run({
        installation,
        engineProfile: profile,
        mirrorProjectPath: prepared.mirror.projectPath,
        runPaths,
        ...(signal === undefined ? {} : { signal }),
      });
      emit({
        type: "godot_probe_completed",
        installationId: installation.id,
        probe: "recovery",
        status: outcome.status === "completed" ? "success" : "failed",
      });

      const postProbe = await inspectGeneratedState(prepared.mirror.projectPath, signal);
      const diagnostics = classifyRecoveryDiagnostics(
        outcome.status === "completed" ? outcome.result.stdout : "",
        outcome.status === "completed" ? outcome.result.stderr : "",
      );
      const workspaceComparison = compareWorkspaceIntegrity(
        baseline,
        await snapshotWorkspaceIntegrity({
          workspaceRoot: dependencies.workspaceRoot,
          ...(dependencies.git === undefined ? {} : { git: dependencies.git }),
          ...(signal === undefined ? {} : { signal }),
        }),
      );

      const cleanupMessage = await destroyMirror(mirror);
      mirror = null;
      const runMessage = await releaseRun();
      const cleanupCompleted = cleanupMessage === null && runMessage === null;

      const base = {
        engine: {
          installationId: installation.id,
          version: profile.version.raw,
          executableFingerprint: profile.fingerprint,
        },
        recoveryMode: true as const,
        mirror: {
          sourceFiles: prepared.mirror.entries.length,
          sourceBytes: prepared.mirror.copiedBytes,
          generatedGodotDirectory: postProbe.generatedGodotDirectory,
          generatedBytes: postProbe.generatedBytes,
          generatedFiles: postProbe.generatedFiles,
          importState: postProbe.importState,
        },
        diagnostics: {
          errors: diagnostics.errors,
          warnings: diagnostics.warnings,
          truncated: diagnostics.truncated || postProbe.truncated,
        },
        process: {
          exitCode: outcome.status === "completed" ? outcome.result.exitCode : null,
          durationMs: Date.now() - startedAt,
          timedOut: outcome.status === "timed-out",
        },
        workspaceIntegrity: {
          unchanged: workspaceComparison.unchanged,
          bounded: workspaceComparison.bounded,
        },
        cleanup: {
          completed: cleanupCompleted,
          ...(cleanupCompleted || cleanupMessage === null ? {} : { message: cleanupMessage }),
        },
      };

      if (!workspaceComparison.unchanged) {
        return {
          ...base,
          status: "workspace_changed",
          message:
            "The source workspace changed during the probe; Solaris does not revert external changes. The probe result is reported without claiming workspace integrity.",
        };
      }
      if (!cleanupCompleted) {
        return {
          ...base,
          status: outcome.status === "completed" ? "failed" : mapOutcomeStatus(outcome),
          message: `The disposable mirror cleanup failed: ${cleanupMessage ?? runMessage}`,
        };
      }
      switch (outcome.status) {
        case "completed": {
          if (outcome.result.exitCode !== 0) {
            return {
              ...base,
              status: "failed",
              message: `The Godot recovery probe exited with code ${String(outcome.result.exitCode)}.`,
            };
          }
          const hasDiagnostics = diagnostics.errors.length > 0 || diagnostics.warnings.length > 0;
          const status = hasDiagnostics ? "completed_with_diagnostics" : "completed";
          lastResult = { ...base, status, message: describeDiagnostics(diagnostics) };
          lastManifestDigest = data.manifestDigest;
          lastEngineVersion = profile.version.raw;
          invalidated = false;
          return lastResult;
        }
        case "timed-out":
          return { ...base, status: "timed_out", message: outcome.message };
        case "cancelled":
          return { ...base, status: "cancelled", message: outcome.message };
        case "sandbox-denied":
        case "sandbox-unavailable":
          return { ...base, status: "sandbox_failed", message: outcome.message };
        case "output-limit":
          return { ...base, status: "failed", message: outcome.message };
        case "unsupported":
          return { ...base, status: "unsupported", message: outcome.message };
        case "failed":
          return { ...base, status: "failed", message: outcome.message };
      }
    } catch (error: unknown) {
      const cleanupMessage = await destroyMirror(mirror);
      mirror = null;
      const runMessage = await releaseRun();
      const cleanupCompleted = cleanupMessage === null && runMessage === null;
      const cleanupDetail = cleanupCompleted
        ? {}
        : { message: (cleanupMessage ?? runMessage) as string };
      if (signal?.aborted || isAbortError(error)) {
        return {
          ...emptyResultBase(),
          status: "cancelled",
          message: "The project probe was cancelled.",
          cleanup: { completed: cleanupCompleted, ...cleanupDetail },
        };
      }
      return {
        ...emptyResultBase(),
        status: "failed",
        message: `The project probe failed: ${describeError(error)}`,
        cleanup: { completed: cleanupCompleted, ...cleanupDetail },
      };
    }
  }

  async function revalidatePreparedState(
    data: PreparedProbeData,
    signal: AbortSignal | undefined,
  ): Promise<{ readonly ok: true } | { readonly ok: false; readonly message: string }> {
    const identity = await revalidateExecutable(data.selection.installation);
    if (!identity.ok) {
      return { ok: false, message: identity.error };
    }
    const refresh = await refreshRiskManifest(
      data.selection.installation,
      data.selection.profile,
      signal,
    );
    if (!refresh.ok) {
      return { ok: false, message: refresh.message };
    }
    if (refresh.manifest.digest !== data.manifestDigest) {
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
    const read = await readProjectFile(dependencies.workspaceRoot, signal);
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
      workspaceRoot: dependencies.workspaceRoot,
      ...(signal === undefined ? {} : { signal }),
      enabledPlugins: scan.enabledPlugins,
      autoloadCount: scan.autoloads.length,
    });
    const language = await detectLanguageProfile({
      workspaceRoot: dependencies.workspaceRoot,
      ...(signal === undefined ? {} : { signal }),
      dotnetAssemblyName: scan.dotnetAssemblyName,
      declaredFeatures: scan.declaredFeatures,
    });
    const authored = await scanAuthoredFiles({
      workspaceRoot: dependencies.workspaceRoot,
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
          const canonicalRoot = await canonicalWorkspaceRoot();
          if (canonicalRoot !== null) {
            const verified = await verifyProjectPathContainment(
              canonicalRoot,
              join(dependencies.workspaceRoot, relativeTarget),
              DEFAULT_FS_OPS,
            );
            if (verified.ok) {
              verifiedTarget = verified.canonicalPath;
            }
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

  async function inspectGeneratedState(
    mirrorProjectPath: string,
    signal: AbortSignal | undefined,
  ): Promise<{
    readonly generatedGodotDirectory: boolean;
    readonly generatedBytes: number | null;
    readonly generatedFiles: number | null;
    readonly importState: GodotImportState;
    readonly truncated: boolean;
  }> {
    const godotDirectory = join(mirrorProjectPath, ".godot");
    const importedDirectory = join(godotDirectory, "imported");
    let metadata;
    try {
      metadata = await lstat(godotDirectory);
    } catch {
      return {
        generatedGodotDirectory: false,
        generatedBytes: null,
        generatedFiles: null,
        importState: "import state unknown",
        truncated: false,
      };
    }
    if (!metadata.isDirectory() || metadata.isSymbolicLink()) {
      return {
        generatedGodotDirectory: false,
        generatedBytes: null,
        generatedFiles: null,
        importState: "import state unknown",
        truncated: false,
      };
    }
    let importedEntries: string[] = [];
    try {
      importedEntries = await readdir(importedDirectory);
    } catch {
      importedEntries = [];
    }
    const counted = await countTree(godotDirectory, signal);
    const importState: GodotImportState =
      importedEntries.length > 0 ? "imports observed" : "project opened";
    return {
      generatedGodotDirectory: true,
      generatedBytes: counted.bytes,
      generatedFiles: counted.files,
      importState,
      truncated: counted.truncated,
    };
  }

  async function countTree(
    directory: string,
    signal: AbortSignal | undefined,
  ): Promise<{ readonly files: number; readonly bytes: number; readonly truncated: boolean }> {
    let files = 0;
    let bytes = 0;
    let truncated = false;
    const walk = async (current: string): Promise<void> => {
      if (truncated) {
        return;
      }
      if (signal?.aborted) {
        throw createAbortError();
      }
      let entries;
      try {
        entries = await readdir(current, { withFileTypes: true });
      } catch {
        return;
      }
      for (const entry of entries) {
        if (truncated) {
          return;
        }
        if (signal?.aborted) {
          throw createAbortError();
        }
        const entryPath = join(current, entry.name);
        let entryMetadata;
        try {
          entryMetadata = await lstat(entryPath);
        } catch {
          continue;
        }
        if (
          entryMetadata.isSymbolicLink() ||
          (!entryMetadata.isDirectory() && !entryMetadata.isFile())
        ) {
          continue;
        }
        if (entryMetadata.isDirectory()) {
          await walk(entryPath);
          continue;
        }
        files += 1;
        bytes += entryMetadata.size;
        if (
          files >= GODOT_LIMITS.maxGeneratedGodotFiles ||
          bytes >= GODOT_LIMITS.maxGeneratedGodotBytes
        ) {
          truncated = true;
          return;
        }
      }
    };
    await walk(directory);
    return { files, bytes, truncated };
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
      join(dependencies.workspaceRoot, relativePath),
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
    let metadata;
    try {
      metadata = await stat(absolutePath);
    } catch {
      return null;
    }
    if (!metadata.isFile()) {
      return null;
    }
    if (metadata.size > GODOT_LIMITS.maxMirrorSingleFileBytes) {
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
      return {
        path: absolutePath.split(sep).join("/"),
        sha256: hash.digest("hex"),
        bytes: metadata.size,
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

  async function sandboxEnforced(): Promise<boolean> {
    let status;
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

  function status(): GodotProjectProbeStatus {
    const state = inFlight
      ? invalidated
        ? "probe-invalidated"
        : "probe-approved"
      : invalidated
        ? "probe-invalidated"
        : "untrusted";
    return {
      state,
      lastResult: lastResult === null ? null : { ...lastResult },
      lastManifestDigest,
      lastEngineVersion,
    };
  }

  return { prepare, execute, status };
}

type PreparedProjectMirrorResult = import("@solaris/core").PreparedProjectMirror;

function mapMirrorPreparationStatus(
  status: "ready" | "conflict" | "mirror_unsupported" | "too_large" | "failed",
): GodotRecoveryProbeResult["status"] {
  switch (status) {
    case "conflict":
      return "conflict";
    case "mirror_unsupported":
      return "unsupported";
    case "too_large":
      return "mirror_too_large";
    case "failed":
      return "failed";
    case "ready":
      return "failed";
  }
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

function mapOutcomeStatus(outcome: GodotRecoveryRunOutcome): GodotRecoveryProbeResult["status"] {
  switch (outcome.status) {
    case "completed":
      return "completed";
    case "timed-out":
      return "timed_out";
    case "cancelled":
      return "cancelled";
    case "sandbox-denied":
    case "sandbox-unavailable":
      return "sandbox_failed";
    case "output-limit":
    case "failed":
      return "failed";
    case "unsupported":
      return "unsupported";
  }
}

function describeDiagnostics(diagnostics: {
  readonly errors: readonly GodotDiagnostic[];
  readonly warnings: readonly GodotDiagnostic[];
}): string {
  const parts: string[] = [];
  if (diagnostics.errors.length > 0) {
    parts.push(`${diagnostics.errors.length} error${diagnostics.errors.length === 1 ? "" : "s"}`);
  }
  if (diagnostics.warnings.length > 0) {
    parts.push(
      `${diagnostics.warnings.length} warning${diagnostics.warnings.length === 1 ? "" : "s"}`,
    );
  }
  if (parts.length === 0) {
    return "The recovery-mode probe completed with no diagnostics.";
  }
  return `The recovery-mode probe completed with ${parts.join(" and ")}.`;
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

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "An unknown project probe failure occurred.";
}

function createAbortError(): Error {
  return new DOMException("The Godot project probe was aborted.", "AbortError");
}
