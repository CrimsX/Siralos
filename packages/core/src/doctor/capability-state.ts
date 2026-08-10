import type {
  CapabilitySnapshot,
  CapabilityState,
  GodotCapabilityStatus,
  ProviderCapabilityStatus,
  ReferenceCapabilityStatus,
  ResearchCapabilityStatus,
  SandboxCapabilityStatus,
  ToolCapabilityStatus,
  WorkspaceCapabilityStatus,
} from "./doctor-model.js";
import type {
  CapabilityDiagnosticResult,
  GodotDiagnosticResult,
  ProviderDiagnosticResult,
  ReferenceDiagnosticResult,
  ResearchDiagnosticResult,
  RuntimeDiagnosticResult,
  SandboxDiagnosticResult,
  WorkspaceDiagnosticResult,
} from "./doctor-ports.js";

/**
 * Deterministic CapabilitySnapshot builder (Stage 3 milestone 6).
 *
 * Pure — no I/O. The doctor collects the authoritative subsystem results
 * once (via DoctorSources) and derives the snapshot from them, so the
 * snapshot is observation of the same data the checks report and can
 * never double-probe or drift from the checks. It grants nothing.
 */

export interface CapabilitySnapshotInput {
  readonly runtime: RuntimeDiagnosticResult;
  readonly providers: ProviderDiagnosticResult;
  readonly sandbox: SandboxDiagnosticResult;
  readonly workspace: WorkspaceDiagnosticResult;
  readonly godot: GodotDiagnosticResult;
  readonly references: ReferenceDiagnosticResult;
  readonly research: ResearchDiagnosticResult;
  readonly tools: CapabilityDiagnosticResult;
}

export function buildCapabilitySnapshot(input: CapabilitySnapshotInput): CapabilitySnapshot {
  const providerStatuses: ProviderCapabilityStatus[] = [
    {
      profileId: input.providers.active.profileId,
      supported: true,
      configured: true,
      toolCalling: input.providers.active.toolCalling,
      state: input.providers.active.state,
      reason: input.providers.active.reason,
    },
  ];
  if (input.providers.reviewProvider.profileId !== null) {
    providerStatuses.push({
      profileId: input.providers.reviewProvider.profileId,
      supported: input.providers.reviewProvider.resolved,
      configured: input.providers.reviewProvider.configured,
      toolCalling: null,
      state: input.providers.reviewProvider.state,
      reason: input.providers.reviewProvider.reason,
    });
  }

  const sandboxState: CapabilityState =
    input.sandbox.backend.state === "available"
      ? input.sandbox.requiredCapabilitiesMissing.length > 0
        ? "degraded"
        : "available"
      : input.sandbox.backend.state === "unsupported"
        ? "unsupported"
        : input.sandbox.backend.state === "failed"
          ? "unavailable"
          : input.sandbox.backend.state === "degraded"
            ? "degraded"
            : "configured";

  const workspaceState: CapabilityState =
    input.workspace.readable && input.workspace.namespaceIntegrity ? "available" : "unavailable";

  const godotState: CapabilityState = !input.godot.report.discovery.selected
    ? input.godot.report.discovery.candidates.length === 0
      ? "unsupported"
      : "configured"
    : input.godot.report.degradedCapabilities.length > 0
      ? "degraded"
      : "available";

  const readyCount = input.references.references.filter((entry) => entry.status === "ready").length;
  const referenceState: CapabilityState =
    input.references.references.length === 0
      ? "unsupported"
      : readyCount === 0
        ? "unavailable"
        : "available";

  const researchState: CapabilityState =
    input.research.gate === "blocked_by_policy" ? "blocked_by_policy" : "available";

  const projected = input.tools.tools;

  const sandbox: SandboxCapabilityStatus = {
    backendId: input.sandbox.backend.backendId,
    backendState: input.sandbox.backend.state,
    selectedProfileId: input.sandbox.selectedProfileId,
    enforcement: {
      filesystemReadRestriction: input.sandbox.backend.capabilities.filesystemReadRestriction,
      filesystemWriteRestriction: input.sandbox.backend.capabilities.filesystemWriteRestriction,
      networkRestriction: input.sandbox.backend.capabilities.networkRestriction,
      processTreeRestriction: input.sandbox.backend.capabilities.processTreeRestriction,
    },
    unrestrictedFallback: input.sandbox.unrestrictedFallback,
    state: sandboxState,
    reason:
      input.sandbox.requiredCapabilitiesMissing.length > 0
        ? `required enforcement missing: ${input.sandbox.requiredCapabilitiesMissing.join(", ")}`
        : input.sandbox.backend.state === "available"
          ? null
          : (input.sandbox.backend.message ?? input.sandbox.backend.state),
  };

  const workspace: WorkspaceCapabilityStatus = {
    root: input.workspace.root,
    readable: input.workspace.readable,
    protectedPathsActive: input.workspace.protectedPathsActive,
    gitAvailable: input.workspace.gitAvailable,
    checkpointStoreAccessible: input.workspace.checkpointStoreAccessible,
    revisionRegistryOperational: input.workspace.revisionRegistryOperational,
    state: workspaceState,
    reason: workspaceState === "available" ? null : "workspace read or namespace integrity failed",
  };

  const selected = input.godot.report.discovery.selected;
  const godot: GodotCapabilityStatus = {
    detected: input.godot.report.discovery.candidates.length > 0,
    selected: selected !== null,
    version: selected?.version?.raw ?? null,
    edition: selected?.edition ?? null,
    fingerprint: selected?.fingerprint ?? null,
    support: selected?.support ?? null,
    engineProfileAvailable: selected?.profiled ?? false,
    apiCacheStale:
      input.godot.versionMatch.state === "stale"
        ? true
        : input.godot.versionMatch.state === "exact"
          ? false
          : null,
    recoveryProbeState: input.godot.report.recoveryProbe.state,
    lspState: "unavailable",
    state: godotState,
    reason: null,
  };

  const references: ReferenceCapabilityStatus = {
    configuredCount: input.references.references.length,
    readyCount,
    failedCount: input.references.references.filter(
      (entry) => entry.status === "declined" || entry.status === "resolution-failed",
    ).length,
    state: referenceState,
    reason: referenceState === "unavailable" ? "no configured reference is ready" : null,
  };

  const research: ResearchCapabilityStatus = {
    sourceKinds: input.research.sources.map((source) => source.kind),
    policyRule: input.research.policyRule,
    gate: input.research.gate,
    state: researchState,
    reason: input.research.gate === "blocked_by_policy" ? "research.fetch denied by policy" : null,
  };

  const tools: ToolCapabilityStatus = {
    projectedAvailable: projected.filter((tool) => tool.state === "available").length,
    projectedGated: projected.filter((tool) => tool.state === "gated").length,
    projectedHidden: projected.filter((tool) => tool.state === "hidden").length,
    state: projected.length === 0 ? "unknown" : "available",
    reason: null,
  };

  return {
    runtime: {
      version: input.runtime.version,
      nodeMajor: input.runtime.nodeMajor,
      platform: input.runtime.platform,
    },
    providers: providerStatuses,
    sandbox,
    workspace,
    godot,
    references,
    research,
    tools,
  };
}
