import { basename } from "node:path";
import {
  classifyGodotEdition,
  classifyGodotReleaseChannel,
  classifyGodotSupport,
  GODOT_SELECTION_RANKS,
  rankGodotCandidates,
  type GodotApplicationEvent,
  type GodotDiscoveryResult,
  type GodotEngineProfile,
  type GodotInstallation,
  type GodotInstallationOverview,
  type GodotProbeRunner,
  type GodotSelectionPreference,
  type SafeDiagnostic,
  type SandboxBackend,
} from "@solaris/core";
import type { UserGodotConfig } from "../../config/user-config.js";
import {
  ENGINE_PROFILE_CACHE_SCHEMA_VERSION,
  type CachedEngineProfile,
  type GodotEngineProfileCache,
} from "../cache/engine-profile-cache.js";
import { validateExecutable } from "../discovery/executable-validation.js";
import { resolveMacOsBundle } from "../discovery/macos-bundle.js";
import {
  discoverOnPath,
  invalidInstallation,
  installationFromIdentity,
} from "../discovery/path-discovery.js";
import { GodotSelectionError } from "../errors.js";

const VERIFIED_BASELINE = { major: 4, minor: 7, patch: 1 };

export interface GodotEngineProfilerDependencies {
  readonly config: UserGodotConfig;
  readonly preference: GodotSelectionPreference;
  /** Precedence level of the explicit override ("cli" or "environment"), if any. */
  readonly overrideSource: "cli" | "environment" | null;
  readonly workspaceRoot: string;
  readonly backend: SandboxBackend;
  readonly probeRunner: GodotProbeRunner;
  readonly cache: GodotEngineProfileCache;
  /** Sanitized host PATH value for discovery. */
  readonly hostPath: string | null;
  readonly hostPathExt: string | null;
  readonly platform: NodeJS.Platform;
  readonly onEvent?: (event: GodotApplicationEvent) => void;
}

export interface GodotEngineProfiler {
  discover(signal?: AbortSignal): Promise<GodotDiscoveryResult>;
}

export interface GodotProfiledCandidate {
  readonly installation: GodotInstallation;
  readonly profile: GodotEngineProfile | null;
  readonly profileError: string | null;
}

