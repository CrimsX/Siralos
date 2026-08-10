import type {
  GDScriptDevelopmentStatus,
  GodotProjectProbeStatus,
  ResearchService,
  SolarisApplication,
  SolarisSecurity,
} from "@solaris/core";
import { formatQualitySummary, type StatusView } from "../output.js";
import type { SessionInfo } from "./session-types.js";

export async function buildSessionStatusView(
  application: SolarisApplication,
  sessionInfo: SessionInfo,
): Promise<StatusView> {
  const inspection = await sessionInfo.git.inspectRepository().catch(() => null);
  const statusResult =
    inspection?.repositoryState === "repository"
      ? await sessionInfo.git.getStatus({}).catch(() => null)
      : null;
  const checkpoints = await sessionInfo.checkpoints.list().catch(() => []);
  const latestApplied = checkpoints.find((checkpoint) => checkpoint.state === "applied");
  const processDecision = sessionInfo.security.evaluateCapability("process.execute");
  const researchDecision = sessionInfo.security.evaluateCapability("research.fetch");
  const godotSelected = await sessionInfo.godot.selected().catch(() => null);
  const godotProject = await sessionInfo.godot.projectProfile().catch(() => null);
  const godotCompatibility = await sessionInfo.godot.compatibility().catch(() => null);
  return {
    status: application.getStatus(),
    workspaceRoot: sessionInfo.workspaceRoot,
    toolCount: sessionInfo.tools.length,
    providerToolCount: sessionInfo.tools.filter(
      (info) => sessionInfo.security.evaluateCapability(info.capability).decision !== "deny",
    ).length,
    profileId: sessionInfo.security.profile.id,
    gitRepositoryState: inspection?.repositoryState ?? "unavailable",
    gitBranch:
      inspection?.repositoryState === "repository" && statusResult !== null
        ? statusResult.branch.detached
          ? `(detached) ${statusResult.branch.oid ?? "unknown"}`
          : statusResult.branch.head
        : null,
    gitDirtyCount:
      statusResult === null
        ? 0
        : statusResult.changes.length +
          statusResult.conflicts.length +
          statusResult.untracked.length,
    latestCheckpoint: latestApplied === undefined ? null : shortenIdentifier(latestApplied.id),
    uncertainCheckpointCount: checkpoints.filter((checkpoint) => checkpoint.state === "uncertain")
      .length,
    processPermission:
      processDecision.decision === "deny"
        ? "denied"
        : processDecision.decision === "ask"
          ? "approval required"
          : "allowed",
    runnerCount: sessionInfo.runners.definitions.length,
    activeCommandId: application.getStatus().activeCommandId,
    lastCommandExitCode: application.getLastCommandExitCode(),
    commandProfile: "validation-offline",
    godotSelectedInstallation: godotSelected?.installationId ?? null,
    godotVersion: godotSelected?.version.raw ?? null,
    godotProjectDetected: godotProject?.detected ?? false,
    godotCompatibility: godotCompatibility?.status ?? null,
    godotWarningCount: godotProject?.warnings.length ?? 0,
    projectProbe: describeProjectProbe(sessionInfo.godotProbe.status()),
    knowledge: describeKnowledge(sessionInfo.knowledge.status()),
    languageSession: describeLanguageSession(sessionInfo.language.status()),
    developmentQuality: describeDevelopmentQuality(sessionInfo.development.status()),
    research: describeResearch(sessionInfo.research, researchDecision),
  };
}

function describeResearch(
  service: ResearchService,
  decision: ReturnType<SolarisSecurity["evaluateCapability"]>,
): string {
  const state = decision.decision === "allow" ? "enabled" : "disabled";
  const count = service.sourceKinds().length;
  return `${state} (${count} source${count === 1 ? "" : "s"})`;
}

function describeDevelopmentQuality(status: GDScriptDevelopmentStatus): string | null {
  if (status.session === null) {
    return null;
  }
  const quality = status.session.quality;
  if (quality.report === null && quality.status === null) {
    return "Quality: not run";
  }
  return formatQualitySummary(quality.report, quality.blockingFindings, quality.advisories);
}

function describeLanguageSession(status: {
  readonly state: string;
  readonly engineVersion: string | null;
  readonly networkIsolation: string;
}): string {
  return status.state === "ready"
    ? `active (${status.engineVersion ?? "?"}, ${status.networkIsolation})`
    : "inactive";
}

function describeKnowledge(status: { readonly state: string }): string {
  return status.state === "ready" ? "ready (exact engine API docs)" : "unavailable";
}

function describeProjectProbe(status: {
  readonly state: string;
  readonly lastResult: GodotProjectProbeStatus["lastResult"];
}): string {
  if (status.lastResult === null) {
    return status.state === "probe-invalidated" ? "approval invalidated" : "never run";
  }
  const diagnostics = status.lastResult.diagnostics;
  const count = diagnostics.errors.length + diagnostics.warnings.length;
  const summary = count === 0 ? "no diagnostics" : `${count} diagnostic${count === 1 ? "" : "s"}`;
  return `${status.lastResult.status} with ${summary}`;
}

function shortenIdentifier(id: string): string {
  return id.length > 12 ? id.slice(0, 12) : id;
}
