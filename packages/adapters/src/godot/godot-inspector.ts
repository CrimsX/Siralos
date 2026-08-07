import {
  assessGodotCompatibility,
  type GodotApplicationEvent,
  type GodotCompatibilityAssessment,
  type GodotDiscoveryResult,
  type GodotDoctorReport,
  type GodotEngineProfile,
  type GodotInspector,
  type GodotProjectProfile,
  type GodotSelectedInstallation,
  type GodotSelectionPreference,
  type SandboxBackend,
} from "@solaris/core";
import type { UserGodotConfig } from "../config/user-config.js";
import type { GodotEngineProfileCache } from "./cache/engine-profile-cache.js";
import { ENGINE_PROFILE_CACHE_SCHEMA_VERSION } from "./cache/engine-profile-cache.js";
import type { GodotProbeRunner } from "@solaris/core";
import { createGodotEngineProfiler, type GodotEngineProfiler } from "./profile/engine-profiler.js";
import {
  createGodotProjectInspector,
  type GodotProjectInspector,
} from "./project/project-inspector.js";

export interface GodotInspectorDependencies {
  readonly config: UserGodotConfig;
  readonly preference: GodotSelectionPreference;
  readonly overrideSource: "cli" | "environment" | null;
  readonly workspaceRoot: string;
  readonly backend: SandboxBackend;
  readonly probeRunner: GodotProbeRunner;
  readonly cache: GodotEngineProfileCache;
  readonly hostPath: string | null;
  readonly hostPathExt: string | null;
  readonly platform: NodeJS.Platform;
  readonly onEvent?: (event: GodotApplicationEvent) => void;
}

/**
 * Provider-neutral Godot inspector: composes engine discovery/profiling and
 * static project inspection. The CLI and provider tools consume only this
 * port; absolute executable paths never appear in its results.
 */
export function createGodotInspector(dependencies: GodotInspectorDependencies): GodotInspector {
  const profiler: GodotEngineProfiler = createGodotEngineProfiler(dependencies);
  const projectInspector: GodotProjectInspector = createGodotProjectInspector({
    workspaceRoot: dependencies.workspaceRoot,
    ...(dependencies.onEvent === undefined ? {} : { onEvent: dependencies.onEvent }),
  });

  async function discover(signal?: AbortSignal): Promise<GodotDiscoveryResult> {
    return profiler.discover(signal);
  }

  async function selected(signal?: AbortSignal): Promise<GodotSelectedInstallation | null> {
    const selection = await profiler.selectedProfile(signal);
    if (selection === null) {
      return null;
    }
    const { installation, profile } = selection;
    return {
      installationId: installation.id,
      sourceLabel: installation.sourceLabel,
      source: installation.source,
      version: profile.version,
      edition: profile.edition,
      editionConfidence: profile.editionConfidence,
      releaseChannel: profile.releaseChannel,
      support: profile.support,
      capabilities: profile.capabilities,
      verifiedCapabilities: profile.verifiedCapabilities,
      degradedCapabilities: profile.degradedCapabilities,
      executableFingerprint: profile.fingerprint,
      apiDumpSha256: profile.apiDumpSha256,
      diagnostics: profile.diagnostics,
    };
  }

  async function projectProfile(signal?: AbortSignal): Promise<GodotProjectProfile> {
    return projectInspector.inspect(signal);
  }

  async function compatibility(signal?: AbortSignal): Promise<GodotCompatibilityAssessment> {
    const [project, engine] = await Promise.all([
      projectInspector.inspect(signal),
      selectedEngineProfile(signal),
    ]);
    return assessGodotCompatibility(engine, project);
  }

  async function selectedEngineProfile(signal?: AbortSignal): Promise<GodotEngineProfile | null> {
    const selection = await profiler.selectedProfile(signal);
    return selection?.profile ?? null;
  }

  async function doctor(signal?: AbortSignal): Promise<GodotDoctorReport> {
    const [discovery, project] = await Promise.all([
      profiler.discover(signal),
      projectInspector.inspect(signal),
    ]);
    const selectedProfile = await selectedEngineProfile(signal);
    const compatibilityAssessment = assessGodotCompatibility(selectedProfile, project);
    let sandboxStatus;
    try {
      sandboxStatus = await dependencies.backend.inspect();
    } catch {
      sandboxStatus = {
        backendId: "unknown",
        state: "failed",
        platform: "unknown",
        version: "unknown",
        capabilities: {
          filesystemReadRestriction: false,
          filesystemWriteRestriction: false,
          networkRestriction: false,
          processTreeRestriction: false,
          violationReporting: false,
        },
      };
    }
    const probes: {
      readonly installationId: string;
      readonly probe: string;
      readonly status: string;
    }[] = [];
    for (const candidate of discovery.candidates) {
      if (candidate.profiled) {
        probes.push({
          installationId: candidate.installationId,
          probe: "profile",
          status: "success",
        });
      } else if (candidate.invalid !== null) {
        probes.push({
          installationId: candidate.installationId,
          probe: "profile",
          status: "failed",
        });
      }
    }
    return {
      discovery,
      project,
      compatibility: compatibilityAssessment,
      cache: {
        schemaVersion: ENGINE_PROFILE_CACHE_SCHEMA_VERSION,
        cachedProfileCount: await dependencies.cache.count().catch(() => 0),
        enabled: true,
      },
      sandbox: {
        state: sandboxStatus.state,
        backendId: sandboxStatus.backendId,
        networkRestriction: sandboxStatus.capabilities.networkRestriction,
        filesystemWriteRestriction: sandboxStatus.capabilities.filesystemWriteRestriction,
        processTreeRestriction: sandboxStatus.capabilities.processTreeRestriction,
      },
      probes,
    };
  }

  return { discover, selected, projectProfile, compatibility, doctor };
}