export function createGodotEngineProfiler(
  dependencies: GodotEngineProfilerDependencies,
): GodotEngineProfiler {
  let cached: GodotDiscoveryResult | null = null;

  function emit(event: GodotApplicationEvent): void {
    dependencies.onEvent?.(event);
  }

  async function discover(signal?: AbortSignal): Promise<GodotDiscoveryResult> {
    if (cached !== null) {
      return cached;
    }
    emit({ type: "godot_discovery_started" });
    const { candidates, duplicates } = await collectCandidates(signal);
    const profiled: GodotProfiledCandidate[] = [];
    for (const installation of candidates) {
      if (signal?.aborted) {
        throw createAbortError();
      }
      profiled.push(await profileCandidate(installation, signal));
    }
    const selection = await select(profiled);
    const diagnostics: SafeDiagnostic[] = [];
    if (selection.configActiveError !== null) {
      diagnostics.push({ severity: "warning", message: selection.configActiveError });
    }
    for (const candidate of profiled) {
      if (candidate.profileError !== null) {
        diagnostics.push({
          severity: "warning",
          message: `Installation ${candidate.installation.id}: ${candidate.profileError}`,
        });
      }
    }
    const overviews = candidates.map((installation) => {
      const candidate = profiled.find((entry) => entry.installation === installation);
      const profile = candidate?.profile ?? null;
      return {
        installationId: installation.id,
        version: profile?.version ?? null,
        edition: profile?.edition ?? null,
        editionConfidence: profile?.editionConfidence ?? null,
        releaseChannel: profile?.releaseChannel ?? null,
        sourceLabel: installation.sourceLabel,
        source: installation.source,
        support: profile?.support ?? null,
        invalid:
          candidate?.profileError ??
          (installation.status === "invalid"
            ? (installation.error ?? "invalid installation")
            : null),
        isDuplicate: duplicates.has(installation.id),
        selected: selection.installation !== null && installation.id === selection.installation.id,
        fingerprint: profile?.fingerprint ?? null,
        profiled: profile !== null,
      } satisfies GodotInstallationOverview;
    });
    const result: GodotDiscoveryResult = {
      candidates: overviews,
      configuration: {
        activeInstallation: dependencies.config.activeInstallation,
        configuredCount: Object.keys(dependencies.config.installations).length,
        discoverOnPath: dependencies.config.discoverOnPath,
        overrides: describeOverrides(dependencies.preference),
      },
      selected: overviews.find((overview) => overview.selected) ?? null,
      rationale: selection.rationale,
      diagnostics,
    };
    cached = result;
    return result;
  }

  async function collectCandidates(signal?: AbortSignal): Promise<{
    readonly candidates: readonly GodotInstallation[];
    readonly duplicates: ReadonlySet<string>;
  }> {
    const candidates: GodotInstallation[] = [];
    const isExplicitPath = dependencies.preference.kind === "path";
    if (isExplicitPath) {
      const path = dependencies.preference.path;
      let executablePath = path;
      if (dependencies.platform === "darwin" && path.endsWith(".app")) {
        const resolved = await resolveBundlePath(path);
        if (resolved === null) {
          candidates.push(
            invalidInstallation(
              "explicit",
              dependencies.overrideSource === "cli" ? "cli-path" : "environment-path",
              dependencies.overrideSource === "cli" ? "CLI --godot-path" : "SOLARIS_GODOT",
              "The configured Godot application bundle is invalid.",
            ),
          );
          return { candidates, duplicates: new Set() };
        }
        executablePath = resolved;
      }
      const validated = await validateExecutable({
        path: executablePath,
        workspaceRoot: dependencies.workspaceRoot,
        ...(signal === undefined ? {} : { signal }),
      });
      if (validated.ok) {
        candidates.push(
          installationFromIdentity(
            "explicit",
            dependencies.overrideSource === "cli" ? "cli-path" : "environment-path",
            dependencies.overrideSource === "cli" ? "CLI --godot-path" : "SOLARIS_GODOT",
            validated.identity,
            "unknown",
          ),
        );
      } else {
        candidates.push(
          invalidInstallation(
            "explicit",
            dependencies.overrideSource === "cli" ? "cli-path" : "environment-path",
            dependencies.overrideSource === "cli" ? "CLI --godot-path" : "SOLARIS_GODOT",
            validated.error,
          ),
        );
      }
      emit({
        type: "godot_candidate_found",
        installationId: "explicit",
        source: dependencies.overrideSource === "cli" ? "CLI --godot-path" : "SOLARIS_GODOT",
      });
      return { candidates, duplicates: new Set() };
    }
    for (const [id, installationConfig] of Object.entries(dependencies.config.installations)) {
      if (signal?.aborted) {
        throw createAbortError();
      }
      let executablePath = installationConfig.path;
      if (dependencies.platform === "darwin" && installationConfig.path.endsWith(".app")) {
        const resolved = await resolveBundlePath(installationConfig.path);
        if (resolved === null) {
          candidates.push(
            invalidInstallation(
              id,
              "user-config",
              "user config",
              "The bundle is not a valid Godot application bundle.",
            ),
          );
          emit({ type: "godot_candidate_found", installationId: id, source: "user config" });
          continue;
        }
        executablePath = resolved;
      }
      const validated = await validateExecutable({
        path: executablePath,
        workspaceRoot: dependencies.workspaceRoot,
        ...(signal === undefined ? {} : { signal }),
      });
      if (validated.ok) {
        candidates.push(
          installationFromIdentity(
            id,
            "user-config",
            "user config",
            validated.identity,
            installationConfig.editionHint,
          ),
        );
      } else {
        candidates.push(invalidInstallation(id, "user-config", "user config", validated.error));
      }
      emit({ type: "godot_candidate_found", installationId: id, source: "user config" });
    }
    if (dependencies.config.discoverOnPath) {
      const pathResult = await discoverOnPath({
        hostPath: dependencies.hostPath,
        hostPathExt: dependencies.hostPathExt,
        platform: dependencies.platform,
        workspaceRoot: dependencies.workspaceRoot,
        ...(signal === undefined ? {} : { signal }),
      });
      for (const candidate of pathResult.candidates) {
        emit({ type: "godot_candidate_found", installationId: candidate.id, source: "PATH" });
      }
      candidates.push(...pathResult.candidates);
    }
    const { deduped, duplicates } = deduplicateCandidates(candidates);
    return { candidates: deduped, duplicates };
  }

  async function resolveBundlePath(path: string): Promise<string | null> {
    const bundle = await resolveMacOsBundle(path);
    return bundle.ok ? bundle.executablePath : null;
  }

  async function profileCandidate(
    installation: GodotInstallation,
    signal?: AbortSignal,
  ): Promise<GodotProfiledCandidate> {
    if (installation.status !== "valid") {
      return {
        installation,
        profile: null,
        profileError: installation.error ?? "invalid installation",
      };
    }
    const cachedProfile = await dependencies.cache.load(installation.sha256);
    if (cachedProfile !== null && (await cacheMatchesExecutable(cachedProfile, installation))) {
      return {
        installation,
        profile: profileFromCache(cachedProfile),
        profileError: null,
      };
    }
    emit({ type: "godot_probe_started", installationId: installation.id, probe: "version" });
    const versionProbe = await dependencies.probeRunner.probeVersion(installation, signal);
    if (versionProbe.status !== "success") {
      emit({
        type: "godot_probe_completed",
        installationId: installation.id,
        probe: "version",
        status: "failed",
      });
      return { installation, profile: null, profileError: versionProbe.message };
    }
    emit({
      type: "godot_probe_completed",
      installationId: installation.id,
      probe: "version",
      status: "success",
    });
    emit({ type: "godot_probe_started", installationId: installation.id, probe: "help" });
    const helpProbe = await dependencies.probeRunner.probeHelp(installation, signal);
    const diagnostics: SafeDiagnostic[] = [];
    const degradedCapabilities: string[] = [];
    if (helpProbe.status === "failed") {
      emit({
        type: "godot_probe_completed",
        installationId: installation.id,
        probe: "help",
        status: "failed",
      });
      return { installation, profile: null, profileError: helpProbe.message };
    }
    const capabilities = helpProbe.capabilities;
    if (helpProbe.status === "degraded") {
      degradedCapabilities.push("help");
      diagnostics.push({ severity: "warning", message: helpProbe.message });
      emit({
        type: "godot_probe_completed",
        installationId: installation.id,
        probe: "help",
        status: "degraded",
      });
    } else {
      emit({
        type: "godot_probe_completed",
        installationId: installation.id,
        probe: "help",
        status: "success",
      });
    }
    let apiDumpSha256: string | null = null;
    const verifiedCapabilities: string[] = ["version", "help"];
    if (capabilities.extensionApiDump) {
      emit({ type: "godot_probe_started", installationId: installation.id, probe: "api" });
      const apiProbe = await dependencies.probeRunner.dumpExtensionApi(installation, signal);
      if (apiProbe.status === "success") {
        apiDumpSha256 = apiProbe.summary.sha256;
        verifiedCapabilities.push("extension-api-dump");
        emit({
          type: "godot_probe_completed",
          installationId: installation.id,
          probe: "api",
          status: "success",
        });
      } else {
        degradedCapabilities.push("extension-api-dump");
        diagnostics.push({ severity: "warning", message: apiProbe.message });
        emit({
          type: "godot_probe_completed",
          installationId: installation.id,
          probe: "api",
          status: apiProbe.status,
        });
      }
    }
    const editionClassification = classifyGodotEdition({
      explicitHint: installation.editionHint,
      filename: basename(installation.canonicalPath),
      capabilities,
      apiConfigurationFeatures: [],
      probesSucceeded: {
        version: true,
        help: true,
        apiDump: apiDumpSha256 !== null,
      },
    });
    for (const conflict of editionClassification.conflicts) {
      diagnostics.push({ severity: "warning", message: conflict });
    }
    const isVerifiedBaseline = isExactVerifiedBaseline(
      versionProbe.version,
      editionClassification.edition,
      editionClassification.confidence,
    );
    const support = classifyGodotSupport({
      version: versionProbe.version,
      edition: editionClassification.edition,
      editionConfidence: editionClassification.confidence,
      isVerifiedBaseline,
    });
    const profile: GodotEngineProfile = {
      installationId: installation.id,
      fingerprint: installation.sha256.slice(0, 12),
      version: versionProbe.version,
      edition: editionClassification.edition,
      editionConfidence: editionClassification.confidence,
      releaseChannel: classifyGodotReleaseChannel(versionProbe.version),
      capabilities,
      verifiedCapabilities,
      degradedCapabilities,
      executableSha256: installation.sha256,
      apiDumpSha256,
      support,
      diagnostics,
    };
    const cachedEntry: CachedEngineProfile = {
      schemaVersion: ENGINE_PROFILE_CACHE_SCHEMA_VERSION,
      installationId: installation.id,
      executable: {
        canonicalPath: installation.canonicalPath,
        sizeBytes: installation.sizeBytes,
        modifiedAtMs: installation.modifiedAtMs,
        sha256: installation.sha256,
      },
      version: profile.version,
      edition: profile.edition,
      editionConfidence: profile.editionConfidence,
      releaseChannel: profile.releaseChannel,
      capabilities: profile.capabilities,
      verifiedCapabilities: profile.verifiedCapabilities,
      degradedCapabilities: profile.degradedCapabilities,
      apiDumpSha256: profile.apiDumpSha256,
      support: profile.support,
      probedAtMs: Date.now(),
      diagnostics: profile.diagnostics,
    };
    await dependencies.cache.store(cachedEntry).catch(() => undefined);
    return { installation, profile, profileError: null };
  }

  function select(profiled: readonly GodotProfiledCandidate[]): Promise<{
    readonly installation: GodotInstallation | null;
    readonly rationale: readonly string[];
    readonly configActiveError: string | null;
  }> {
    return Promise.resolve(selectSync(profiled));
  }

  function selectSync(profiled: readonly GodotProfiledCandidate[]): {
    readonly installation: GodotInstallation | null;
    readonly rationale: readonly string[];
    readonly configActiveError: string | null;
  } {
    const valid = profiled.filter(
      (candidate) => candidate.profile !== null && candidate.installation.status === "valid",
    );
    const rationale: string[] = [];
    const preference = dependencies.preference;
    if (preference.kind === "path") {
      const match = valid.find((candidate) => candidate.installation.id === "explicit");
      if (match === undefined) {
        throw new GodotSelectionError(
          "The explicit Godot path did not resolve to a valid, probed installation.",
        );
      }
      return {
        installation: match.installation,
        rationale: ["Explicitly selected by path (CLI or environment override)."],
        configActiveError: null,
      };
    }
    if (preference.kind === "installation-id") {
      const match = profiled.find(
        (candidate) => candidate.installation.id === preference.installationId,
      );
      if (match === undefined) {
        throw new GodotSelectionError(
          `The explicit Godot installation id does not exist: ${preference.installationId}`,
        );
      }
      if (match.profile === null) {
        throw new GodotSelectionError(
          `The explicit Godot installation id is invalid: ${preference.installationId}`,
        );
      }
      return {
        installation: match.installation,
        rationale: ["Explicitly selected by installation id (CLI or environment override)."],
        configActiveError: null,
      };
    }
    if (preference.kind === "config-active") {
      const activeId = dependencies.config.activeInstallation;
      const match = profiled.find((candidate) => candidate.installation.id === activeId);
      if (match === undefined || match.profile === null) {
        const message =
          match === undefined
            ? `The configured active installation "${activeId}" does not exist; falling back to automatic selection.`
            : `The configured active installation "${activeId}" is invalid; falling back to automatic selection.`;
        const automatic = selectAutomatic(valid, rationale);
        return { ...automatic, configActiveError: message };
      }
      return {
        installation: match.installation,
        rationale: [`Configured active installation: ${activeId}.`],
        configActiveError: null,
      };
    }
    const automatic = selectAutomatic(valid, rationale);
    return { ...automatic, configActiveError: null };
  }

  function selectAutomatic(
    valid: readonly GodotProfiledCandidate[],
    rationale: string[],
  ): {
    readonly installation: GodotInstallation | null;
    readonly rationale: readonly string[];
  } {
    const ranked = rankGodotCandidates(
      valid.map((candidate) => ({
        installation: candidate.installation,
        profile: candidate.profile as GodotEngineProfile,
      })),
    );
    const selectable = ranked.filter((candidate) => candidate.rank !== null);
    if (selectable.length === 0) {
      rationale.push("No selectable Godot installation was discovered.");
      return { installation: null, rationale };
    }
    const winner = selectable[0] as (typeof selectable)[number];
    const rankedLabel: Record<number, string> = {
      [GODOT_SELECTION_RANKS.verifiedBaseline]: "verified baseline stable standard editor",
      [GODOT_SELECTION_RANKS.compatibleStableStandard]: "compatible stable standard editor",
      [GODOT_SELECTION_RANKS.compatibleStableDotnet]: "compatible stable .NET editor",
      [GODOT_SELECTION_RANKS.prereleaseEditor]: "prerelease editor",
    };
    for (const candidate of selectable) {
      const label = rankedLabel[candidate.rank ?? 0] ?? "candidate";
      rationale.push(
        `Rank ${String(candidate.rank)}: ${candidate.installation.id} (${candidate.profile.version.raw}, ${label}).`,
      );
    }
    rationale.push(
      `Selected ${winner.installation.id} (${winner.profile.version.raw}) by deterministic ranking.`,
    );
    return { installation: winner.installation, rationale };
  }

  return { discover };
}

function profileFromCache(cached: CachedEngineProfile): GodotEngineProfile {
  return {
    installationId: cached.installationId,
    fingerprint: cached.executable.sha256.slice(0, 12),
    version: cached.version,
    edition: cached.edition,
    editionConfidence: cached.editionConfidence,
    releaseChannel: cached.releaseChannel,
    capabilities: cached.capabilities,
    verifiedCapabilities: cached.verifiedCapabilities,
    degradedCapabilities: cached.degradedCapabilities,
    executableSha256: cached.executable.sha256,
    apiDumpSha256: cached.apiDumpSha256,
    support: cached.support,
    diagnostics: cached.diagnostics,
  };
}

async function cacheMatchesExecutable(
  cached: CachedEngineProfile,
  installation: GodotInstallation,
): Promise<boolean> {
  if (cached.executable.sha256 !== installation.sha256) {
    return false;
  }
  const { stat } = await import("node:fs/promises");
  try {
    const metadata = await stat(installation.canonicalPath);
    return (
      metadata.size === installation.sizeBytes && metadata.mtimeMs === installation.modifiedAtMs
    );
  } catch {
    return false;
  }
}

function isExactVerifiedBaseline(
  version: {
    readonly major: number;
    readonly minor: number;
    readonly patch: number | null;
    readonly status: string;
  },
  edition: string,
  editionConfidence: string,
): boolean {
  return (
    version.major === VERIFIED_BASELINE.major &&
    version.minor === VERIFIED_BASELINE.minor &&
    version.patch === VERIFIED_BASELINE.patch &&
    version.status === "stable" &&
    edition === "standard" &&
    editionConfidence === "high"
  );
}

function deduplicateCandidates(candidates: readonly GodotInstallation[]): {
  readonly deduped: readonly GodotInstallation[];
  readonly duplicates: ReadonlySet<string>;
} {
  const seen = new Set<string>();
  const duplicates = new Set<string>();
  const deduped: GodotInstallation[] = [];
  for (const candidate of candidates) {
    if (candidate.status === "invalid") {
      deduped.push(candidate);
      continue;
    }
    const key = candidate.canonicalPath.toLowerCase();
    if (seen.has(key)) {
      duplicates.add(candidate.id);
      continue;
    }
    seen.add(key);
    deduped.push(candidate);
  }
  return { deduped, duplicates };
}

function describeOverrides(preference: GodotSelectionPreference): readonly string[] {
  switch (preference.kind) {
    case "path":
      return ["explicit executable path override"];
    case "installation-id":
      return ["explicit installation id override"];
    case "config-active":
    case "auto":
    case "none":
      return [];
  }
}

function createAbortError(): Error {
  return new DOMException("Godot discovery was aborted.", "AbortError");
}
